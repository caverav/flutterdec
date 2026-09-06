impl<'a> FuncEmitter<'a> {
    pub(super) fn replace_identifier_token(line: &str, from: &str, to: &str) -> String {
        if from.is_empty() || from == to {
            return line.to_string();
        }

        let bytes = line.as_bytes();
        let from_bytes = from.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + to.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if i + from_bytes.len() <= bytes.len() && bytes[i..].starts_with(from_bytes) {
                let prev_ok = if i == 0 {
                    true
                } else {
                    !Self::is_ident_char(bytes[i - 1] as char)
                };
                let next_i = i + from_bytes.len();
                let next_ok = if next_i >= bytes.len() {
                    true
                } else {
                    !Self::is_ident_char(bytes[next_i] as char)
                };
                if prev_ok && next_ok {
                    out.extend_from_slice(to.as_bytes());
                    i += from_bytes.len();
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).unwrap_or_else(|_| line.to_string())
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
                    let code = crate::code_before_annotation(line);
                    let t = code.trim();
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
                    // The whole call temporary, `tN`, and nothing appended to it.
                    //
                    // Two rejections, both drawn from real output. A bare `t` prefix
                    // matched `{id} = thread.f104` and `{id} = true`. Requiring a digit
                    // still matched `{id} = t8.f12`, which is a field *of* a call
                    // result, not a call result - and that was the dominant case, 456
                    // files, often beside a genuine `{id} = t8` naming a different
                    // local. So `resultTmpN` asserted a source it had not observed,
                    // which is the defect the naming rule exists to remove, sitting
                    // inside one of the two names that rule kept.
                    //
                    // `pool_assign` above never had this shape: it requires a literal
                    // `pool[`. Same digits-only discipline as `is_opaque_temporary`.
                    if t.strip_prefix(&call_assign).is_some_and(|rest| {
                        let digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                        digits.len() < rest.len() && (digits.is_empty() || digits.starts_with(';'))
                    }) {
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
        if !t.ends_with(';') || t.contains('=') {
            return false;
        }
        if t.contains('(') || t.contains(')') {
            return false;
        }
        let decl = t.trim_end_matches(';').trim();
        let mut parts = decl.split_whitespace();
        let Some(ty) = parts.next() else {
            return false;
        };
        let Some(name) = parts.next() else {
            return false;
        };
        if parts.next().is_some() {
            return false;
        }
        if ty == "return" || ty == "if" || ty == "else" || ty == "while" || ty == "for" {
            return false;
        }
        name.chars().all(Self::is_ident_char)
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
        lines
                    .iter()
                    .any(|line| crate::code_before_annotation(line).contains(name))
    }

    pub(super) fn identifier_assigned(lines: &[String], ident: &str) -> bool {
        lines
                    .iter()
                    .any(|line| Self::assigns_ident(crate::code_before_annotation(line), ident))
    }

    pub(super) fn infer_declared_types_from_context(
        lines: &[String],
        ids: &[String],
    ) -> HashMap<String, String> {
        let mut out: HashMap<String, String> = HashMap::new();

        for line in lines {
                    let line = crate::code_before_annotation(line);
                    if let Some(condition) = Self::extract_if_condition(line) {
                        for id in ids {
                            if Self::condition_suggests_bool(&condition, id) {
                                Self::upsert_inferred_type(&mut out, id, "bool");
                            }
                        }
                    }
        
                    if let Some((callee, args)) = Self::extract_call_site(line) {
                        if let Some(assign_id) = Self::extract_assignment_ident(line) {
                            if ids.contains(&assign_id) {
                                if let Some(local_ty) = Self::constructed_type_from_semantic_path(&callee) {
                                    Self::upsert_inferred_type(&mut out, &assign_id, &local_ty);
                                } else if let Some(local_ty) =
                                    Self::return_type_from_semantic_path(&callee)
                                {
                                    Self::upsert_inferred_type(&mut out, &assign_id, &local_ty);
                                } else if let Some(path) = Self::extract_semantic_path_from_comment(line) {
                                    if let Some(local_ty) = Self::constructed_type_from_semantic_path(&path)
                                    {
                                        Self::upsert_inferred_type(&mut out, &assign_id, &local_ty);
                                    } else if let Some(local_ty) =
                                        Self::return_type_from_semantic_path(&path)
                                    {
                                        Self::upsert_inferred_type(&mut out, &assign_id, &local_ty);
                                    }
                                }
                            }
                        }
        
                        if let Some(receiver_ty) = Self::receiver_type_from_semantic_path(&callee) {
                            if let Some(receiver_id) = Self::receiver_ident_from_args(&args) {
                                if ids.contains(&receiver_id) {
                                    Self::upsert_inferred_type(&mut out, &receiver_id, &receiver_ty);
                                }
                            }
                        }
                    }
        
                    if let Some(path) = Self::extract_semantic_path_from_comment(line) {
                        if let Some(receiver_ty) = Self::receiver_type_from_semantic_path(&path) {
                            if let Some((_, args)) = Self::extract_call_site(line) {
                                if let Some(receiver_id) = Self::receiver_ident_from_args(&args) {
                                    if ids.contains(&receiver_id) {
                                        Self::upsert_inferred_type(&mut out, &receiver_id, &receiver_ty);
                                    }
                                }
                            }
                        }
                    }
        
                    for id in ids {
                        let Some(rhs) = Self::extract_assignment_rhs(line, id) else {
                            continue;
                        };
                        let rhs_trim = rhs.trim();
                        if rhs_trim.starts_with('"') && rhs_trim.ends_with('"') && rhs_trim.len() >= 2 {
                            Self::upsert_inferred_type(&mut out, id, "String");
                        } else if let Some((literal_prefix, _)) = rhs_trim.split_once("/* pool[") {
                            let literal_prefix = literal_prefix.trim();
                            if literal_prefix.starts_with('"')
                                && literal_prefix.ends_with('"')
                                && literal_prefix.len() >= 2
                            {
                                Self::upsert_inferred_type(&mut out, id, "String");
                            }
                        } else if rhs_trim == "true" || rhs_trim == "false" {
                            Self::upsert_inferred_type(&mut out, id, "bool");
                        } else if Self::is_integer_literal(rhs_trim) {
                            Self::upsert_inferred_type(&mut out, id, "int");
                        }
                    }
                }

        out
    }

    fn extract_if_condition(line: &str) -> Option<String> {
        let t = line.trim();
        if !t.starts_with("if ") && !t.starts_with("if(") {
            return None;
        }
        let open = t.find('(')?;
        let close = Self::match_paren(t, open)?;
        let cond = t[open + 1..close].trim();
        if cond.is_empty() {
            None
        } else {
            Some(cond.to_string())
        }
    }

    fn condition_suggests_bool(cond: &str, id: &str) -> bool {
        let compact: String = cond.chars().filter(|c| !c.is_whitespace()).collect();
        let trimmed = compact.trim_matches(|c| c == '(' || c == ')');
        if trimmed == id || trimmed == format!("!{id}") {
            return true;
        }

        for pos in Self::identifier_positions(trimmed, id) {
            let before = &trimmed[..pos];
            let after = &trimmed[pos + id.len()..];
            if after.starts_with("==true")
                || after.starts_with("==false")
                || after.starts_with("!=true")
                || after.starts_with("!=false")
            {
                return true;
            }
            if before.ends_with("true==")
                || before.ends_with("false==")
                || before.ends_with("true!=")
                || before.ends_with("false!=")
            {
                return true;
            }
            if after.starts_with("&&")
                || after.starts_with("||")
                || before.ends_with("&&")
                || before.ends_with("||")
            {
                return true;
            }
            if before.ends_with('!') && (after.is_empty() || after.starts_with("&&") || after.starts_with("||")) {
                return true;
            }
        }
        false
    }

    fn identifier_positions(s: &str, id: &str) -> Vec<usize> {
        let mut out = Vec::new();
        for (idx, _) in s.match_indices(id) {
            let prev_ok = idx == 0
                || s[..idx]
                    .chars()
                    .last()
                    .map(|c| !Self::is_ident_char(c))
                    .unwrap_or(true);
            let next_i = idx + id.len();
            let next_ok = next_i >= s.len()
                || s[next_i..]
                    .chars()
                    .next()
                    .map(|c| !Self::is_ident_char(c))
                    .unwrap_or(true);
            if prev_ok && next_ok {
                out.push(idx);
            }
        }
        out
    }

    fn upsert_inferred_type(out: &mut HashMap<String, String>, id: &str, ty: &str) {
        let next = ty.to_string();
        match out.get(id) {
            Some(cur) if cur == &next => {}
            Some(cur) if cur == "dynamic" => {
                out.insert(id.to_string(), next);
            }
            None => {
                out.insert(id.to_string(), next);
            }
            _ => {}
        }
    }

    fn extract_assignment_rhs(line: &str, ident: &str) -> Option<String> {
        let t = line.trim();
        if !Self::assigns_ident(t, ident) {
            return None;
        }
        let needle = format!("{ident} =");
        let pos = t.find(&needle)?;
        let rhs = t[pos + needle.len()..].trim();
        let rhs = rhs.strip_suffix(';').unwrap_or(rhs).trim();
        Some(rhs.to_string())
    }

    fn extract_semantic_path_from_comment(line: &str) -> Option<String> {
        let (_, comment) = line.split_once("//")?;
        let comment = comment.trim();
        for prefix in ["framework:", "stdlib:", "runtime:", "package:"] {
            if let Some(rest) = comment.strip_prefix(prefix) {
                let token = rest
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
        None
    }

    fn extract_assignment_ident(line: &str) -> Option<String> {
        let t = line.trim();
        let eq_idx = t.find('=')?;
        let lhs = t[..eq_idx].trim();
        let lhs = lhs
            .strip_prefix("final ")
            .or_else(|| lhs.strip_prefix("var "))
            .unwrap_or(lhs)
            .trim();
        let ident = lhs.split_whitespace().last()?.trim();
        if ident.is_empty()
            || !ident.chars().all(Self::is_ident_char)
            || !ident
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            return None;
        }
        Some(ident.to_string())
    }

    fn receiver_type_from_semantic_path(path: &str) -> Option<String> {
        let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
        if parts.len() < 4 {
            return None;
        }
        if parts.last().is_some_and(|m| m.eq_ignore_ascii_case("new")) {
            return None;
        }

        if !(path.starts_with("flutter.") || path.starts_with("dart.")) {
            let first = parts.first().copied().unwrap_or_default();
            let first_is_package_like = !first.is_empty()
                && first
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !first_is_package_like {
                return None;
            }
            let owner = parts.get(parts.len().saturating_sub(2)).copied().unwrap_or("");
            if owner.is_empty() || !owner.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                return None;
            }
        }

        Some(parts[..parts.len() - 1].join("."))
    }

    fn constructed_type_from_semantic_path(path: &str) -> Option<String> {
        let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }
        if !parts.last().is_some_and(|m| m.eq_ignore_ascii_case("new")) {
            return None;
        }

        let first = parts.first().copied().unwrap_or_default();
        let first_is_supported_root = first == "flutter"
            || first == "dart"
            || first == "dart_vm"
            || first
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            || first
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !first_is_supported_root {
            return None;
        }

        Some(parts[..parts.len() - 1].join("."))
    }

    fn return_type_from_semantic_path(path: &str) -> Option<String> {
        let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }
        let method = parts.last().copied().unwrap_or("").to_ascii_lowercase();
        if method.is_empty() || method == "new" {
            return None;
        }

        if path.starts_with("dart.async.Future.") {
            if method == "asstream" {
                return Some("dart.async.Stream".to_string());
            }
            return Some("dart.async.Future".to_string());
        }
        if path.starts_with("dart.async.Stream.") && method == "listen" {
            return Some("dart.async.StreamSubscription".to_string());
        }

        if matches!(
            method.as_str(),
            "startswith"
                | "endswith"
                | "contains"
                | "containskey"
                | "supportsansiescapes"
                | "isempty"
                | "isnotempty"
        ) {
            return Some("bool".to_string());
        }

        if matches!(
            method.as_str(),
            "hashcode"
                | "compareto"
                | "indexof"
                | "lastindexof"
                | "codeunitat"
                | "offsetinbytes"
                | "lengthinbytes"
                | "elementsizeinbytes"
                | "getint8"
                | "getuint8"
                | "getint16"
                | "getuint16"
                | "getint32"
                | "getuint32"
                | "getint64"
                | "getuint64"
        ) {
            return Some("int".to_string());
        }

        if matches!(method.as_str(), "getfloat32" | "getfloat64") {
            return Some("double".to_string());
        }

        if matches!(
            method.as_str(),
            "tostring"
                | "substring"
                | "replaceall"
                | "tolowercase"
                | "touppercase"
                | "trim"
                | "trimleft"
                | "trimright"
                | "join"
        ) {
            return Some("String".to_string());
        }

        if method == "runtimetype" {
            return Some("Type".to_string());
        }

        None
    }

    fn extract_call_site(line: &str) -> Option<(String, Vec<String>)> {
        let t = line.trim();
        let rhs = if let Some((_, r)) = t.split_once('=') {
            r.trim()
        } else {
            let rest = t.strip_prefix("return ")?;
            rest.trim()
        };

        let open = rhs.find('(')?;
        let callee = rhs[..open].trim().to_string();
        if callee.is_empty() || callee.contains(char::is_whitespace) {
            return None;
        }

        let close = Self::match_paren(rhs, open)?;
        let args_src = &rhs[open + 1..close];
        let args = Self::split_call_args(args_src);
        Some((callee, args))
    }

    fn match_paren(s: &str, open: usize) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut depth = 0i32;
        let mut i = open;
        let mut in_string = false;
        while i < bytes.len() {
            let b = bytes[i];
            if in_string {
                if b == b'\\' {
                    i = i.saturating_add(2);
                    continue;
                }
                if b == b'"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            match b {
                b'"' => in_string = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn split_call_args(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut depth_paren = 0i32;
        let mut depth_bracket = 0i32;
        let mut depth_brace = 0i32;
        let mut in_string = false;

        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if in_string {
                cur.push(b as char);
                if b == b'\\' && i + 1 < bytes.len() {
                    cur.push(bytes[i + 1] as char);
                    i += 2;
                    continue;
                }
                if b == b'"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }

            match b {
                b'"' => {
                    in_string = true;
                    cur.push('"');
                }
                b'(' => {
                    depth_paren += 1;
                    cur.push('(');
                }
                b')' => {
                    depth_paren -= 1;
                    cur.push(')');
                }
                b'[' => {
                    depth_bracket += 1;
                    cur.push('[');
                }
                b']' => {
                    depth_bracket -= 1;
                    cur.push(']');
                }
                b'{' => {
                    depth_brace += 1;
                    cur.push('{');
                }
                b'}' => {
                    depth_brace -= 1;
                    cur.push('}');
                }
                b','
                    if depth_paren == 0
                        && depth_bracket == 0
                        && depth_brace == 0
                        && !in_string =>
                {
                    let piece = cur.trim();
                    if !piece.is_empty() {
                        out.push(piece.to_string());
                    }
                    cur.clear();
                }
                _ => cur.push(b as char),
            }
            i += 1;
        }

        let piece = cur.trim();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        out
    }

    fn extract_ident_expr(expr: &str) -> Option<String> {
        let mut t = expr.trim();
        while let Some(inner) = t.strip_prefix('(').and_then(|v| v.strip_suffix(')')) {
            t = inner.trim();
        }
        if t.is_empty() {
            return None;
        }
        if t.chars().all(Self::is_ident_char)
            && t.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            return Some(t.to_string());
        }
        None
    }

    fn receiver_ident_from_args(args: &[String]) -> Option<String> {
        if let Some(first) = args.first().and_then(|a| Self::extract_ident_expr(a)) {
            return Some(first);
        }

        if args.len() >= 2 {
            if let Some(class_id_ident) = Self::extract_class_id_ident(&args[0]) {
                if let Some(second) = Self::extract_ident_expr(&args[1]) {
                    if second == class_id_ident {
                        return Some(second);
                    }
                }
                return Some(class_id_ident);
            }
        }

        None
    }

    fn extract_class_id_ident(expr: &str) -> Option<String> {
        let t = expr.trim();
        let inner = t.strip_prefix("classId(")?.strip_suffix(')')?.trim();
        Self::extract_ident_expr(inner)
    }

    fn is_integer_literal(s: &str) -> bool {
        let t = s.trim();
        if t.is_empty() {
            return false;
        }
        if let Some(hex) = t.strip_prefix("0x") {
            return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
        }
        let digits = t.strip_prefix('-').unwrap_or(t);
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    }

}

#[cfg(test)]
mod naming_support_utf8_tests {
    use super::*;

    #[test]
    fn replace_identifier_token_handles_utf8_text() {
        let line = r#"final title = "Možete"; final x = local + 1;"#;
        let out = FuncEmitter::replace_identifier_token(line, "local", "renamed");
        assert!(out.contains(r#""Možete""#));
        assert!(out.contains("renamed + 1"));
    }
}
