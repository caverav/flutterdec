use anyhow::{anyhow, bail, Context, Result};
use goblin::elf::Elf;
use regex::bytes::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub mod dart_profile;
pub mod identity;
pub mod layout;
pub mod registry;

use dart_profile::ResolvedDartProfile;
use identity::{SnapshotIdentity, SnapshotKind, TargetArch};

#[derive(Debug, Clone)]
pub struct SnapshotBundle {
    pub input_path: PathBuf,
    /// Display path of the shared object the snapshot was read from. For an APK
    /// this is the member name, which is not a path any tool can open.
    pub libapp_path: PathBuf,
    /// The member of `input_path` the shared object came from, when it came from
    /// inside a container rather than from the filesystem.
    ///
    /// `libapp_path` alone cannot express the difference, and the difference is
    /// load bearing: an external backend given `lib/arm64-v8a/libapp.so` opens a
    /// path relative to wherever it happens to be running.
    pub libapp_entry: Option<String>,
    pub arch: String,
    pub snapshot_hash: String,
    pub vm_data: Vec<u8>,
    pub isolate_data: Vec<u8>,
    pub vm_instr: Vec<u8>,
    pub isolate_instr: Vec<u8>,
    pub vm_instr_va: u64,
    pub isolate_instr_va: u64,
    /// Dart version and layout profile for `snapshot_hash`, when the hash is known.
    pub dart_profile: Option<ResolvedDartProfile>,
    /// The snapshot's features string, when the header parsed.
    ///
    /// `WriteVersionAndFeatures` (`runtime/vm/app_snapshot.cc`) writes the
    /// 32-character snapshot hash then this string, NUL-terminated, at
    /// `kSnapshotHeaderSize`. It records the build flags the snapshot was
    /// produced with.
    pub snapshot_features: Option<String>,
    /// Whether the snapshot was built with compressed pointers, read from the
    /// features string rather than inferred.
    ///
    /// `Dart::FeaturesString` (`runtime/vm/dart.cc`) appends exactly
    /// `compressed-pointers` or `no-compressed-pointers`, so this is a fact
    /// about the binary. It decides the word size of a reference field, and the
    /// value of `kSmiBits`, and therefore which offset table applies. `None`
    /// means the header did not parse and nothing may be assumed.
    pub compressed_pointers: Option<bool>,
    /// The authoritative typed identity. `arch`, `snapshot_hash`,
    /// `snapshot_features`, and `compressed_pointers` above are flattened views
    /// of this value, populated from it so the two cannot disagree.
    /// Compatibility decisions must read this field: it is the only one that
    /// carries how well each fact is known.
    pub identity: SnapshotIdentity,
}

#[derive(Debug, Clone)]
struct SymbolSpan {
    va: u64,
    file_offset: usize,
    size: usize,
}

pub struct ApkSession {
    path: PathBuf,
    entry_names: Vec<String>,
    entry_index: HashMap<String, usize>,
    zip: RefCell<ZipArchive<fs::File>>,
    entry_cache: RefCell<HashMap<String, Vec<u8>>>,
}

