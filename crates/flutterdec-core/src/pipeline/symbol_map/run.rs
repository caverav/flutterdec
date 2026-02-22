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
    target_summaries
        .sort_by(|a, b| b.call_count.cmp(&a.call_count).then(a.target_va.cmp(&b.target_va)));

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

pub fn load_symbol_target_symbols(
    path: &Path,
    include_nearest: bool,
) -> Result<BTreeMap<u64, String>> {
    let bytes =
        fs::read(path).with_context(|| format!("read symbol target map {}", path.display()))?;
    let entries: Vec<SymbolTargetSummary> = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse symbol target map JSON {}", path.display()))?;

    let mut out = BTreeMap::new();
    for e in entries {
        if e.symbol_name.is_none() {
            continue;
        }
        let kind = e.match_kind.to_ascii_lowercase();
        if kind == "exact" || (include_nearest && kind == "nearest") {
            out.insert(e.target_va, e.symbol_name.unwrap_or_default());
        }
    }
    Ok(out)
}
