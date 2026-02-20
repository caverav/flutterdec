impl<'a> FuncEmitter<'a> {
    pub(super) fn replace_identifier_token(line: &str, from: &str, to: &str) -> String {
        if from.is_empty() || from == to {
            return line.to_string();
        }

        let mut out = String::with_capacity(line.len());
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < line.len() {
            if line[i..].starts_with(from) {
                let prev_ok = if i == 0 {
                    true
                } else {
                    !Self::is_ident_char(bytes[i - 1] as char)
                };
                let next_i = i + from.len();
                let next_ok = if next_i >= line.len() {
                    true
                } else {
                    !Self::is_ident_char(bytes[next_i] as char)
                };
                if prev_ok && next_ok {
                    out.push_str(to);
                    i += from.len();
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    pub(super) fn collect_ident_stats(lines: &[String], id: &str) -> IdentStats {
        let mut s = IdentStats::default();
        let field_pat = format!("{id}.");
        let null_eq_1 = format!("{id} == null");
        let null_eq_2 = format!("null == {id}");
        let null_ne_1 = format!("{id} != null");
        let null_ne_2 = format!("null != {id}");
        let call_assign = format!("{id} = t");

        for line in lines {
            let t = line.trim();
            s.field_access += t.matches(&field_pat).count();
            s.arith_ops += t.matches(&format!("{id} +")).count();
            s.arith_ops += t.matches(&format!("{id} -")).count();
            s.arith_ops += t.matches(&format!("{id} <<")).count();
            s.arith_ops += t.matches(&format!("{id} >>")).count();
            s.arith_ops += t.matches(&format!("{id} &")).count();
            s.arith_ops += t.matches(&format!("{id} |")).count();
            s.arith_ops += t.matches(&format!("{id} ^")).count();
            s.null_cmp += t.matches(&null_eq_1).count();
            s.null_cmp += t.matches(&null_eq_2).count();
            s.null_cmp += t.matches(&null_ne_1).count();
            s.null_cmp += t.matches(&null_ne_2).count();

            if t.starts_with(&format!("{id} = pool["))
                || t.contains(&format!("{id} = (pool["))
                || t.contains(&format!("{id} = ((pool["))
            {
                s.pool_assign += 1;
            }
            if t.starts_with(&call_assign) {
                s.call_assign += 1;
            }
        }
        s
    }

    pub(super) fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
        if !used.contains(base) {
            used.insert(base.to_string());
            return base.to_string();
        }
        let mut i = 2usize;
        loop {
            let candidate = format!("{base}{i}");
            if !used.contains(&candidate) {
                used.insert(candidate.clone());
                return candidate;
            }
            i += 1;
        }
    }

    pub(super) fn is_local_decl_line(t: &str) -> bool {
        if !(t.starts_with("int ") || t.starts_with("dynamic ")) {
            return false;
        }
        if !t.ends_with(';') || t.contains('=') {
            return false;
        }
        !t.contains('(')
    }

    pub(super) fn prelude_insert_index(lines: &[String]) -> usize {
        let mut idx = 1usize;
        while idx < lines.len() {
            let t = lines[idx].trim();
            if t.is_empty() || t.starts_with("//") || Self::is_local_decl_line(t) {
                idx += 1;
                continue;
            }
            break;
        }
        idx
    }

    pub(super) fn minus_one_idents(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut start = 0usize;
        while let Some(rel) = line[start..].find(" - 1)") {
            let idx = start + rel;
            let prefix = &line[..idx];
            if let Some(lp) = prefix.rfind('(') {
                let ident = prefix[lp + 1..].trim();
                if !ident.is_empty()
                    && ident.chars().all(Self::is_ident_char)
                    && ident
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                {
                    out.push(ident.to_string());
                }
            }
            start = idx + " - 1)".len();
        }
        out
    }

    pub(super) fn name_taken(lines: &[String], name: &str) -> bool {
        lines.iter().any(|l| l.contains(name))
    }

    pub(super) fn identifier_assigned(lines: &[String], ident: &str) -> bool {
        lines.iter().any(|l| Self::assigns_ident(l, ident))
    }

}
