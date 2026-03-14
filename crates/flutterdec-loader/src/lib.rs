use anyhow::{anyhow, bail, Context, Result};
use goblin::elf::Elf;
use regex::bytes::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

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

    let hash = detect_snapshot_hash(&vm_data_bytes, &isolate_data_bytes);

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
        list_apk_entries, load_snapshot_bundle_from_apk_session, read_apk_entry, ApkSession,
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
}
