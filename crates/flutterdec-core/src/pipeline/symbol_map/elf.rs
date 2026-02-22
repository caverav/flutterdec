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
        maybe_insert_symbol(
            &mut out,
            sym.st_value,
            sym.st_type(),
            elf.strtab.get_at(sym.st_name),
        );
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

fn maybe_insert_symbol(out: &mut BTreeMap<u64, String>, va: u64, sym_type: u8, name: Option<&str>) {
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
