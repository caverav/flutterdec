impl<'a> FuncEmitter<'a> {
    fn alias_dispatch_target_slot_calls(&mut self) {
        if self.lines.is_empty() {
            return;
        }

        let mut replaced = false;
        for line in &mut self.lines {
            if !line.contains("indirect via: dispatchTarget") {
                continue;
            }
            if line.contains("reg21.f0.invoke(") {
                *line = line.replace("reg21.f0.invoke(", "dispatchTargetFn.invoke(");
                replaced = true;
            }
            if line.contains("reg21.f0(") {
                *line = line.replace("reg21.f0(", "dispatchTargetFn(");
                replaced = true;
            }
        }

        if !replaced {
            return;
        }

        let alias_decl = "  final dispatchTargetFn = reg21.f0;".to_string();
        if self.lines.iter().any(|l| l.trim() == alias_decl.trim()) {
            return;
        }
        let idx = Self::prelude_insert_index(&self.lines);
        self.lines.insert(idx, alias_decl);
    }

    fn alias_repeated_stack_slots(&mut self) {
        if self.lines.len() < 3 {
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in &self.lines {
            for slot in Self::stack_slot_refs(line) {
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

    pub(super) fn extract_minus_one_aliases(&mut self) {
        if self.lines.len() < 3 {
            return;
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for line in &self.lines {
            for ident in Self::minus_one_idents(line) {
                *counts.entry(ident).or_insert(0) += 1;
            }
        }

        let mut candidates: Vec<(String, usize)> = counts
            .into_iter()
            .filter(|(_, count)| *count >= 4)
            .collect();
        candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1));
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

        let arg_ids: Vec<String> = (0..8).map(|i| format!("arg{i}")).collect();
        let local_ids: Vec<String> = self.locals.values().cloned().collect();
        let mut used = HashSet::new();
        used.insert("thread".to_string());
        used.insert("pool".to_string());
        used.insert("sp".to_string());
        used.insert("null".to_string());
        used.insert("flags".to_string());
        used.insert("dynamic".to_string());

        let mut renames: HashMap<String, String> = HashMap::new();
        let mut arg_types: HashMap<String, String> = HashMap::new();
        let mut local_types: HashMap<String, String> = HashMap::new();
        let mut typed_ids = arg_ids.clone();
        typed_ids.extend(local_ids.clone());
        let inferred_types = Self::infer_declared_types_from_context(&self.lines, &typed_ids);

        for arg in &arg_ids {
            let stats = Self::collect_ident_stats(&self.lines, arg);
            let idx = arg.trim_start_matches("arg").parse::<usize>().unwrap_or(0);
            let base = if idx == 0 {
                "receiver".to_string()
            } else if stats.field_access >= 1 {
                format!("obj{idx}")
            } else if stats.arith_ops >= 2 && stats.field_access == 0 {
                format!("value{idx}")
            } else {
                format!("param{idx}")
            };
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
        let mut obj_i = 1usize;
        let mut int_i = 1usize;
        let mut tmp_i = 1usize;
        for local in &local_ids {
            let stats = Self::collect_ident_stats(&self.lines, local);
            let base = if stats.pool_assign > 0 {
                let n = pool_i;
                pool_i += 1;
                format!("poolVal{n}")
            } else if stats.field_access >= 2 {
                let n = obj_i;
                obj_i += 1;
                format!("objTmp{n}")
            } else if stats.arith_ops >= 2 && stats.field_access == 0 {
                let n = int_i;
                int_i += 1;
                format!("intTmp{n}")
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
        rename_pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for line in &mut self.lines {
            let mut cur = line.clone();
            for (from, to) in &rename_pairs {
                cur = Self::replace_identifier_token(&cur, from, to);
            }
            *line = cur;
        }

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

        self.alias_dispatch_target_slot_calls();
        self.alias_repeated_stack_slots();
    }
}
