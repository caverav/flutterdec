/// Order the identifier renames before they are applied as sequential textual
/// substitutions. The total secondary key protects output from HashMap seed order.
pub(crate) fn sort_rename_pairs(pairs: &mut [(String, String)]) {
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
}

/// Order the minus-one alias candidates before they are turned into named locals.
///
/// Most frequent first, so the alias budget goes to the identifiers that appear most,
/// then lexicographic to make the order total. The second key is load-bearing for the
/// same reason as `sort_rename_pairs`, and worse here: the candidates arrive from a
/// `HashMap` whose iteration order is seeded per process, and `sort_unstable_by` does not
/// even preserve that order for equal keys. With frequency as the only key, two idents
/// sharing a count were emitted in an arbitrary order, so `reg8Minus1` and `reg9Minus1`
/// swapped declarations between runs while every counter stayed identical.
pub(crate) fn sort_alias_candidates(candidates: &mut [(String, usize)]) {
    candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
}

impl<'a> FuncEmitter<'a> {
    fn alias_repeated_stack_slots(&mut self) {
        if self.lines.len() < 3 {
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in &self.lines {
                    for slot in Self::stack_slot_refs(crate::code_before_annotation(line)) {
                        *counts.entry(slot).or_insert(0) += 1;
                    }
                }

        let mut candidates: Vec<String> = counts
            .into_iter()
            .filter_map(|(slot, count)| if count >= 3 { Some(slot) } else { None })
            .collect();
        candidates.sort();
        if candidates.is_empty() {
            return;
        }

        let insert_idx = Self::prelude_insert_index(&self.lines);
        let mut inserts = Vec::new();
        for slot in candidates {
            if Self::stack_slot_is_written(&self.lines, &slot) {
                continue;
            }
            let needle = format!("sp[{slot}]");
            if !self.lines.iter().any(|l| l.contains(&needle)) {
                continue;
            }

            let base = Self::stack_slot_alias_base(&slot);
            let mut alias = base.clone();
            let mut n = 2usize;
            while Self::name_taken(&self.lines, &alias)
                || inserts
                    .iter()
                    .any(|l: &String| l.contains(&format!(" {alias} = ")))
            {
                alias = format!("{base}{n}");
                n += 1;
            }

            let mut replaced = false;
            for line in &mut self.lines {
                if line.contains(&needle) {
                    *line = line.replace(&needle, &alias);
                    replaced = true;
                }
            }
            if replaced {
                inserts.push(format!("  final {alias} = sp[{slot}];"));
            }
        }

        if !inserts.is_empty() {
            self.lines.splice(insert_idx..insert_idx, inserts);
        }
    }

    fn alias_repeated_pool_literals(&mut self) {
        if self.lines.len() < 4 {
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in &self.lines {
                    for lit in Self::pool_mapped_literals(crate::code_before_annotation(line)) {
                        *counts.entry(lit).or_insert(0) += 1;
                    }
                }

        let mut candidates: Vec<String> = counts
            .into_iter()
            .filter_map(|(lit, count)| if count >= 3 { Some(lit) } else { None })
            .collect();
        candidates.sort();
        if candidates.is_empty() {
            return;
        }

        let insert_idx = Self::prelude_insert_index(&self.lines);
        let mut inserts = Vec::new();
        for literal in candidates {
            if !self.lines.iter().any(|l| l.contains(&literal)) {
                continue;
            }
            let base = Self::pool_literal_alias_base(&literal);
            let mut alias = base.clone();
            let mut n = 2usize;
            while Self::name_taken(&self.lines, &alias)
                || inserts
                    .iter()
                    .any(|l: &String| l.contains(&format!(" {alias} = ")))
            {
                alias = format!("{base}{n}");
                n += 1;
            }

            let mut replaced = false;
            for line in &mut self.lines {
                if line.contains(&literal) {
                    *line = line.replace(&literal, &alias);
                    replaced = true;
                }
            }

            if replaced {
                inserts.push(format!("  final String {alias} = {literal};"));
            }
        }

        if !inserts.is_empty() {
            self.lines.splice(insert_idx..insert_idx, inserts);
        }
    }

    fn stack_slot_refs(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < line.len() {
            let Some(rel) = line[i..].find("sp[") else {
                break;
            };
            let start = i + rel + 3;
            let Some(end_rel) = line[start..].find(']') else {
                break;
            };
            let end = start + end_rel;
            let token = line[start..end].trim();
            if Self::is_simple_stack_slot_token(token) {
                out.push(token.to_string());
            }
            i = end + 1;
        }
        out
    }

    fn stack_slot_is_written(lines: &[String], slot: &str) -> bool {
        let spaced = format!("sp[{slot}] =");
        let compact = format!("sp[{slot}]=");
        let plus = format!("sp[{slot}] +=");
        let minus = format!("sp[{slot}] -=");
        let mul = format!("sp[{slot}] *=");
        let div = format!("sp[{slot}] /=");
        lines.iter().any(|line| {
                    let line = crate::code_before_annotation(line);
                    line.contains(&spaced)
                        || line.contains(&compact)
                        || line.contains(&plus)
                        || line.contains(&minus)
                        || line.contains(&mul)
                        || line.contains(&div)
                })
    }

    fn is_simple_stack_slot_token(token: &str) -> bool {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return false;
        }
        let rest = trimmed.strip_prefix('-').unwrap_or(trimmed);
        if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
            !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
        } else {
            rest.chars().all(|c| c.is_ascii_digit())
        }
    }

