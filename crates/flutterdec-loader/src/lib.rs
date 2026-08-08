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

use dart_profile::ResolvedDartProfile;

#[derive(Debug, Clone)]
pub struct SnapshotBundle {
    pub input_path: PathBuf,
    pub libapp_path: PathBuf,
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
fn parse_snapshot_header(bytes: &[u8]) -> Option<(String, String)> {
    const MAGIC: [u8; 4] = [0xf5, 0xf5, 0xdc, 0xdc];
    const HEADER_SIZE: usize = 20;
    const HASH_LEN: usize = 32;

    let start = bytes.windows(MAGIC.len()).position(|w| w == MAGIC)?;
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
        String::from_utf8_lossy(features).to_string(),
    ))
}

/// Whether the features string says the snapshot uses compressed pointers.
///
/// `Dart::FeaturesString` appends one of the two spellings, so the negative form
/// has to be tested first: `compressed-pointers` is a substring of
/// `no-compressed-pointers`.
fn compressed_pointers_from_features(features: &str) -> Option<bool> {
    if features.contains("no-compressed-pointers") {
        return Some(false);
    }
    if features.contains("compressed-pointers") {
        return Some(true);
    }
    None
}

fn detect_snapshot_hash(vm_data: &[u8], isolate_data: &[u8]) -> String {
    let mut probe = Vec::new();
    probe.extend_from_slice(&vm_data[..vm_data.len().min(65536)]);
    probe.extend_from_slice(&isolate_data[..isolate_data.len().min(65536)]);

    let pattern = Regex::new(r"([0-9a-f]{32})product\s+no-code_comments").expect("valid regex");
    if let Some(caps) = pattern.captures(&probe) {
        if let Some(m) = caps.get(1) {
            return String::from_utf8_lossy(m.as_bytes()).to_string();
        }
    }

    let fallback = Regex::new(r"\b([0-9a-f]{32})\b").expect("valid regex");
    if let Some(caps) = fallback.captures(&probe) {
        if let Some(m) = caps.get(1) {
            return String::from_utf8_lossy(m.as_bytes()).to_string();
        }
    }

    "unknown".to_string()
}

fn from_elf(path: &Path, libapp_display: PathBuf, bytes: Vec<u8>) -> Result<SnapshotBundle> {
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
    let hash = match &header {
        Some((hash, _)) => hash.clone(),
        None => detect_snapshot_hash(&vm_data_bytes, &isolate_data_bytes),
    };
    let snapshot_features = header.map(|(_, features)| features);
    let compressed_pointers = snapshot_features
        .as_deref()
        .and_then(compressed_pointers_from_features);
    let dart_profile = dart_profile::profile_for_hash(&hash);

    Ok(SnapshotBundle {
        input_path: path.to_path_buf(),
        libapp_path: libapp_display,
        arch,
        snapshot_hash: hash,
        vm_data: vm_data_bytes,
        isolate_data: isolate_data_bytes,
        vm_instr: vm_instr_bytes,
        isolate_instr: isolate_instr_bytes,
        vm_instr_va: vm_instr.va,
        isolate_instr_va: isolate_instr.va,
        dart_profile,
        snapshot_features,
        compressed_pointers,
    })
}

pub fn load_snapshot_bundle_from_apk_session(
    path: &Path,
    apk: &ApkSession,
) -> Result<SnapshotBundle> {
    let (lib_path, lib_bytes) = find_libapp_in_apk_session(apk)?;
    from_elf(path, lib_path, lib_bytes)
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
    from_elf(path, path.to_path_buf(), bytes)
}

#[cfg(test)]
mod tests {
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
        let mut out = vec![0u8; 8];
        out.extend_from_slice(&[0xf5, 0xf5, 0xdc, 0xdc]);
        out.extend_from_slice(&1234i64.to_le_bytes());
        out.extend_from_slice(&3i64.to_le_bytes());
        out.extend_from_slice(hash.as_bytes());
        out.extend_from_slice(features.as_bytes());
        out.push(0);
        out
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
        let (hash, features) = parse_snapshot_header(&blob).expect("header parses");
        assert_eq!(hash, "80a49c7111088100a233b2ae788e1f48");
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
}
