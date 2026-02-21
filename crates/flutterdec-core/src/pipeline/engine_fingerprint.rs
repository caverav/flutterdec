use goblin::elf::header::{EM_386, EM_ARM, EM_MIPS, EM_PPC64, EM_RISCV, EM_X86_64};

#[derive(Debug, Clone)]
pub struct EngineFingerprintOptions {
    pub out_dir: Option<PathBuf>,
    pub max_markers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineFingerprintReport {
    pub input_path: String,
    pub file_size: u64,
    pub elf_class: String,
    pub machine: String,
    pub machine_id: u16,
    pub build_id: Option<String>,
    pub soname: Option<String>,
    pub needed_libraries: Vec<String>,
    pub symbol_count: usize,
    pub dyn_symbol_count: usize,
    pub exec_section_count: usize,
    pub exec_section_total_size: u64,
    pub flutter_markers: Vec<String>,
    pub dart_markers: Vec<String>,
    pub candidate_flutter_version: Option<String>,
    pub candidate_dart_version: Option<String>,
    pub confidence: String,
    pub report_path: Option<String>,
}

pub fn run_engine_fingerprint(
    input_path: &Path,
    opt: &EngineFingerprintOptions,
) -> Result<EngineFingerprintReport> {
    let bytes = fs::read(input_path)
        .with_context(|| format!("read shared object {}", input_path.display()))?;
    let elf = goblin::elf::Elf::parse(&bytes)
        .with_context(|| format!("parse ELF metadata for {}", input_path.display()))?;

    let build_id = extract_build_id(&elf, &bytes);
    let soname = elf.soname.map(str::to_string);
    let needed_libraries = elf.libraries.iter().map(|s| (*s).to_string()).collect();
    let exec_sections = collect_exec_sections(&elf, &bytes);
    let exec_section_total_size = exec_sections.iter().map(|s| s.size as u64).sum();

    let (flutter_markers, dart_markers, candidate_flutter_version, candidate_dart_version) =
        extract_engine_markers(&bytes, opt.max_markers.max(1));

    let confidence = confidence_level(
        build_id.is_some(),
        candidate_flutter_version.is_some() || candidate_dart_version.is_some(),
    );

    let mut report = EngineFingerprintReport {
        input_path: input_path.display().to_string(),
        file_size: bytes.len() as u64,
        elf_class: if elf.is_64 { "ELF64" } else { "ELF32" }.to_string(),
        machine: machine_name(elf.header.e_machine).to_string(),
        machine_id: elf.header.e_machine,
        build_id,
        soname,
        needed_libraries,
        symbol_count: elf.syms.len(),
        dyn_symbol_count: elf.dynsyms.len(),
        exec_section_count: exec_sections.len(),
        exec_section_total_size,
        flutter_markers,
        dart_markers,
        candidate_flutter_version,
        candidate_dart_version,
        confidence: confidence.to_string(),
        report_path: None,
    };

    if let Some(out_dir) = &opt.out_dir {
        fs::create_dir_all(out_dir)
            .with_context(|| format!("create fingerprint output dir {}", out_dir.display()))?;
        let report_path = out_dir.join("engine_fingerprint.json");
        fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
            .with_context(|| format!("write {}", report_path.display()))?;
        report.report_path = Some(report_path.display().to_string());
        fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
            .with_context(|| format!("write {}", report_path.display()))?;
    }

    Ok(report)
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        goblin::elf::header::EM_AARCH64 => "AArch64",
        EM_X86_64 => "x86_64",
        EM_ARM => "ARM",
        EM_386 => "x86",
        EM_RISCV => "RISC-V",
        EM_MIPS => "MIPS",
        EM_PPC64 => "PowerPC64",
        _ => "Unknown",
    }
}

fn extract_build_id(elf: &goblin::elf::Elf, bytes: &[u8]) -> Option<String> {
    for sh in &elf.section_headers {
        let name = elf.shdr_strtab.get_at(sh.sh_name).unwrap_or("");
        if !name.contains("note") {
            continue;
        }

        let Ok(off) = usize::try_from(sh.sh_offset) else {
            continue;
        };
        let Ok(size) = usize::try_from(sh.sh_size) else {
            continue;
        };
        if off.checked_add(size).is_none_or(|end| end > bytes.len()) {
            continue;
        }

        if let Some(id) = parse_build_id_note(&bytes[off..off + size]) {
            return Some(id);
        }
    }
    None
}

