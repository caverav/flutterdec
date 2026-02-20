use anyhow::{anyhow, bail, Context, Result};
use goblin::elf::Elf;
use regex::bytes::Regex;
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

fn find_libapp_in_apk(path: &Path) -> Result<(PathBuf, Vec<u8>)> {
    let f = fs::File::open(path).with_context(|| format!("open apk: {}", path.display()))?;
    let mut zip = ZipArchive::new(f).context("parse apk zip")?;

    let preferred = ["lib/arm64-v8a/libapp.so", "base/lib/arm64-v8a/libapp.so"];

    for want in preferred {
        if let Ok(mut entry) = zip.by_name(want) {
            let mut out = Vec::new();
            entry
                .read_to_end(&mut out)
                .context("read libapp from apk")?;
            return Ok((PathBuf::from(want), out));
        }
    }

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with("/libapp.so") || name == "libapp.so" {
            let mut out = Vec::new();
            entry
                .read_to_end(&mut out)
                .context("read fallback libapp")?;
            return Ok((PathBuf::from(name), out));
        }
    }

    bail!("APK does not contain libapp.so");
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

pub fn load_snapshot_bundle(path: &Path) -> Result<SnapshotBundle> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "apk" {
        let (lib_path, lib_bytes) = find_libapp_in_apk(path)?;
        return from_elf(path, lib_path, lib_bytes);
    }

    let bytes = fs::read(path).with_context(|| format!("read input file: {}", path.display()))?;
    from_elf(path, path.to_path_buf(), bytes)
}