impl ApkSession {
    pub fn open(path: &Path) -> Result<Self> {
        let mut zip = open_apk_zip(path)?;
        let mut entry_names = Vec::with_capacity(zip.len());
        let mut entry_index = HashMap::with_capacity(zip.len());
        for idx in 0..zip.len() {
            let entry = zip.by_index(idx)?;
            let name = entry.name().to_string();
            entry_index.entry(name.clone()).or_insert(idx);
            entry_names.push(name);
        }
        Ok(Self {
            path: path.to_path_buf(),
            entry_names,
            entry_index,
            zip: RefCell::new(zip),
            entry_cache: RefCell::new(HashMap::new()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn entry_names(&self) -> &[String] {
        &self.entry_names
    }

    pub fn read_entry(&self, entry_name: &str) -> Result<Vec<u8>> {
        if let Some(bytes) = self.entry_cache.borrow().get(entry_name) {
            return Ok(bytes.clone());
        }

        let index =
            self.entry_index.get(entry_name).copied().ok_or_else(|| {
                anyhow!("entry {} not found in {}", entry_name, self.path.display())
            })?;
        let mut out = Vec::new();
        {
            let mut zip = self.zip.borrow_mut();
            let mut entry = zip.by_index(index).with_context(|| {
                format!("open apk entry {} in {}", entry_name, self.path.display())
            })?;
            entry.read_to_end(&mut out).with_context(|| {
                format!("read apk entry {} in {}", entry_name, self.path.display())
            })?;
        }
        self.entry_cache
            .borrow_mut()
            .insert(entry_name.to_string(), out.clone());
        Ok(out)
    }
}

fn find_libapp_in_apk_session(apk: &ApkSession) -> Result<(PathBuf, Vec<u8>)> {
    let preferred = ["lib/arm64-v8a/libapp.so", "base/lib/arm64-v8a/libapp.so"];

    for want in preferred {
        if apk.entry_index.contains_key(want) {
            let out = apk.read_entry(want).context("read libapp from apk")?;
            return Ok((PathBuf::from(want), out));
        }
    }

    for name in apk.entry_names() {
        if name.ends_with("/libapp.so") || name == "libapp.so" {
            let out = apk.read_entry(name).context("read fallback libapp")?;
            return Ok((PathBuf::from(name), out));
        }
    }

    bail!("APK does not contain libapp.so");
}

fn open_apk_zip(path: &Path) -> Result<ZipArchive<fs::File>> {
    let file = fs::File::open(path).with_context(|| format!("open apk: {}", path.display()))?;
    ZipArchive::new(file).context("parse apk zip")
}

pub fn list_apk_entries(path: &Path) -> Result<Vec<String>> {
    let session = ApkSession::open(path)?;
    Ok(session.entry_names().to_vec())
}

pub fn read_apk_entry(path: &Path, entry_name: &str) -> Result<Vec<u8>> {
    let session = ApkSession::open(path)?;
    session.read_entry(entry_name)
}

fn va_to_offset(elf: &Elf<'_>, va: u64) -> Option<usize> {
    for ph in &elf.program_headers {
        let start = ph.p_vaddr;
        let end = start.saturating_add(ph.p_memsz);
        if va >= start && va < end {
            let delta = va - start;
            let off = ph.p_offset.saturating_add(delta);
            return usize::try_from(off).ok();
        }
    }
    None
}

fn collect_symbols(elf: &Elf<'_>) -> HashMap<String, (u64, u64)> {
    let mut out = HashMap::new();

    for sym in &elf.dynsyms {
        if let Some(name) = elf.dynstrtab.get_at(sym.st_name) {
            out.insert(name.to_string(), (sym.st_value, sym.st_size));
        }
    }
    for sym in &elf.syms {
        if let Some(name) = elf.strtab.get_at(sym.st_name) {
            out.insert(name.to_string(), (sym.st_value, sym.st_size));
        }
    }

    out
}

fn read_symbol_span(
    elf: &Elf<'_>,
    bytes: &[u8],
    symbols: &HashMap<String, (u64, u64)>,
    name: &str,
) -> Result<SymbolSpan> {
    let (va, size) = symbols
        .get(name)
        .copied()
        .ok_or_else(|| anyhow!("missing symbol {}", name))?;

    let offset =
        va_to_offset(elf, va).ok_or_else(|| anyhow!("cannot map VA for symbol {}", name))?;
    let size = usize::try_from(size).unwrap_or(0);
    if size == 0 {
        bail!("symbol {} has size 0; stripped/unsupported binary", name);
    }
    if offset >= bytes.len() || offset + size > bytes.len() {
        bail!("symbol {} range out of bounds", name);
    }

    Ok(SymbolSpan {
        va,
        file_offset: offset,
        size,
    })
}

/// Snapshot hash and features string, read from the header rather than scanned.
///
/// `runtime/vm/snapshot.h` fixes the layout: magic `0xdcdcf5f5`, then an `int64`
/// length and an `int64` kind, for a 20-byte header.
/// `WriteVersionAndFeatures` (`runtime/vm/app_snapshot.cc`) then writes
/// `Version::SnapshotString()`, which is the 32-character snapshot hash and
/// carries no separator, followed by the NUL-terminated features string.
fn parse_snapshot_header(bytes: &[u8]) -> Option<(String, SnapshotKind, String)> {
    const MAGIC: [u8; 4] = [0xf5, 0xf5, 0xdc, 0xdc];
    const HEADER_SIZE: usize = 20;
    const HASH_LEN: usize = 32;

    let start = bytes.windows(MAGIC.len()).position(|w| w == MAGIC)?;
    // `length` counts the bytes after it, so a real header's payload fits inside
    // the span. Checking it rejects a run of bytes that merely contains the
    // magic, which would otherwise yield a fabricated hash.
    let length = i64::from_le_bytes(bytes.get(start + 4..start + 12)?.try_into().ok()?);
    let remaining = i64::try_from(bytes.len().checked_sub(start)?).ok()?;
    if length <= 0 || length > remaining {
        return None;
    }
    // The kind field decides which serializer wrote the payload. Reading it here
    // rather than assuming AOT is what lets the caller refuse a JIT or core
    // snapshot instead of parsing it under the wrong contract.
    let kind = SnapshotKind::from_header_value(i64::from_le_bytes(
        bytes.get(start + 12..start + 20)?.try_into().ok()?,
    ));
    let hash_at = start.checked_add(HEADER_SIZE)?;
    let features_at = hash_at.checked_add(HASH_LEN)?;
    let hash = bytes.get(hash_at..features_at)?;
    if !hash.iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // The features string is bounded: a missing terminator means this is not a
    // header, not that the rest of the file is one string.
    let limit = features_at.saturating_add(1024).min(bytes.len());
    let end = bytes
        .get(features_at..limit)?
        .iter()
        .position(|b| *b == 0)?;
    let features = bytes.get(features_at..features_at + end)?;
    if !features.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return None;
    }
    Some((
        String::from_utf8_lossy(hash).to_ascii_lowercase(),
        kind,
        String::from_utf8_lossy(features).to_string(),
    ))
}

/// Whether the features string says the snapshot uses compressed pointers.
///
/// `Dart::FeaturesString` appends one of the two spellings, so the negative form
/// has to be tested first: `compressed-pointers` is a substring of
/// `no-compressed-pointers`.
fn compressed_pointers_from_features(features: &str) -> Option<bool> {
    match identity::FeatureEvidence::parse(features).pointer_compression() {
        identity::PointerCompression::Compressed => Some(true),
        identity::PointerCompression::Uncompressed => Some(false),
        // Silence and contradiction are both "nothing may be assumed"; the typed
        // identity keeps them apart for the caller that needs the difference.
        identity::PointerCompression::Unavailable | identity::PointerCompression::Conflicting => {
            None
        }
    }
}

fn detect_snapshot_hash(vm_data: &[u8], isolate_data: &[u8]) -> Option<String> {
    let mut probe = Vec::new();
    probe.extend_from_slice(&vm_data[..vm_data.len().min(65536)]);
    probe.extend_from_slice(&isolate_data[..isolate_data.len().min(65536)]);

    let pattern = Regex::new(r"([0-9a-f]{32})product\s+no-code_comments").expect("valid regex");
    if let Some(caps) = pattern.captures(&probe) {
        if let Some(m) = caps.get(1) {
            return Some(String::from_utf8_lossy(m.as_bytes()).to_string());
        }
    }

    let fallback = Regex::new(r"\b([0-9a-f]{32})\b").expect("valid regex");
    if let Some(caps) = fallback.captures(&probe) {
        if let Some(m) = caps.get(1) {
            return Some(String::from_utf8_lossy(m.as_bytes()).to_string());
        }
    }

    None
}

fn from_elf(
    path: &Path,
    libapp_display: PathBuf,
    libapp_entry: Option<String>,
    bytes: Vec<u8>,
) -> Result<SnapshotBundle> {
    let elf = Elf::parse(&bytes).context("parse ELF libapp")?;
    let arch = match elf.header.e_machine {
        goblin::elf::header::EM_AARCH64 => "arm64",
        _ => "unsupported",
    }
    .to_string();

    if arch != "arm64" {
        bail!("only Android ARM64 is supported in v1");
    }

    let symbols = collect_symbols(&elf);

    let vm_data = read_symbol_span(&elf, &bytes, &symbols, "_kDartVmSnapshotData")?;
    let isolate_data = read_symbol_span(&elf, &bytes, &symbols, "_kDartIsolateSnapshotData")?;
    let vm_instr = read_symbol_span(&elf, &bytes, &symbols, "_kDartVmSnapshotInstructions")?;
    let isolate_instr =
        read_symbol_span(&elf, &bytes, &symbols, "_kDartIsolateSnapshotInstructions")?;

    let vm_data_bytes = bytes[vm_data.file_offset..vm_data.file_offset + vm_data.size].to_vec();
    let isolate_data_bytes =
        bytes[isolate_data.file_offset..isolate_data.file_offset + isolate_data.size].to_vec();
    let vm_instr_bytes = bytes[vm_instr.file_offset..vm_instr.file_offset + vm_instr.size].to_vec();
    let isolate_instr_bytes =
        bytes[isolate_instr.file_offset..isolate_instr.file_offset + isolate_instr.size].to_vec();

    // The header is exact where it is present and also yields the features
    // string. The byte scan stays as the fallback: it tolerates a snapshot that
    // does not begin at the start of the span, and narrowing what the loader
    // accepts would silently degrade profile and adapter resolution instead of
    // failing.
    let header = parse_snapshot_header(&vm_data_bytes)
        .or_else(|| parse_snapshot_header(&isolate_data_bytes));
    let identity = match &header {
        Some((hash, kind, features)) => {
            SnapshotIdentity::from_header(TargetArch::Arm64, hash, *kind, features)
        }
        None => SnapshotIdentity::without_header(
            TargetArch::Arm64,
            detect_snapshot_hash(&vm_data_bytes, &isolate_data_bytes),
        ),
    };
    let hash = identity
        .hash
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let snapshot_features = identity.features.raw.clone();
    let compressed_pointers = snapshot_features
        .as_deref()
        .and_then(compressed_pointers_from_features);
    // Profile data is selected by the host registry after the identity gate.
    // The loader intentionally does not maintain a second hash inventory.

    Ok(SnapshotBundle {
        input_path: path.to_path_buf(),
        libapp_path: libapp_display,
        libapp_entry,
        arch,
        snapshot_hash: hash,
        vm_data: vm_data_bytes,
        isolate_data: isolate_data_bytes,
        vm_instr: vm_instr_bytes,
        isolate_instr: isolate_instr_bytes,
        vm_instr_va: vm_instr.va,
        isolate_instr_va: isolate_instr.va,
        dart_profile: None,
        snapshot_features,
        compressed_pointers,
        identity,
    })
}

pub fn load_snapshot_bundle_from_apk_session(
    path: &Path,
    apk: &ApkSession,
) -> Result<SnapshotBundle> {
    let (lib_path, lib_bytes) = find_libapp_in_apk_session(apk)?;
    let entry = lib_path.to_string_lossy().into_owned();
    from_elf(path, lib_path, Some(entry), lib_bytes)
}

pub fn load_snapshot_bundle(path: &Path) -> Result<SnapshotBundle> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "apk" {
        let apk = ApkSession::open(path)?;
        return load_snapshot_bundle_from_apk_session(path, &apk);
    }

    let bytes = fs::read(path).with_context(|| format!("read input file: {}", path.display()))?;
    from_elf(path, path.to_path_buf(), None, bytes)
}

#[cfg(test)]
mod tests {
    use super::identity::{
        HashSource, IdentityRejection, PointerCompression, SnapshotIdentity, SnapshotKind,
        TargetArch,
    };
    use super::{
        compressed_pointers_from_features, list_apk_entries, load_snapshot_bundle_from_apk_session,
        parse_snapshot_header, read_apk_entry, ApkSession,
    };
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn build_test_apk() -> std::path::PathBuf {
        let temp = tempdir().expect("tempdir");
        let apk_path = temp.path().join("sample.apk");
        let file = File::create(&apk_path).expect("create apk");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("classes.dex", options)
            .expect("start classes.dex");
        zip.write_all(b"dex-bytes").expect("write classes.dex");
        zip.start_file("AndroidManifest.xml", options)
            .expect("start manifest");
        zip.write_all(b"<manifest />").expect("write manifest");
        zip.start_file("lib/arm64-v8a/libapp.so", options)
            .expect("start libapp");
        zip.write_all(b"not-an-elf").expect("write libapp");
        zip.finish().expect("finish zip");
        let persisted = temp.path().to_path_buf();
        std::mem::forget(temp);
        persisted.join("sample.apk")
    }

    #[test]
    fn lists_apk_entries() {
        let apk = build_test_apk();
        let entries = list_apk_entries(&apk).expect("list entries");
        assert_eq!(
            entries,
            vec![
                "classes.dex",
                "AndroidManifest.xml",
                "lib/arm64-v8a/libapp.so"
            ]
        );
    }

    #[test]
    fn reads_apk_entry_bytes() {
        let apk = build_test_apk();
        let bytes = read_apk_entry(&apk, "classes.dex").expect("read entry");
        assert_eq!(bytes, b"dex-bytes");
    }

    #[test]
    fn apk_session_lists_and_reads_entries() {
        let apk = build_test_apk();
        let session = ApkSession::open(&apk).expect("open session");
        assert_eq!(
            session.entry_names(),
            &[
                "classes.dex".to_string(),
                "AndroidManifest.xml".to_string(),
                "lib/arm64-v8a/libapp.so".to_string()
            ]
        );
        assert_eq!(
            session.read_entry("classes.dex").expect("read classes.dex"),
            b"dex-bytes"
        );
        assert_eq!(
            session
                .read_entry("AndroidManifest.xml")
                .expect("read manifest"),
            b"<manifest />"
        );
    }

    #[test]
    fn snapshot_bundle_from_apk_session_requires_real_elf() {
        let apk = build_test_apk();
        let session = ApkSession::open(&apk).expect("open session");
        let err = load_snapshot_bundle_from_apk_session(&apk, &session).expect_err("non-elf fails");
        assert!(err.to_string().contains("parse ELF libapp"));
    }

    fn snapshot_blob(hash: &str, features: &str) -> Vec<u8> {
        snapshot_blob_of_kind(hash, features, 3)
    }

    fn snapshot_blob_of_kind(hash: &str, features: &str, kind: i64) -> Vec<u8> {
        // Eight bytes of padding before the header, so the parser has to find the
        // magic rather than assume offset zero.
        let lead = 8usize;
        let payload_len = 20 + hash.len() + features.len() + 1;
        let mut out = vec![0u8; lead];
        out.extend_from_slice(&[0xf5, 0xf5, 0xdc, 0xdc]);
        // `length` has to describe a payload that fits inside the span, which is
        // what makes the check able to reject a stray magic.
        out.extend_from_slice(&(payload_len as i64).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(hash.as_bytes());
        out.extend_from_slice(features.as_bytes());
        out.push(0);
        out
    }

    /// A header whose length does not fit the span is not a header. This is the
    /// check that makes a stray magic in a run of data rejectable, so it needs
    /// its own case rather than resting on the hash and features checks.
    #[test]
    fn rejects_a_header_whose_length_exceeds_the_span() {
        let mut blob = snapshot_blob(
            "80a49c7111088100a233b2ae788e1f48",
            "product arm64 compressed-pointers",
        );
        assert!(parse_snapshot_header(&blob).is_some(), "baseline parses");
        // Only the length field changes.
        blob[12..20].copy_from_slice(&i64::MAX.to_le_bytes());
        assert!(parse_snapshot_header(&blob).is_none());
    }

    /// The header fixes the layout, so the hash and features come from it rather
    /// than from a byte scan. `Version::SnapshotString()` is the 32-character
    /// hash with no separator before the features string.
    #[test]
    fn parses_hash_and_features_from_the_snapshot_header() {
        let blob = snapshot_blob(
            "80a49c7111088100a233b2ae788e1f48",
            "product no-code_comments arm64 android compressed-pointers",
        );
        let (hash, kind, features) = parse_snapshot_header(&blob).expect("header parses");
        assert_eq!(hash, "80a49c7111088100a233b2ae788e1f48");
        assert_eq!(kind, SnapshotKind::FullAot);
        assert!(features.ends_with("compressed-pointers"));
        assert_eq!(compressed_pointers_from_features(&features), Some(true));
    }

    /// `compressed-pointers` is a substring of `no-compressed-pointers`, so the
    /// negative spelling has to be tested first or every uncompressed snapshot
    /// reads as compressed.
    #[test]
    fn the_negative_pointer_mode_spelling_wins() {
        assert_eq!(
            compressed_pointers_from_features("product arm64 no-compressed-pointers"),
            Some(false)
        );
        assert_eq!(
            compressed_pointers_from_features("product arm64 compressed-pointers"),
            Some(true)
        );
        // Absent rather than false: nothing may be assumed from silence.
        assert_eq!(compressed_pointers_from_features("product arm64"), None);
    }

    /// A run of bytes that happens to contain the magic is not a header. Without
    /// the checks a stray match would yield a fabricated hash.
    #[test]
    fn rejects_a_stray_magic_that_is_not_a_header() {
        let mut blob = vec![0xf5, 0xf5, 0xdc, 0xdc];
        blob.extend_from_slice(&[0xffu8; 200]);
        assert!(parse_snapshot_header(&blob).is_none());
        // A valid hash but no terminated features string is also not a header.
        let mut truncated = vec![0u8; 20];
        truncated[0..4].copy_from_slice(&[0xf5, 0xf5, 0xdc, 0xdc]);
        truncated.extend_from_slice(b"80a49c7111088100a233b2ae788e1f48");
        truncated.extend_from_slice(&[b'x'; 2048]);
        assert!(parse_snapshot_header(&truncated).is_none());
    }

    const AOT_FEATURES: &str = "product no-code_comments arm64 android compressed-pointers";
    const AOT_HASH: &str = "80a49c7111088100a233b2ae788e1f48";

    /// Build the identity the loader would build for these header bytes, going
    /// through the real header parser rather than constructing it by hand.
    fn identity_from_blob(blob: &[u8]) -> SnapshotIdentity {
        match parse_snapshot_header(blob) {
            Some((hash, kind, features)) => {
                SnapshotIdentity::from_header(TargetArch::Arm64, &hash, kind, &features)
            }
            None => SnapshotIdentity::without_header(TargetArch::Arm64, None),
        }
    }

    /// The whole point of the type: a valid FullAOT header yields exact,
    /// header-sourced facts and a selection key that carries no kind and no
    /// semantic version.
    #[test]
    fn a_full_aot_header_yields_an_exact_identity_and_a_selection_key() {
        let identity = identity_from_blob(&snapshot_blob(AOT_HASH, AOT_FEATURES));

        assert_eq!(identity.hash.as_deref(), Some(AOT_HASH));
        assert_eq!(identity.hash_source, HashSource::Header);
        assert_eq!(identity.kind, Some(SnapshotKind::FullAot));
        assert_eq!(identity.target_arch, TargetArch::Arm64);
        assert_eq!(identity.pointer_compression, PointerCompression::Compressed);
        assert!(identity.is_exact());
        // Normalized evidence is sorted and deduplicated so it can be compared.
        assert_eq!(
            identity.features.normalized,
            vec![
                "android".to_string(),
                "arm64".to_string(),
                "compressed-pointers".to_string(),
                "no-code_comments".to_string(),
                "product".to_string(),
            ]
        );

        let key = identity.exact_selection_key().expect("gate passes");
        assert_eq!(key.hash, AOT_HASH);
        assert_eq!(key.target_arch, TargetArch::Arm64);
        assert_eq!(key.features, identity.features.normalized);
    }

    /// A hash written in uppercase is the same hash. Normalizing at the boundary
    /// is what keeps a registry lookup from missing on case alone.
    #[test]
    fn header_hashes_are_normalized_to_lowercase() {
        let blob = snapshot_blob("80A49C7111088100A233B2AE788E1F48", AOT_FEATURES);
        let identity = identity_from_blob(&blob);
        assert_eq!(identity.hash.as_deref(), Some(AOT_HASH));
    }

    /// FullAOT is a gate, so every other kind has to stop before selection even
    /// though its header is otherwise perfectly valid.
    #[test]
    fn non_full_aot_kinds_are_rejected_at_the_gate() {
        for (value, expected) in [
            (0i64, SnapshotKind::Full),
            (1, SnapshotKind::FullCore),
            (2, SnapshotKind::FullJit),
            (9, SnapshotKind::Unrecognized),
        ] {
            let blob = snapshot_blob_of_kind(AOT_HASH, AOT_FEATURES, value);
            let identity = identity_from_blob(&blob);
            assert_eq!(identity.kind, Some(expected), "kind {} decodes", value);
            // The hash is still exact; it is the kind that withholds authority.
            assert_eq!(identity.hash_source, HashSource::Header);
            assert_eq!(
                identity.exact_selection_key(),
                Err(IdentityRejection::NotFullAot(Some(expected))),
            );
        }
    }

    /// A malformed hash means the header did not parse at all, so the identity
    /// falls back rather than carrying a half-read hash.
    #[test]
    fn a_malformed_hash_leaves_no_header_identity() {
        let blob = snapshot_blob("80a49c7111088100a233b2ae788e1zzz", AOT_FEATURES);
        assert!(parse_snapshot_header(&blob).is_none());
        let identity = identity_from_blob(&blob);
        assert_eq!(identity.hash, None);
        assert_eq!(identity.hash_source, HashSource::Unavailable);
        assert_eq!(identity.kind, None);
        assert_eq!(
            identity.exact_selection_key(),
            Err(IdentityRejection::HashNotHeaderDerived(
                HashSource::Unavailable
            )),
        );
    }

    /// Both pointer spellings in one features string is not a snapshot the VM
    /// writes. Picking one would decide the word size of every reference field
    /// on a coin flip, so the conflict has to survive into the gate.
    #[test]
    fn conflicting_pointer_features_are_a_conflict_not_a_choice() {
        let blob = snapshot_blob(
            AOT_HASH,
            "product arm64 compressed-pointers no-compressed-pointers",
        );
        let identity = identity_from_blob(&blob);
        assert_eq!(
            identity.pointer_compression,
            PointerCompression::Conflicting
        );
        assert_eq!(
            identity.exact_selection_key(),
            Err(IdentityRejection::PointerCompressionUnavailable(
                PointerCompression::Conflicting
            )),
        );
    }

    /// Silence about pointer compression is also not a value.
    #[test]
    fn absent_pointer_features_stop_the_gate() {
        let blob = snapshot_blob(AOT_HASH, "product arm64 android");
        let identity = identity_from_blob(&blob);
        assert_eq!(
            identity.pointer_compression,
            PointerCompression::Unavailable
        );
        assert_eq!(
            identity.exact_selection_key(),
            Err(IdentityRejection::PointerCompressionUnavailable(
                PointerCompression::Unavailable
            )),
        );
    }

    /// A scanned hash is a 32-hex run that happened to be in a data section. It
    /// is evidence, not identity, and must not reach an exact parser.
    #[test]
    fn a_scan_only_hash_cannot_authorize_exact_selection() {
        let identity =
            SnapshotIdentity::without_header(TargetArch::Arm64, Some(AOT_HASH.to_string()));
        assert_eq!(identity.hash.as_deref(), Some(AOT_HASH));
        assert_eq!(identity.hash_source, HashSource::Scan);
        assert!(!identity.is_exact());
        assert_eq!(
            identity.exact_selection_key(),
            Err(IdentityRejection::HashNotHeaderDerived(HashSource::Scan)),
        );
    }

    /// The features string and the container have to agree about the target, or
    /// one of them is describing a different binary.
    #[test]
    fn a_features_target_conflicting_with_the_container_is_rejected() {
        let blob = snapshot_blob(AOT_HASH, "product x64 compressed-pointers");
        let identity = identity_from_blob(&blob);
        assert_eq!(
            identity.exact_selection_key(),
            Err(IdentityRejection::TargetArchConflict {
                declared: "x64".to_string(),
                container: "arm64".to_string(),
            }),
        );
    }

    /// Host architecture is not target architecture; an unsupported target stops
    /// before any lookup regardless of what the host happens to be.
    #[test]
    fn an_unsupported_target_stops_before_lookup() {
        let mut identity = identity_from_blob(&snapshot_blob(AOT_HASH, AOT_FEATURES));
        identity.target_arch = TargetArch::Unsupported("riscv64".to_string());
        assert_eq!(
            identity.exact_selection_key(),
            Err(IdentityRejection::UnsupportedTarget("riscv64".to_string())),
        );
    }

    /// Assemble a minimal ARM64 shared object carrying the four snapshot
    /// symbols, so `load_snapshot_bundle` can be exercised on bytes rather than
    /// on the header parser alone.
    ///
    /// The single `PT_LOAD` maps at address zero with file offset zero, which
    /// makes a symbol's virtual address equal its file offset and keeps the
    /// fixture readable. Only the pieces `goblin` needs to find a symbol table
    /// are present: a program header, `.symtab`, `.strtab`, and `.shstrtab`.
    fn synthetic_libapp(
        vm_data: &[u8],
        isolate_data: &[u8],
        vm_instr: &[u8],
        isolate_instr: &[u8],
    ) -> Vec<u8> {
        const EHDR: usize = 64;
        const PHDR: usize = 56;
        const SHDR: usize = 64;
        const SYM: usize = 24;

        let mut out = vec![0u8; 128];
        let place = |out: &mut Vec<u8>, bytes: &[u8]| -> (u64, u64) {
            let at = out.len() as u64;
            out.extend_from_slice(bytes);
            (at, bytes.len() as u64)
        };
        let spans = [
            place(&mut out, vm_data),
            place(&mut out, isolate_data),
            place(&mut out, vm_instr),
            place(&mut out, isolate_instr),
        ];

        let names = [
            "_kDartVmSnapshotData",
            "_kDartIsolateSnapshotData",
            "_kDartVmSnapshotInstructions",
            "_kDartIsolateSnapshotInstructions",
        ];
        let mut strtab = vec![0u8];
        let mut name_offsets = Vec::new();
        for name in names {
            name_offsets.push(strtab.len() as u32);
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }

        // Index 0 is the reserved null symbol.
        let mut symtab = vec![0u8; SYM];
        for (index, (value, size)) in spans.iter().enumerate() {
            symtab.extend_from_slice(&name_offsets[index].to_le_bytes());
            symtab.push(0x11); // STB_GLOBAL | STT_OBJECT
            symtab.push(0);
            symtab.extend_from_slice(&1u16.to_le_bytes()); // any defined section
            symtab.extend_from_slice(&value.to_le_bytes());
            symtab.extend_from_slice(&size.to_le_bytes());
        }

        let mut shstrtab = vec![0u8];
        let section_name = |shstrtab: &mut Vec<u8>, name: &str| -> u32 {
            let at = shstrtab.len() as u32;
            shstrtab.extend_from_slice(name.as_bytes());
            shstrtab.push(0);
            at
        };
        let symtab_name = section_name(&mut shstrtab, ".symtab");
        let strtab_name = section_name(&mut shstrtab, ".strtab");
        let shstrtab_name = section_name(&mut shstrtab, ".shstrtab");

        let symtab_off = out.len() as u64;
        out.extend_from_slice(&symtab);
        let strtab_off = out.len() as u64;
        out.extend_from_slice(&strtab);
        let shstrtab_off = out.len() as u64;
        out.extend_from_slice(&shstrtab);
        let shoff = out.len() as u64;

        let mut section =
            |name: u32, kind: u32, offset: u64, size: u64, link: u32, entsize: u64| {
                let mut hdr = Vec::with_capacity(SHDR);
                hdr.extend_from_slice(&name.to_le_bytes());
                hdr.extend_from_slice(&kind.to_le_bytes());
                hdr.extend_from_slice(&0u64.to_le_bytes()); // flags
                hdr.extend_from_slice(&0u64.to_le_bytes()); // addr
                hdr.extend_from_slice(&offset.to_le_bytes());
                hdr.extend_from_slice(&size.to_le_bytes());
                hdr.extend_from_slice(&link.to_le_bytes());
                hdr.extend_from_slice(&0u32.to_le_bytes()); // info
                hdr.extend_from_slice(&1u64.to_le_bytes()); // addralign
                hdr.extend_from_slice(&entsize.to_le_bytes());
                out.extend_from_slice(&hdr);
            };
        section(0, 0, 0, 0, 0, 0); // SHT_NULL
        section(
            symtab_name,
            2,
            symtab_off,
            symtab.len() as u64,
            2,
            SYM as u64,
        ); // SHT_SYMTAB
        section(strtab_name, 3, strtab_off, strtab.len() as u64, 0, 0); // SHT_STRTAB
        section(shstrtab_name, 3, shstrtab_off, shstrtab.len() as u64, 0, 0);

        let total = out.len() as u64;

        let mut header = Vec::with_capacity(EHDR);
        header.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
        header.extend_from_slice(&[0u8; 8]);
        header.extend_from_slice(&3u16.to_le_bytes()); // ET_DYN
        header.extend_from_slice(&183u16.to_le_bytes()); // EM_AARCH64
        header.extend_from_slice(&1u32.to_le_bytes()); // version
        header.extend_from_slice(&0u64.to_le_bytes()); // entry
        header.extend_from_slice(&(EHDR as u64).to_le_bytes()); // phoff
        header.extend_from_slice(&shoff.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // flags
        header.extend_from_slice(&(EHDR as u16).to_le_bytes());
        header.extend_from_slice(&(PHDR as u16).to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes()); // phnum
        header.extend_from_slice(&(SHDR as u16).to_le_bytes());
        header.extend_from_slice(&4u16.to_le_bytes()); // shnum
        header.extend_from_slice(&3u16.to_le_bytes()); // shstrndx
        out[..EHDR].copy_from_slice(&header);

        // One PT_LOAD covering the file at address zero.
        let mut phdr = Vec::with_capacity(PHDR);
        phdr.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        phdr.extend_from_slice(&5u32.to_le_bytes()); // R+X
        phdr.extend_from_slice(&0u64.to_le_bytes()); // offset
        phdr.extend_from_slice(&0u64.to_le_bytes()); // vaddr
        phdr.extend_from_slice(&0u64.to_le_bytes()); // paddr
        phdr.extend_from_slice(&total.to_le_bytes()); // filesz
        phdr.extend_from_slice(&total.to_le_bytes()); // memsz
        phdr.extend_from_slice(&0x1000u64.to_le_bytes()); // align
        out[EHDR..EHDR + PHDR].copy_from_slice(&phdr);

        out
    }

    fn load_synthetic(vm_data: &[u8]) -> super::SnapshotBundle {
        let bytes = synthetic_libapp(vm_data, &[0u8; 32], &[0x1fu8; 16], &[0x2fu8; 16]);
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("libapp.so");
        std::fs::write(&path, &bytes).expect("write libapp");
        super::load_snapshot_bundle(&path).expect("load synthetic libapp")
    }

    /// The identity a real load produces, not just the one the header parser
    /// returns. This is the wiring that decides what every downstream
    /// compatibility check sees.
    #[test]
    fn loading_a_real_container_produces_the_header_identity() {
        let bundle = load_synthetic(&snapshot_blob(AOT_HASH, AOT_FEATURES));

        assert_eq!(bundle.identity.hash.as_deref(), Some(AOT_HASH));
        assert_eq!(bundle.identity.hash_source, HashSource::Header);
        assert_eq!(bundle.identity.kind, Some(SnapshotKind::FullAot));
        assert_eq!(bundle.identity.target_arch, TargetArch::Arm64);
        assert_eq!(
            bundle.identity.pointer_compression,
            PointerCompression::Compressed
        );
        assert!(bundle.identity.exact_selection_key().is_ok());

        // The flattened fields are views of the identity, so they cannot drift.
        assert_eq!(bundle.snapshot_hash, AOT_HASH);
        assert_eq!(bundle.arch, "arm64");
        assert_eq!(bundle.snapshot_features.as_deref(), Some(AOT_FEATURES));
        assert_eq!(bundle.compressed_pointers, Some(true));
    }

    /// A JIT snapshot loads fine and is refused at the gate rather than at the
    /// parser, which is what keeps the refusal explainable.
    #[test]
    fn loading_a_jit_container_stops_at_the_gate_not_the_loader() {
        let bundle = load_synthetic(&snapshot_blob_of_kind(AOT_HASH, AOT_FEATURES, 2));
        assert_eq!(bundle.identity.kind, Some(SnapshotKind::FullJit));
        assert_eq!(
            bundle.identity.exact_selection_key(),
            Err(IdentityRejection::NotFullAot(Some(SnapshotKind::FullJit)))
        );
    }

    /// With no parseable header, the loader falls back to a byte scan. The hash
    /// it finds is recorded as scanned, so it reaches the gate and is refused
    /// instead of quietly selecting a parser.
    #[test]
    fn a_container_without_a_header_falls_back_to_a_scan_that_cannot_select() {
        let mut data = vec![0u8; 16];
        data.extend_from_slice(AOT_HASH.as_bytes());
        data.extend_from_slice(b"product no-code_comments arm64\0");
        let bundle = load_synthetic(&data);

        assert_eq!(bundle.identity.hash.as_deref(), Some(AOT_HASH));
        assert_eq!(bundle.identity.hash_source, HashSource::Scan);
        assert_eq!(bundle.identity.kind, None);
        assert_eq!(bundle.snapshot_features, None);
        assert_eq!(bundle.compressed_pointers, None);
        assert_eq!(
            bundle.identity.exact_selection_key(),
            Err(IdentityRejection::HashNotHeaderDerived(HashSource::Scan))
        );
    }

    /// Nothing hash-shaped anywhere: the hash is unavailable rather than a
    /// string that later compares against a registry key.
    #[test]
    fn a_container_with_no_hash_at_all_reports_it_as_unavailable() {
        let bundle = load_synthetic(&[0x5au8; 256]);
        assert_eq!(bundle.identity.hash, None);
        assert_eq!(bundle.identity.hash_source, HashSource::Unavailable);
        assert_eq!(bundle.snapshot_hash, "unknown");
        assert_eq!(
            bundle.identity.exact_selection_key(),
            Err(IdentityRejection::HashNotHeaderDerived(
                HashSource::Unavailable
            ))
        );
    }
}