fn parse_build_id_note(note_blob: &[u8]) -> Option<String> {
    let mut i = 0usize;
    while i + 12 <= note_blob.len() {
        let namesz = u32::from_le_bytes(note_blob[i..i + 4].try_into().ok()?) as usize;
        let descsz = u32::from_le_bytes(note_blob[i + 4..i + 8].try_into().ok()?) as usize;
        let ntype = u32::from_le_bytes(note_blob[i + 8..i + 12].try_into().ok()?);
        i += 12;

        if i + namesz > note_blob.len() {
            return None;
        }
        let name = &note_blob[i..i + namesz];
        i = align4(i + namesz);

        if i + descsz > note_blob.len() {
            return None;
        }
        let desc = &note_blob[i..i + descsz];
        i = align4(i + descsz);

        let is_gnu = name.starts_with(b"GNU");
        if is_gnu && ntype == 3 && !desc.is_empty() {
            return Some(hex_encode(desc));
        }
    }

    None
}

fn align4(v: usize) -> usize {
    (v + 3) & !3
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

fn extract_engine_markers(
    bytes: &[u8],
    max_markers: usize,
) -> (Vec<String>, Vec<String>, Option<String>, Option<String>) {
    let mut flutter_markers = Vec::new();
    let mut dart_markers = Vec::new();
    let mut flutter_version = None;
    let mut dart_version = None;

    let mut uniq = std::collections::BTreeSet::new();

    for s in ascii_strings(bytes, 10) {
        let t = s.trim();
        if t.len() > 240 {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if (lower.contains("flutter") || lower.contains("engine")) && uniq.insert(format!("f:{t}"))
        {
            if flutter_markers.len() < max_markers {
                flutter_markers.push(t.to_string());
            }
            if flutter_version.is_none() {
                flutter_version = extract_semver_token(t);
            }
        }
        if (lower.contains("dart")
            || lower.contains("isolate snapshot")
            || lower.contains("vm snapshot"))
            && uniq.insert(format!("d:{t}"))
        {
            if dart_markers.len() < max_markers {
                dart_markers.push(t.to_string());
            }
            if dart_version.is_none() {
                dart_version = extract_semver_token(t);
            }
        }
        if flutter_markers.len() >= max_markers && dart_markers.len() >= max_markers {
            break;
        }
    }

    (flutter_markers, dart_markers, flutter_version, dart_version)
}

fn ascii_strings(bytes: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();

    for &b in bytes {
        if (0x20..=0x7e).contains(&b) {
            cur.push(b);
            continue;
        }
        if cur.len() >= min_len {
            if let Ok(s) = String::from_utf8(cur.clone()) {
                out.push(s);
            }
        }
        cur.clear();
    }

    if cur.len() >= min_len {
        if let Ok(s) = String::from_utf8(cur) {
            out.push(s);
        }
    }

    out
}

fn extract_semver_token(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'.' {
            continue;
        }
        i += 1;
        let mut mid_digits = 0usize;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            mid_digits += 1;
        }
        if mid_digits == 0 || i >= bytes.len() || bytes[i] != b'.' {
            continue;
        }
        i += 1;
        let mut end_digits = 0usize;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            end_digits += 1;
        }
        if end_digits == 0 {
            continue;
        }
        return Some(text[start..i].to_string());
    }

    None
}

fn confidence_level(has_build_id: bool, has_version_hint: bool) -> &'static str {
    match (has_build_id, has_version_hint) {
        (true, true) => "high",
        (true, false) | (false, true) => "medium",
        (false, false) => "low",
    }
}

#[cfg(test)]
mod engine_fingerprint_tests {
    use super::*;

    #[test]
    fn parses_semver_token() {
        assert_eq!(
            extract_semver_token("Flutter Engine 3.24.1 (stable)"),
            Some("3.24.1".to_string())
        );
        assert_eq!(extract_semver_token("Dart VM version: 3.5.0"), Some("3.5.0".to_string()));
        assert_eq!(extract_semver_token("no version here"), None);
    }

    #[test]
    fn parses_gnu_build_id_note_blob() {
        let name = b"GNU\0";
        let desc = [0x12u8, 0x34, 0xab, 0xcd];
        let mut blob = Vec::new();
        blob.extend_from_slice(&(name.len() as u32).to_le_bytes());
        blob.extend_from_slice(&(desc.len() as u32).to_le_bytes());
        blob.extend_from_slice(&3u32.to_le_bytes());
        blob.extend_from_slice(name);
        while blob.len() % 4 != 0 {
            blob.push(0);
        }
        blob.extend_from_slice(&desc);
        while blob.len() % 4 != 0 {
            blob.push(0);
        }

        assert_eq!(parse_build_id_note(&blob), Some("1234abcd".to_string()));
    }
}
