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
