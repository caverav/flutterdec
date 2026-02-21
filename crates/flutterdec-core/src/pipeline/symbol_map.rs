use capstone::arch::arm64::ArchMode;
use capstone::prelude::*;
use goblin::elf::header::EM_AARCH64;
use goblin::elf::section_header::SHF_EXECINSTR;
use goblin::elf::sym::{STT_FUNC, STT_NOTYPE};
use goblin::elf::Elf;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct SymbolMapOptions {
    pub out_dir: PathBuf,
    pub include_branches: bool,
    pub nearest_max_distance: u64,
    pub require_exec_match: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolTargetSummary {
    pub target_va: u64,
    pub call_count: usize,
    pub match_kind: String,
    pub symbol_name: Option<String>,
    pub symbol_va: Option<u64>,
    pub symbol_offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolMapReport {
    pub stripped_path: String,
    pub unstripped_path: String,
    pub arch: String,
    pub unstripped_symbol_count: usize,
    pub scanned_exec_section_count: usize,
    pub total_direct_calls: usize,
    pub unique_call_targets: usize,
    pub exact_symbol_hits: usize,
    pub nearest_symbol_hits: usize,
    pub unresolved_calls: usize,
    pub exec_layout_match: bool,
    pub exec_bytes_match: bool,
    pub notes: Vec<String>,
    pub report_path: String,
    pub targets_path: String,
    pub callsites_path: String,
}

#[derive(Debug, Clone)]
struct ExecSection {
    name: String,
    addr: u64,
    offset: usize,
    size: usize,
}

#[derive(Debug, Clone)]
struct CallSite {
    section: String,
    call_va: u64,
    mnemonic: String,
    target_va: Option<u64>,
    match_kind: MatchKind,
    symbol_name: Option<String>,
    symbol_va: Option<u64>,
    symbol_offset: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
enum MatchKind {
    Exact,
    Nearest,
    Unresolved,
}

impl MatchKind {
    fn as_str(self) -> &'static str {
        match self {
            MatchKind::Exact => "exact",
            MatchKind::Nearest => "nearest",
            MatchKind::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedTarget {
    kind: MatchKind,
    symbol_name: Option<String>,
    symbol_va: Option<u64>,
    symbol_offset: Option<i64>,
}

pub fn run_symbol_map(
    stripped_path: &Path,
    unstripped_path: &Path,
    opt: &SymbolMapOptions,
) -> Result<SymbolMapReport> {
    let stripped_bytes = fs::read(stripped_path)
        .with_context(|| format!("read stripped binary {}", stripped_path.display()))?;
    let unstripped_bytes = fs::read(unstripped_path)
        .with_context(|| format!("read unstripped binary {}", unstripped_path.display()))?;

    let stripped_elf =
        Elf::parse(&stripped_bytes).with_context(|| "parse stripped ELF metadata")?;
    let unstripped_elf =
        Elf::parse(&unstripped_bytes).with_context(|| "parse unstripped ELF metadata")?;

    if stripped_elf.header.e_machine != EM_AARCH64 || unstripped_elf.header.e_machine != EM_AARCH64
    {
        bail!("map-symbols currently supports only ARM64 ELF binaries");
    }

    let stripped_exec = collect_exec_sections(&stripped_elf, &stripped_bytes);
    let unstripped_exec = collect_exec_sections(&unstripped_elf, &unstripped_bytes);

    let mut notes = Vec::new();
    let (exec_layout_match, exec_bytes_match) = compare_exec_layouts(
        &stripped_exec,
        &stripped_bytes,
        &unstripped_exec,
        &unstripped_bytes,
        &mut notes,
    );

    if opt.require_exec_match && !exec_bytes_match {
        bail!(
            "executable section bytes differ between stripped and unstripped binaries; rerun without --require-exec-match if intentional"
        );
    }

    let symbols = collect_symbols(&unstripped_elf);
    if symbols.is_empty() {
        bail!("no useful function symbols found in unstripped binary");
    }

    let callsites_raw = scan_direct_calls(&stripped_exec, &stripped_bytes, opt.include_branches)?;

    let mut callsites = Vec::with_capacity(callsites_raw.len());
    for c in callsites_raw {
        let resolved = resolve_target(&symbols, c.target_va, opt.nearest_max_distance);
        callsites.push(CallSite {
            section: c.section,
            call_va: c.call_va,
            mnemonic: c.mnemonic,
            target_va: c.target_va,
            match_kind: resolved.kind,
            symbol_name: resolved.symbol_name,
            symbol_va: resolved.symbol_va,
            symbol_offset: resolved.symbol_offset,
        });
    }

    let mut exact_hits = 0usize;
    let mut nearest_hits = 0usize;
    let mut unresolved_hits = 0usize;
    for c in &callsites {
        match c.match_kind {
            MatchKind::Exact => exact_hits += 1,
            MatchKind::Nearest => nearest_hits += 1,
            MatchKind::Unresolved => unresolved_hits += 1,
        }
    }

    let mut agg: std::collections::HashMap<u64, SymbolTargetSummary> =
        std::collections::HashMap::new();
    for c in &callsites {
        let Some(target_va) = c.target_va else {
            continue;
        };
        let entry = agg.entry(target_va).or_insert_with(|| SymbolTargetSummary {
            target_va,
            call_count: 0,
            match_kind: c.match_kind.as_str().to_string(),
            symbol_name: c.symbol_name.clone(),
            symbol_va: c.symbol_va,
            symbol_offset: c.symbol_offset,
        });
        entry.call_count += 1;
    }

    let mut target_summaries: Vec<SymbolTargetSummary> = agg.into_values().collect();
    target_summaries.sort_by(|a, b| b.call_count.cmp(&a.call_count).then(a.target_va.cmp(&b.target_va)));

    fs::create_dir_all(&opt.out_dir)
        .with_context(|| format!("create output directory {}", opt.out_dir.display()))?;

    let callsites_path = opt.out_dir.join("symbol_call_sites.tsv");
    let targets_path = opt.out_dir.join("symbol_target_summary.json");
    let report_path = opt.out_dir.join("symbol_map_report.json");

    write_callsites_tsv(&callsites_path, &callsites)?;
    fs::write(&targets_path, serde_json::to_vec_pretty(&target_summaries)?)
        .with_context(|| format!("write {}", targets_path.display()))?;

    let report = SymbolMapReport {
        stripped_path: stripped_path.display().to_string(),
        unstripped_path: unstripped_path.display().to_string(),
        arch: "arm64".to_string(),
        unstripped_symbol_count: symbols.len(),
        scanned_exec_section_count: stripped_exec.len(),
        total_direct_calls: callsites.len(),
        unique_call_targets: target_summaries.len(),
        exact_symbol_hits: exact_hits,
        nearest_symbol_hits: nearest_hits,
        unresolved_calls: unresolved_hits,
        exec_layout_match,
        exec_bytes_match,
        notes,
        report_path: report_path.display().to_string(),
        targets_path: targets_path.display().to_string(),
        callsites_path: callsites_path.display().to_string(),
    };

    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(report)
}

pub fn load_elf_function_symbols(path: &Path) -> Result<BTreeMap<u64, String>> {
    let bytes = fs::read(path).with_context(|| format!("read ELF {}", path.display()))?;
    let elf = Elf::parse(&bytes).with_context(|| format!("parse ELF {}", path.display()))?;
    Ok(collect_symbols(&elf))
}

fn collect_exec_sections(elf: &Elf, bytes: &[u8]) -> Vec<ExecSection> {
    let mut out = Vec::new();

    for sh in &elf.section_headers {
        if sh.sh_size == 0 {
            continue;
        }
        if (sh.sh_flags & SHF_EXECINSTR as u64) == 0 {
            continue;
        }

        let Ok(offset) = usize::try_from(sh.sh_offset) else {
            continue;
        };
        let Ok(size) = usize::try_from(sh.sh_size) else {
            continue;
        };

        if offset.checked_add(size).is_none_or(|end| end > bytes.len()) {
            continue;
        }

        let name = elf.shdr_strtab.get_at(sh.sh_name).unwrap_or("<exec>").to_string();
        out.push(ExecSection {
            name,
            addr: sh.sh_addr,
            offset,
            size,
        });
    }

    out.sort_by_key(|s| s.addr);
    out
}

fn compare_exec_layouts(
    stripped_sections: &[ExecSection],
    stripped_bytes: &[u8],
    unstripped_sections: &[ExecSection],
    unstripped_bytes: &[u8],
    notes: &mut Vec<String>,
) -> (bool, bool) {
    let mut un_map: BTreeMap<(u64, usize), &ExecSection> = BTreeMap::new();
    for s in unstripped_sections {
        un_map.insert((s.addr, s.size), s);
    }

    let mut layout_match = stripped_sections.len() == unstripped_sections.len();
    let mut bytes_match = layout_match;

    for s in stripped_sections {
        let key = (s.addr, s.size);
        let Some(u) = un_map.get(&key) else {
            layout_match = false;
            bytes_match = false;
            notes.push(format!(
                "missing exec section match for addr=0x{:x} size=0x{:x} ({})",
                s.addr, s.size, s.name
            ));
            continue;
        };

        let s_bytes = &stripped_bytes[s.offset..s.offset + s.size];
        let u_bytes = &unstripped_bytes[u.offset..u.offset + u.size];
        if s_bytes != u_bytes {
            bytes_match = false;
            notes.push(format!(
                "exec bytes differ at addr=0x{:x} size=0x{:x} ({})",
                s.addr, s.size, s.name
            ));
        }
    }

    if !layout_match {
        notes.push("executable section layout differs between binaries".to_string());
    }

    (layout_match, bytes_match)
}

fn collect_symbols(elf: &Elf) -> BTreeMap<u64, String> {
    let mut out = BTreeMap::new();

    for sym in &elf.syms {
        maybe_insert_symbol(&mut out, sym.st_value, sym.st_type(), elf.strtab.get_at(sym.st_name));
    }
    for sym in &elf.dynsyms {
        maybe_insert_symbol(
            &mut out,
            sym.st_value,
            sym.st_type(),
            elf.dynstrtab.get_at(sym.st_name),
        );
    }

    out
}

fn maybe_insert_symbol(
    out: &mut BTreeMap<u64, String>,
    va: u64,
    sym_type: u8,
    name: Option<&str>,
) {
    if va == 0 {
        return;
    }
    if sym_type != STT_FUNC && sym_type != STT_NOTYPE {
        return;
    }

    let Some(name) = name.map(str::trim) else {
        return;
    };
    if !is_useful_symbol_name(name) {
        return;
    }

    let replace = match out.get(&va) {
        None => true,
        Some(existing) => name.len() > existing.len(),
    };
    if replace {
        out.insert(va, name.to_string());
    }
}

fn is_useful_symbol_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with('$') || name.starts_with(".L") {
        return false;
    }
    true
}

fn scan_direct_calls(
    sections: &[ExecSection],
    bytes: &[u8],
    include_branches: bool,
) -> Result<Vec<CallSite>> {
    let cs = Capstone::new()
        .arm64()
        .mode(ArchMode::Arm)
        .detail(false)
        .build()
        .context("build capstone")?;

    let mut out = Vec::new();

    for s in sections {
        let code = &bytes[s.offset..s.offset + s.size];
        let insns = match cs.disasm_all(code, s.addr) {
            Ok(v) => v,
            Err(_) => continue,
        };

        for ins in insns.iter() {
            let mnemonic = ins.mnemonic().unwrap_or("").to_ascii_lowercase();
            let is_call = mnemonic == "bl";
            let is_direct_branch = include_branches && mnemonic == "b";
            if !is_call && !is_direct_branch {
                continue;
            }

            let op_str = ins.op_str().unwrap_or("");
            let target_va = parse_target_va(op_str);

            out.push(CallSite {
                section: s.name.clone(),
                call_va: ins.address(),
                mnemonic,
                target_va,
                match_kind: MatchKind::Unresolved,
                symbol_name: None,
                symbol_va: None,
                symbol_offset: None,
            });
        }
    }

    Ok(out)
}

fn parse_target_va(op_str: &str) -> Option<u64> {
    for token in op_str.split(|c: char| c.is_whitespace() || c == ',' || c == '[' || c == ']') {
        let t = token.trim().trim_start_matches('#');
        if t.is_empty() {
            continue;
        }

        if let Some(hex) = t.strip_prefix("0x") {
            if let Ok(v) = u64::from_str_radix(hex, 16) {
                return Some(v);
            }
            continue;
        }

        if let Ok(v) = t.parse::<u64>() {
            return Some(v);
        }
    }

    None
}

fn resolve_target(
    symbols: &BTreeMap<u64, String>,
    target_va: Option<u64>,
    nearest_max_distance: u64,
) -> ResolvedTarget {
    let Some(target_va) = target_va else {
        return ResolvedTarget {
            kind: MatchKind::Unresolved,
            symbol_name: None,
            symbol_va: None,
            symbol_offset: None,
        };
    };

    if let Some(name) = symbols.get(&target_va) {
        return ResolvedTarget {
            kind: MatchKind::Exact,
            symbol_name: Some(name.clone()),
            symbol_va: Some(target_va),
            symbol_offset: Some(0),
        };
    }

    if let Some((sym_va, sym_name)) = symbols.range(..=target_va).next_back() {
        let delta = target_va - *sym_va;
        if delta <= nearest_max_distance {
            return ResolvedTarget {
                kind: MatchKind::Nearest,
                symbol_name: Some(sym_name.clone()),
                symbol_va: Some(*sym_va),
                symbol_offset: Some(delta as i64),
            };
        }
    }

    ResolvedTarget {
        kind: MatchKind::Unresolved,
        symbol_name: None,
        symbol_va: None,
        symbol_offset: None,
    }
}

fn write_callsites_tsv(path: &Path, callsites: &[CallSite]) -> Result<()> {
    let mut out = String::new();
    out.push_str("call_va\tsection\tmnemonic\ttarget_va\tmatch\tsymbol\tsymbol_va\tsymbol_offset\n");

    for c in callsites {
        let target = c
            .target_va
            .map(|v| format!("0x{v:x}"))
            .unwrap_or_else(|| "-".to_string());
        let symbol = c.symbol_name.as_deref().unwrap_or("-");
        let symbol_va = c
            .symbol_va
            .map(|v| format!("0x{v:x}"))
            .unwrap_or_else(|| "-".to_string());
        let symbol_off = c
            .symbol_offset
            .map(|v| format!("{v}"))
            .unwrap_or_else(|| "-".to_string());

        out.push_str(&format!(
            "0x{:x}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            c.call_va,
            c.section,
            c.mnemonic,
            target,
            c.match_kind.as_str(),
            symbol,
            symbol_va,
            symbol_off
        ));
    }

    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_target_from_hex_and_decimal_operands() {
        assert_eq!(parse_target_va("#0x4010"), Some(0x4010));
        assert_eq!(parse_target_va("x0, #0x2008"), Some(0x2008));
        assert_eq!(parse_target_va("1234"), Some(1234));
        assert_eq!(parse_target_va("x17"), None);
    }

    #[test]
    fn resolves_exact_before_nearest() {
        let mut syms = BTreeMap::new();
        syms.insert(0x1000, "foo".to_string());
        syms.insert(0x1100, "bar".to_string());

        let exact = resolve_target(&syms, Some(0x1100), 0x200);
        assert!(matches!(exact.kind, MatchKind::Exact));
        assert_eq!(exact.symbol_name.as_deref(), Some("bar"));

        let near = resolve_target(&syms, Some(0x1120), 0x40);
        assert!(matches!(near.kind, MatchKind::Nearest));
        assert_eq!(near.symbol_name.as_deref(), Some("bar"));
        assert_eq!(near.symbol_offset, Some(0x20));

        let far = resolve_target(&syms, Some(0x2000), 0x10);
        assert!(matches!(far.kind, MatchKind::Unresolved));
    }
}