    fn stack_slot_alias_base(slot: &str) -> String {
        let token = slot.trim();
        if let Some(rest) = token.strip_prefix('-') {
            format!("stackSlotNeg{}", rest.to_ascii_lowercase())
        } else {
            format!("stackSlot{}", token.to_ascii_lowercase())
        }
    }

    fn pool_mapped_literals(line: &str) -> Vec<String> {
        let bytes = line.as_bytes();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] != b'"' {
                i += 1;
                continue;
            }

            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'\\' && j + 1 < bytes.len() {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' {
                    break;
                }
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }

            let mut k = j + 1;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k + 1 >= bytes.len() || bytes[k] != b'/' || bytes[k + 1] != b'*' {
                i = j + 1;
                continue;
            }
            k += 2;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k + 5 > bytes.len() || &bytes[k..k + 5] != b"pool[" {
                i = j + 1;
                continue;
            }
            k += 5;
            let digit_start = k;
            while k < bytes.len() && bytes[k].is_ascii_digit() {
                k += 1;
            }
            if k == digit_start || k >= bytes.len() || bytes[k] != b']' {
                i = j + 1;
                continue;
            }
            k += 1;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k + 1 >= bytes.len() || bytes[k] != b'*' || bytes[k + 1] != b'/' {
                i = j + 1;
                continue;
            }
            let end = k + 2;
            out.push(line[i..end].to_string());
            i = end;
        }
        out
    }

    fn pool_literal_alias_base(literal: &str) -> String {
        let Some(start) = literal.find("/* pool[") else {
            return "poolStr".to_string();
        };
        let rest = &literal[start + "/* pool[".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            "poolStr".to_string()
        } else {
            format!("poolStr{}", digits)
        }
    }

    pub(super) fn extract_minus_one_aliases(&mut self) {
        if self.lines.len() < 3 {
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in &self.lines {
                    for ident in Self::minus_one_idents(crate::code_before_annotation(line)) {
                        *counts.entry(ident).or_insert(0) += 1;
                    }
                }

        let mut candidates: Vec<(String, usize)> = counts
            .into_iter()
            .filter(|(_, count)| *count >= 4)
            .collect();
        sort_alias_candidates(&mut candidates);
        if candidates.is_empty() {
            return;
        }

        let insert_idx = Self::prelude_insert_index(&self.lines);
        let mut inserts = Vec::new();
        for (ident, _) in candidates {
            if Self::identifier_assigned(&self.lines, &ident) {
                continue;
            }
            let pattern = format!("({ident} - 1)");
            if !self.lines.iter().any(|l| l.contains(&pattern)) {
                continue;
            }

            let base = if ident.starts_with("value") {
                "codePoint".to_string()
            } else {
                format!("{ident}Minus1")
            };
            let mut alias = base.clone();
            let mut n = 2usize;
            while Self::name_taken(&self.lines, &alias)
                || inserts.iter().any(|l: &String| l.contains(&alias))
            {
                alias = format!("{base}{n}");
                n += 1;
            }

            let mut replaced = false;
            for line in &mut self.lines {
                if line.contains(&pattern) {
                    *line = line.replace(&pattern, &alias);
                    replaced = true;
                }
            }
            if replaced {
                inserts.push(format!("  final int {alias} = ({ident} - 1);"));
            }
        }

        if !inserts.is_empty() {
            self.lines.splice(insert_idx..insert_idx, inserts);
        }
    }

    pub(super) fn apply_name_and_type_hints(&mut self, fn_name: &str) {
        if self.lines.is_empty() {
            return;
        }

        // One identifier per register the Dart convention passes an argument
        // in. This rewrites `lines[0]`, so a wider range here silently widens
        // every signature regardless of what the emitter wrote.
        let arg_ids: Vec<String> = (0..DART_ARGUMENT_REGISTERS.len())
            .map(|i| format!("arg{i}"))
            .collect();
        let local_ids: Vec<String> = self.locals.values().cloned().collect();
        // Seeded from the one definition of these, so a local can never be given a
        // name the emitter already renders as a global.
        let mut used: HashSet<String> = RESERVED_EMITTER_IDENTIFIERS
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        let mut renames: HashMap<String, String> = HashMap::new();
        let mut arg_types: HashMap<String, String> = HashMap::new();
        let mut local_types: HashMap<String, String> = HashMap::new();
        let mut typed_ids = arg_ids.clone();
        typed_ids.extend(local_ids.clone());
        let inferred_types = Self::infer_declared_types_from_context(&self.lines, &typed_ids);

        for arg in &arg_ids {
            let stats = Self::collect_ident_stats(&self.lines, arg);
            let idx = arg.trim_start_matches("arg").parse::<usize>().unwrap_or(0);
            // `slot{idx}` and nothing more. The previous spelling named a
            // parameter from usage counts - index 0 became `receiver`, one field
            // access became `obj{idx}`, two arithmetic ops became `value{idx}` -
            // which reads like a recovered source name while resting on evidence
            // that cannot support it. One field access does not make a receiver
            // an object, and no analysis here recovers a parameter's role.
            //
            // The index is the one earned fact: it is the position in
            // `DART_ARGUMENT_REGISTERS`. It must survive verbatim, because a
            // caller renders arguments from the same register file, so
            // renumbering would relabel which register a reader is looking at.
            //
            // `stats` is still consumed below for the *type* hint, where a usage
            // count is legitimate evidence: a guessed type is declared `dynamic`
            // and stays checkable, while a guessed name is indistinguishable
            // from a recovered one.
            let base = format!("slot{idx}");
            let name = Self::unique_name(&base, &mut used);
            if name != *arg {
                renames.insert(arg.clone(), name);
            }
            let ty = inferred_types
                .get(arg)
                .cloned()
                .unwrap_or_else(|| {
                    if stats.arith_ops >= 2 && stats.field_access == 0 {
                        "int".to_string()
                    } else {
                        "dynamic".to_string()
                    }
                });
            arg_types.insert(arg.clone(), ty.to_string());
        }

        let mut pool_i = 1usize;
        let mut tmp_i = 1usize;
        for local in &local_ids {
            let stats = Self::collect_ident_stats(&self.lines, local);
            // A name may describe where a value came from. It may not assert
            // what the value *is*.
            //
            // `poolVal` and `resultTmp` survive because each states an observed
            // fact about the assignment: this local was assigned from the object
            // pool, or from a call result. `objTmp` and `intTmp` did not - they
            // guessed a *type* from usage counts, where two field accesses made
            // something an "obj" and two arithmetic operations made it an "int",
            // and then rendered that guess as a name indistinguishable from a
            // recovered one. A type guess belongs in the declared type, which is
            // `dynamic` when unproven and stays checkable; it does not belong in
            // an identifier, which a reader cannot check.
            //
            // Both collapse into the `tmp` counter, which claims nothing.
            let base = if stats.pool_assign > 0 {
                let n = pool_i;
                pool_i += 1;
                format!("poolVal{n}")
            } else if stats.call_assign > 0 {
                let n = tmp_i;
                tmp_i += 1;
                format!("resultTmp{n}")
            } else {
                let n = tmp_i;
                tmp_i += 1;
                format!("tmp{n}")
            };
            let name = Self::unique_name(&base, &mut used);
            if name != *local {
                renames.insert(local.clone(), name);
            }
            let ty = inferred_types
                .get(local)
                .cloned()
                .unwrap_or_else(|| {
                    if stats.arith_ops >= 2 && stats.field_access == 0 {
                        "int".to_string()
                    } else {
                        "dynamic".to_string()
                    }
                });
            local_types.insert(local.clone(), ty.to_string());
        }

        let mut rename_pairs: Vec<(String, String)> = renames.into_iter().collect();
        sort_rename_pairs(&mut rename_pairs);
        for line in &mut self.lines {
            let mut cur = line.clone();
            for (from, to) in &rename_pairs {
                cur = Self::replace_identifier_token(&cur, from, to);
            }
            *line = cur;
        }
        // Kept for the annotation appenders. They insert after this pass, from
        // candidates captured before it, so they must replay these renames or emit
        // identifiers the body no longer has.
        self.identifier_renames = rename_pairs.clone();


        let args_sig = arg_ids
            .iter()
            .map(|arg| {
                let name = rename_pairs
                    .iter()
                    .find_map(|(from, to)| if from == arg { Some(to.clone()) } else { None })
                    .unwrap_or_else(|| arg.clone());
                let ty = arg_types
                    .get(arg)
                    .cloned()
                    .unwrap_or_else(|| "dynamic".to_string());
                format!("{ty} {name}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.lines[0] = format!("dynamic {}({}) {{", fn_name, args_sig);

        let mut local_type_by_name: HashMap<String, String> = HashMap::new();
        for local in &local_ids {
            let name = rename_pairs
                .iter()
                .find_map(|(from, to)| {
                    if from == local {
                        Some(to.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| local.clone());
            let ty = local_types
                .get(local)
                .cloned()
                .unwrap_or_else(|| "dynamic".to_string());
            local_type_by_name.insert(name, ty);
        }

        for line in &mut self.lines {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("var ") {
                if let Some(name) = rest.strip_suffix(';') {
                    if let Some(ty) = local_type_by_name.get(name.trim()) {
                        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
                        *line = format!("{}{} {};", " ".repeat(indent), ty, name.trim());
                    }
                }
            }
        }

        for line in &mut self.lines {
            let mut cur = line.clone();
            for n in 0..=30 {
                let from = format!("x{n}");
                let to = named_register_alias(n);
                cur = Self::replace_identifier_token(&cur, &from, &to);
            }
            *line = cur;
        }

        self.alias_repeated_stack_slots();
        self.alias_repeated_pool_literals();
    }
}
