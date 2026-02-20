impl<'a> FuncEmitter<'a> {
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
            let ty = if stats.arith_ops >= 2 && stats.field_access == 0 {
                "int"
            } else {
                "dynamic"
            };
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
            let ty = if stats.arith_ops >= 2 && stats.field_access == 0 {
                "int"
            } else {
                "dynamic"
            };
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
    }
}
