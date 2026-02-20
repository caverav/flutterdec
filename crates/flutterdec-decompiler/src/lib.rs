use flutterdec_ir::{BasicBlock, FunctionIr, IROp};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct PseudocodeArtifact {
    pub function_id: u64,
    pub function_name: String,
    pub source: String,
    pub placeholder_ifs: usize,
    pub unresolved_cf: usize,
    pub raw_register_calls: usize,
    pub total_calls: usize,
    pub indirect_calls: usize,
}

#[derive(Debug, Default, Clone)]
struct LiftState {
    reg_values: HashMap<String, String>,
    last_cmp: Option<(String, String)>,
    call_index: usize,
}

struct FuncEmitter<'a> {
    ir: &'a FunctionIr,
    symbol_names: &'a HashMap<u64, String>,
    locals: BTreeMap<i64, String>,
    block_by_id: HashMap<usize, &'a BasicBlock>,
    va_to_id: HashMap<u64, usize>,

    emitted: HashSet<usize>,
    active_stack: Vec<usize>,
    inline_visits: HashMap<usize, usize>,
    omitted_blocks: BTreeSet<usize>,
    loop_back_edges: BTreeSet<usize>,
    loop_context: Vec<usize>,
    lines: Vec<String>,

    state: LiftState,
    placeholder_ifs: usize,
    unresolved_cf: usize,
    raw_register_calls: usize,
    total_calls: usize,
    indirect_calls: usize,
}

#[derive(Debug, Clone)]
struct HelperMeta {
    id: usize,
    start: usize,
    end: usize,
    body_lines: Vec<String>,
    return_expr: Option<String>,
}

#[derive(Debug, Clone)]
struct InlineHelperPlan {
    lines: Vec<String>,
    append_null_return: bool,
}

#[derive(Debug, Default, Clone)]
struct IdentStats {
    field_access: usize,
    arith_ops: usize,
    pool_assign: usize,
    null_cmp: usize,
    call_assign: usize,
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "function".to_string()
    } else {
        out
    }
}

fn canonical_reg(token: &str) -> Option<String> {
    let t = token.trim().trim_end_matches('!').to_ascii_lowercase();
    if t.len() < 2 {
        return None;
    }
    let mut chars = t.chars();
    let first = chars.next()?;
    if first != 'x' && first != 'w' {
        return None;
    }
    let rest = chars.as_str();
    if rest.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("x{rest}"));
    }
    None
}

fn is_zero_reg(token: &str) -> bool {
    matches!(
        token
            .trim()
            .trim_end_matches('!')
            .to_ascii_lowercase()
            .as_str(),
        "xzr" | "wzr"
    )
}

fn parse_int(token: &str) -> Option<i64> {
    let t = token.trim().trim_start_matches('#');
    if let Some(hex) = t.strip_prefix("-0x") {
        return i64::from_str_radix(hex, 16).ok().map(|v| -v);
    }
    if let Some(hex) = t.strip_prefix("0x") {
        return i64::from_str_radix(hex, 16).ok();
    }
    t.parse::<i64>().ok()
}

fn parse_expr_int(expr: &str) -> Option<i64> {
    let t = expr.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        return parse_expr_int(inner);
    }
    parse_int(t)
}

fn fmt_int(v: i64) -> String {
    if v < 0 {
        let mag = -v;
        if mag >= 10 {
            format!("-0x{mag:x}")
        } else {
            format!("-{mag}")
        }
    } else if v >= 10 {
        format!("0x{v:x}")
    } else {
        v.to_string()
    }
}

fn parse_base_offset_expr(expr: &str) -> Option<(String, i64)> {
    let mut t = expr.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        t = inner.trim();
    }

    let bytes = t.as_bytes();
    let mut depth = 0i32;
    let mut op_idx = None;
    let mut op_ch = '+';
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        let c = bytes[i] as char;
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
        } else if depth == 0
            && bytes[i] == b' '
            && (bytes[i + 1] == b'+' || bytes[i + 1] == b'-')
            && bytes[i + 2] == b' '
        {
            op_idx = Some(i);
            op_ch = bytes[i + 1] as char;
            break;
        }
        i += 1;
    }

    let idx = op_idx?;
    let lhs = t[..idx].trim();
    let rhs = t[idx + 3..].trim();
    if lhs.is_empty() {
        return None;
    }
    let off = parse_expr_int(rhs)?;
    let signed_off = if op_ch == '-' { -off } else { off };
    Some((lhs.to_string(), signed_off))
}

fn simplify_bin_expr(lhs: String, op: &str, rhs: String) -> String {
    let lt = lhs.trim();
    let rt = rhs.trim();
    let l_int = parse_expr_int(lt);
    let r_int = parse_expr_int(rt);

    match op {
        "+" => {
            if lt == "null" && r_int.is_some() {
                return fmt_int(r_int.unwrap());
            }
            if rt == "null" && l_int.is_some() {
                return fmt_int(l_int.unwrap());
            }
            if l_int == Some(0) {
                return rt.to_string();
            }
            if r_int == Some(0) {
                return lt.to_string();
            }
            if let (Some(a), Some(b)) = (l_int, r_int) {
                return fmt_int(a + b);
            }
            if let (Some((base, off)), Some(delta)) = (parse_base_offset_expr(lt), r_int) {
                let sum = off + delta;
                if sum == 0 {
                    return base;
                }
                if sum > 0 {
                    return format!("({base} + {})", fmt_int(sum));
                }
                return format!("({base} - {})", fmt_int(-sum));
            }
        }
        "-" => {
            if r_int == Some(0) {
                return lt.to_string();
            }
            if let (Some(a), Some(b)) = (l_int, r_int) {
                return fmt_int(a - b);
            }
            if let (Some((base, off)), Some(delta)) = (parse_base_offset_expr(lt), r_int) {
                let sum = off - delta;
                if sum == 0 {
                    return base;
                }
                if sum > 0 {
                    return format!("({base} + {})", fmt_int(sum));
                }
                return format!("({base} - {})", fmt_int(-sum));
            }
        }
        _ => {}
    }

    format!("({lt} {op} {rt})")
}

fn split_operands(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;

    for c in s.chars() {
        match c {
            '[' => {
                depth += 1;
                cur.push(c);
            }
            ']' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                let piece = cur.trim();
                if !piece.is_empty() {
                    out.push(piece.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }

    let piece = cur.trim();
    if !piece.is_empty() {
        out.push(piece.to_string());
    }

    out
}

fn split_instruction(src: &str) -> (String, Vec<String>) {
    let mut it = src.trim().splitn(2, char::is_whitespace);
    let mnemonic = it.next().unwrap_or("").to_ascii_lowercase();
    let operands = it.next().unwrap_or("").trim();
    let ops = if operands.is_empty() {
        Vec::new()
    } else {
        split_operands(operands)
    };
    (mnemonic, ops)
}

fn parse_mem_operand(op: &str) -> Option<(String, i64)> {
    let s = op.trim();
    let lb = s.find('[')?;
    let rb = s[lb..].find(']')? + lb;
    let inside = &s[lb + 1..rb];
    let parts: Vec<&str> = inside.split(',').collect();
    if parts.is_empty() {
        return None;
    }
    let base = parts[0].trim().to_ascii_lowercase();
    let off = if parts.len() >= 2 {
        parse_int(parts[1]).unwrap_or(0)
    } else {
        0
    };
    Some((base, off))
}

fn normalize_target(target: &str) -> String {
    let mut last_hex = None;
    for token in target.split(|c: char| c.is_whitespace() || c == ',') {
        let t = token.trim().trim_start_matches('#');
        if let Some(hex) = t.strip_prefix("0x") {
            if let Ok(v) = u64::from_str_radix(hex, 16) {
                last_hex = Some(v);
            }
        }
    }
    if let Some(v) = last_hex {
        return format!("0x{v:x}");
    }
    for token in target.split(|c: char| c.is_whitespace() || c == ',') {
        let t = token.trim().trim_start_matches('#');
        if let Some(reg) = canonical_reg(t) {
            return reg;
        }
    }
    target.trim_start_matches('#').to_string()
}

fn named_indirect_target(token: &str) -> String {
    if let Some(reg) = canonical_reg(token) {
        if let Some(id) = reg.strip_prefix('x') {
            if let Ok(n) = id.parse::<usize>() {
                return match n {
                    30 => "dispatchTarget".to_string(),
                    2 => "cachedTarget".to_string(),
                    _ => format!("indirectTarget{n}"),
                };
            }
        }
    }
    token.to_string()
}

fn named_register_alias(n: usize) -> String {
    match n {
        30 => "returnAddress".to_string(),
        29 => "framePointer".to_string(),
        _ => format!("reg{n}"),
    }
}

fn local_name(off: i64) -> String {
    if off < 0 {
        format!("local_m{}", -off)
    } else if off > 0 {
        format!("local_p{}", off)
    } else {
        "local_0".to_string()
    }
}

fn collect_stack_offsets(ir: &FunctionIr) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();

    for block in &ir.blocks {
        for ins in &block.instrs {
            let (mnemonic, ops) = split_instruction(&ins.src);
            if (mnemonic == "ldur" || mnemonic == "ldr" || mnemonic == "stur" || mnemonic == "str")
                && ops.len() >= 2
            {
                if let Some((base, off)) = parse_mem_operand(&ops[1]) {
                    if base == "x29" {
                        out.insert(off);
                    }
                }
            }
        }
    }

    out
}

fn init_state() -> LiftState {
    let mut s = LiftState::default();
    for i in 0..8 {
        s.reg_values.insert(format!("x{i}"), format!("arg{i}"));
    }
    s.reg_values.insert("x15".to_string(), "sp".to_string());
    s.reg_values.insert("x22".to_string(), "null".to_string());
    s.reg_values.insert("x26".to_string(), "thread".to_string());
    s.reg_values.insert("x27".to_string(), "pool".to_string());
    s
}

fn cond_from_cmp(branch: &str, cmp: &(String, String)) -> Option<String> {
    let op = match branch {
        "b.eq" => "==",
        "b.ne" => "!=",
        "b.lt" => "<",
        "b.le" => "<=",
        "b.gt" => ">",
        "b.ge" => ">=",
        "b.hi" => ">",
        "b.ls" => "<=",
        "b.lo" => "<",
        "b.hs" => ">=",
        _ => return None,
    };
    Some(format!("{} {} {}", cmp.0, op, cmp.1))
}

impl<'a> FuncEmitter<'a> {
    fn new(ir: &'a FunctionIr, symbol_names: &'a HashMap<u64, String>) -> Self {
        let offsets = collect_stack_offsets(ir);
        let mut locals = BTreeMap::new();
        for off in offsets {
            locals.insert(off, local_name(off));
        }

        let mut block_by_id = HashMap::new();
        let mut va_to_id = HashMap::new();
        for b in &ir.blocks {
            block_by_id.insert(b.id, b);
            va_to_id.insert(b.start_va, b.id);
        }

        Self {
            ir,
            symbol_names,
            locals,
            block_by_id,
            va_to_id,
            emitted: HashSet::new(),
            active_stack: Vec::new(),
            inline_visits: HashMap::new(),
            omitted_blocks: BTreeSet::new(),
            loop_back_edges: BTreeSet::new(),
            loop_context: Vec::new(),
            lines: Vec::new(),
            state: init_state(),
            placeholder_ifs: 0,
            unresolved_cf: 0,
            raw_register_calls: 0,
            total_calls: 0,
            indirect_calls: 0,
        }
    }

    fn emit(mut self) -> PseudocodeArtifact {
        let fn_name = sanitize_name(&self.ir.name);

        self.lines.push(format!(
            "dynamic {}(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {{",
            fn_name
        ));
        for name in self.locals.values() {
            self.lines.push(format!("  var {};", name));
        }
        if !self.locals.is_empty() {
            self.lines.push(String::new());
        }

        let body_start = self.lines.len();
        if let Some(entry) = self.ir.blocks.first() {
            self.emit_block(entry.id, 1, 0);
        }

        let body_lines = self.lines.len().saturating_sub(body_start);
        if body_lines == 0 {
            for b in &self.ir.blocks {
                if self.emitted.contains(&b.id) {
                    continue;
                }
                self.emit_block(b.id, 1, 0);
                break;
            }
        }

        self.lines.push("}".to_string());
        if !self.omitted_blocks.is_empty() {
            self.lines.push(String::new());
            self.append_helper_functions();
            self.inline_trivial_helpers();
            self.collapse_remaining_helpers();
        }
        self.insert_loop_summary_comment();
        self.compact_lines();
        for line in &mut self.lines {
            *line = Self::clean_expr(line.clone());
        }
        self.apply_name_and_type_hints(&fn_name);

        PseudocodeArtifact {
            function_id: self.ir.function_id,
            function_name: fn_name,
            source: self.lines.join("\n"),
            placeholder_ifs: self.placeholder_ifs,
            unresolved_cf: self.unresolved_cf,
            raw_register_calls: self.raw_register_calls,
            total_calls: self.total_calls,
            indirect_calls: self.indirect_calls,
        }
    }

    fn push_line(&mut self, indent: usize, line: &str) {
        self.lines.push(format!("{}{}", "  ".repeat(indent), line));
    }

    fn emit_omitted_path(&mut self, indent: usize, block_id: Option<usize>) {
        if let Some(id) = block_id {
            self.omitted_blocks.insert(id);
            self.push_line(indent, &format!("return _block_{}();", id));
        } else {
            self.push_line(indent, "/* path omitted */");
        }
    }

    fn compact_lines(&mut self) {
        for _pass in 0..8 {
            let mut changed = false;
            let mut out = Vec::new();
            let mut i = 0usize;
            let mut retry_loop_id = 1usize;

            while i < self.lines.len() {
                let cur = &self.lines[i];
                let cur_trim = cur.trim();

                if let Some(var) = Self::retry_decl_var(cur_trim) {
                    if i + 1 < self.lines.len() {
                        let next_trim = self.lines[i + 1].trim();
                        if Self::while_var(next_trim).as_deref() == Some(var.as_str()) {
                            if let Some(loop_end) = Self::find_block_end(&self.lines, i + 1) {
                                let has_continue = (i + 2..loop_end)
                                    .any(|idx| self.lines[idx].trim() == "continue;");
                                if !has_continue {
                                    for idx in i + 2..loop_end {
                                        let t = self.lines[idx].trim();
                                        if t == format!("{var} = false;")
                                            || t == format!("{var} = true;")
                                        {
                                            continue;
                                        }
                                        out.push(Self::dedent_once(&self.lines[idx]));
                                    }
                                    i = loop_end + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }
                }

                if cur_trim == "while (true) {" {
                    if let Some(j) = Self::find_block_end(&self.lines, i) {
                        let mut rel_depth = 1i32;
                        let mut has_continue = false;
                        let mut continue_count = 0usize;
                        let mut last_non_empty = None;
                        let mut break_at_top_level = false;

                        for idx in i + 1..j {
                            let t = self.lines[idx].trim();
                            if !t.is_empty() {
                                last_non_empty = Some(idx);
                                if t == "continue;" {
                                    has_continue = true;
                                    continue_count += 1;
                                }
                            }
                            rel_depth +=
                                self.lines[idx].chars().filter(|&c| c == '{').count() as i32;
                            rel_depth -=
                                self.lines[idx].chars().filter(|&c| c == '}').count() as i32;
                        }

                        if let Some(last_idx) = last_non_empty {
                            break_at_top_level =
                                self.lines[last_idx].trim() == "break;" && rel_depth == 1;
                            if self.lines[last_idx].trim() == "break;" {
                                let mut depth_at_break = 1i32;
                                for idx in i + 1..last_idx {
                                    depth_at_break +=
                                        self.lines[idx].chars().filter(|&c| c == '{').count()
                                            as i32;
                                    depth_at_break -=
                                        self.lines[idx].chars().filter(|&c| c == '}').count()
                                            as i32;
                                }
                                break_at_top_level = depth_at_break == 1;
                            }
                        }

                        if break_at_top_level && !has_continue {
                            for idx in i + 1..j {
                                if Some(idx) == last_non_empty && self.lines[idx].trim() == "break;"
                                {
                                    continue;
                                }
                                out.push(Self::dedent_once(&self.lines[idx]));
                            }
                            i = j + 1;
                            changed = true;
                            continue;
                        }

                        if break_at_top_level && continue_count >= 2 {
                            let indent = Self::leading_indent(cur);
                            let retry_var = format!("retryLoop{retry_loop_id}");
                            retry_loop_id += 1;

                            out.push(format!("{}bool {} = true;", " ".repeat(indent), retry_var));
                            out.push(format!("{}while ({}) {{", " ".repeat(indent), retry_var));

                            for idx in i + 1..j {
                                if Some(idx) == last_non_empty && self.lines[idx].trim() == "break;"
                                {
                                    continue;
                                }
                                out.push(self.lines[idx].clone());
                            }
                            out.push(format!("{}{} = false;", " ".repeat(indent + 2), retry_var));

                            out.push(self.lines[j].clone());
                            i = j + 1;
                            changed = true;
                            continue;
                        }
                    }
                }

                if cur_trim.starts_with("if (") && cur_trim.ends_with(") {") {
                    let cond = Self::if_condition(cur_trim).unwrap_or("");
                    if !cond.contains("flags.") && !cond.contains("/* cond */") {
                        if let Some(first_end) = Self::find_block_end(&self.lines, i) {
                            let mut first_else = first_end + 1;
                            while first_else < self.lines.len()
                                && self.lines[first_else].trim().is_empty()
                            {
                                first_else += 1;
                            }
                            let first_has_else = first_else < self.lines.len()
                                && self.lines[first_else].trim() == "else {";
                            if !first_has_else {
                                if let Some(then_ret) =
                                    Self::single_top_level_return(&self.lines, i + 1, first_end)
                                {
                                    let indent =
                                        cur.chars().take_while(|c| c.is_whitespace()).count();
                                    let mut next = first_end + 1;
                                    while next < self.lines.len()
                                        && self.lines[next].trim().is_empty()
                                    {
                                        next += 1;
                                    }
                                    if next < self.lines.len() {
                                        let next_line = &self.lines[next];
                                        let next_trim = next_line.trim();
                                        if Self::leading_indent(next_line) == indent
                                            && next_trim.starts_with("if (")
                                            && next_trim.ends_with(") {")
                                        {
                                            if let Some(next_end) =
                                                Self::find_block_end(&self.lines, next)
                                            {
                                                let mut next_else = next_end + 1;
                                                while next_else < self.lines.len()
                                                    && self.lines[next_else].trim().is_empty()
                                                {
                                                    next_else += 1;
                                                }
                                                let next_has_else = next_else < self.lines.len()
                                                    && self.lines[next_else].trim() == "else {";
                                                if !next_has_else
                                                    && Self::single_top_level_stmt(
                                                        &self.lines,
                                                        next + 1,
                                                        next_end,
                                                    )
                                                    .as_deref()
                                                        == Some("continue;")
                                                {
                                                    if let Some((lhs1, op1, rhs1)) =
                                                        Self::parse_simple_cmp(cond)
                                                    {
                                                        if let Some((lhs2, op2, rhs2)) =
                                                            Self::parse_simple_cmp(
                                                                Self::if_condition(next_trim)
                                                                    .unwrap_or(""),
                                                            )
                                                        {
                                                            if lhs1 == lhs2
                                                                && op1 == ">"
                                                                && op2 == ">="
                                                            {
                                                                if let (Some(k), Some(l)) = (
                                                                    Self::parse_int_literal(&rhs1),
                                                                    Self::parse_int_literal(&rhs2),
                                                                ) {
                                                                    if l <= k {
                                                                        out.push(format!(
                                                                            "{}if (({} >= {}) && ({} <= {})) {{",
                                                                            " ".repeat(indent),
                                                                            lhs2,
                                                                            rhs2,
                                                                            lhs2,
                                                                            rhs1
                                                                        ));
                                                                        out.push(format!(
                                                                            "{}continue;",
                                                                            " ".repeat(indent + 2)
                                                                        ));
                                                                        out.push(format!(
                                                                            "{}}}",
                                                                            " ".repeat(indent)
                                                                        ));
                                                                        out.push(cur.clone());
                                                                        out.push(format!(
                                                                            "{}{}",
                                                                            " ".repeat(indent + 2),
                                                                            then_ret
                                                                        ));
                                                                        out.push(
                                                                            self.lines[first_end]
                                                                                .clone(),
                                                                        );
                                                                        i = next_end + 1;
                                                                        changed = true;
                                                                        continue;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(then_end) = Self::find_block_end(&self.lines, i) {
                            let mut then_else = then_end + 1;
                            while then_else < self.lines.len()
                                && self.lines[then_else].trim().is_empty()
                            {
                                then_else += 1;
                            }
                            let has_else = then_else < self.lines.len()
                                && self.lines[then_else].trim() == "else {";
                            if !has_else {
                                if let Some(then_ret) =
                                    Self::single_top_level_return(&self.lines, i + 1, then_end)
                                {
                                    let mut next = then_end + 1;
                                    while next < self.lines.len()
                                        && self.lines[next].trim().is_empty()
                                    {
                                        next += 1;
                                    }
                                    if next < self.lines.len()
                                        && self.lines[next].trim() == then_ret
                                    {
                                        let indent =
                                            cur.chars().take_while(|c| c.is_whitespace()).count();
                                        out.push(format!("{}{}", " ".repeat(indent), then_ret));
                                        i = next + 1;
                                        changed = true;
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(first_cond) = Self::if_condition(cur_trim) {
                        if let Some(first_end) = Self::find_block_end(&self.lines, i) {
                            let mut first_else = first_end + 1;
                            while first_else < self.lines.len()
                                && self.lines[first_else].trim().is_empty()
                            {
                                first_else += 1;
                            }
                            let first_has_else = first_else < self.lines.len()
                                && self.lines[first_else].trim() == "else {";
                            let mut conds = Vec::new();
                            if !first_has_else
                                && Self::single_top_level_stmt(&self.lines, i + 1, first_end)
                                    .as_deref()
                                    == Some("continue;")
                            {
                                conds.push(first_cond.to_string());
                                let indent = Self::leading_indent(cur);
                                let mut end = first_end;
                                loop {
                                    let mut next = end + 1;
                                    while next < self.lines.len()
                                        && self.lines[next].trim().is_empty()
                                    {
                                        next += 1;
                                    }
                                    if next >= self.lines.len() {
                                        break;
                                    }
                                    if Self::leading_indent(&self.lines[next]) != indent {
                                        break;
                                    }
                                    let next_trim = self.lines[next].trim();
                                    let Some(next_cond) = Self::if_condition(next_trim) else {
                                        break;
                                    };
                                    let Some(next_end) = Self::find_block_end(&self.lines, next)
                                    else {
                                        break;
                                    };
                                    let mut next_else = next_end + 1;
                                    while next_else < self.lines.len()
                                        && self.lines[next_else].trim().is_empty()
                                    {
                                        next_else += 1;
                                    }
                                    let next_has_else = next_else < self.lines.len()
                                        && self.lines[next_else].trim() == "else {";
                                    if next_has_else {
                                        break;
                                    }
                                    if Self::single_top_level_stmt(&self.lines, next + 1, next_end)
                                        .as_deref()
                                        != Some("continue;")
                                    {
                                        break;
                                    }
                                    conds.push(next_cond.to_string());
                                    end = next_end;
                                }

                                if conds.len() >= 2 {
                                    out.push(format!(
                                        "{}if ({}) {{",
                                        " ".repeat(indent),
                                        conds
                                            .iter()
                                            .map(|c| format!("({})", c))
                                            .collect::<Vec<_>>()
                                            .join(" || ")
                                    ));
                                    out.push(format!("{}continue;", " ".repeat(indent + 2)));
                                    out.push(format!("{}}}", " ".repeat(indent)));
                                    i = end + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(outer_cond) = Self::if_condition(cur_trim) {
                        if let Some(outer_end) = Self::find_block_end(&self.lines, i) {
                            let mut inner_start = None;
                            for idx in i + 1..outer_end {
                                if !self.lines[idx].trim().is_empty() {
                                    inner_start = Some(idx);
                                    break;
                                }
                            }

                            if let Some(inner_start) = inner_start {
                                let inner_trim = self.lines[inner_start].trim();
                                if Self::leading_indent(&self.lines[inner_start])
                                    == Self::leading_indent(cur) + 2
                                    && inner_trim.starts_with("if (")
                                    && inner_trim.ends_with(") {")
                                {
                                    if let Some(inner_end) =
                                        Self::find_block_end(&self.lines, inner_start)
                                    {
                                        if inner_end < outer_end {
                                            let mut only_inner = true;
                                            for idx in i + 1..outer_end {
                                                if idx >= inner_start && idx <= inner_end {
                                                    continue;
                                                }
                                                if !self.lines[idx].trim().is_empty() {
                                                    only_inner = false;
                                                    break;
                                                }
                                            }
                                            if only_inner {
                                                if let Some(inner_cond) =
                                                    Self::if_condition(inner_trim)
                                                {
                                                    let indent = Self::leading_indent(cur);
                                                    out.push(format!(
                                                        "{}if (({}) && ({})) {{",
                                                        " ".repeat(indent),
                                                        outer_cond,
                                                        inner_cond
                                                    ));
                                                    for idx in inner_start + 1..inner_end {
                                                        out.push(Self::dedent_once(
                                                            &self.lines[idx],
                                                        ));
                                                    }
                                                    out.push(self.lines[outer_end].clone());
                                                    i = outer_end + 1;
                                                    changed = true;
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let cur_indent = Self::leading_indent(cur);
                    if let Some(id) = Self::null_checked_ident(cur_trim) {
                        if let Some(first_end) = Self::find_block_end(&self.lines, i) {
                            let mut first_else = first_end + 1;
                            while first_else < self.lines.len()
                                && self.lines[first_else].trim().is_empty()
                            {
                                first_else += 1;
                            }
                            let first_has_else = first_else < self.lines.len()
                                && self.lines[first_else].trim() == "else {";
                            if !first_has_else
                                && Self::block_terminates_at_top_level(
                                    &self.lines,
                                    i + 1,
                                    first_end,
                                )
                            {
                                let mut rewritten = false;
                                let mut scan = first_end + 1;
                                while scan < self.lines.len() {
                                    let line = &self.lines[scan];
                                    let t = line.trim();
                                    if t.is_empty() {
                                        scan += 1;
                                        continue;
                                    }

                                    let indent = Self::leading_indent(line);
                                    if indent < cur_indent {
                                        break;
                                    }

                                    if Self::assigns_ident(line, &id) {
                                        break;
                                    }

                                    if indent == cur_indent
                                        && t.starts_with("if (")
                                        && t.ends_with(") {")
                                    {
                                        if Self::null_checked_ident(t).as_deref() == Some(&id) {
                                            if let Some(second_end) =
                                                Self::find_block_end(&self.lines, scan)
                                            {
                                                for idx in i..scan {
                                                    out.push(self.lines[idx].clone());
                                                }

                                                let mut second_else = second_end + 1;
                                                while second_else < self.lines.len()
                                                    && self.lines[second_else].trim().is_empty()
                                                {
                                                    second_else += 1;
                                                }
                                                if second_else < self.lines.len()
                                                    && self.lines[second_else].trim() == "else {"
                                                {
                                                    if let Some(second_else_end) =
                                                        Self::find_block_end(
                                                            &self.lines,
                                                            second_else,
                                                        )
                                                    {
                                                        for idx in second_else + 1..second_else_end
                                                        {
                                                            out.push(Self::dedent_once(
                                                                &self.lines[idx],
                                                            ));
                                                        }
                                                        i = second_else_end + 1;
                                                    } else {
                                                        i = second_end + 1;
                                                    }
                                                } else {
                                                    i = second_end + 1;
                                                }
                                                changed = true;
                                                rewritten = true;
                                                break;
                                            }
                                        }
                                    }

                                    scan += 1;
                                }
                                if rewritten {
                                    continue;
                                }
                            }
                        }
                    }

                    let cond = cur_trim
                        .strip_prefix("if (")
                        .and_then(|s| s.strip_suffix(") {"))
                        .unwrap_or("");

                    if !cond.contains("flags.") && !cond.contains("/* cond */") {
                        if let Some(then_end) = Self::find_block_end(&self.lines, i) {
                            if let Some(then_ret) =
                                Self::single_top_level_return(&self.lines, i + 1, then_end)
                            {
                                let mut next = then_end + 1;
                                while next < self.lines.len() && self.lines[next].trim().is_empty()
                                {
                                    next += 1;
                                }
                                if next < self.lines.len() && self.lines[next].trim() == then_ret {
                                    let indent =
                                        cur.chars().take_while(|c| c.is_whitespace()).count();
                                    out.push(format!("{}{}", " ".repeat(indent), then_ret));
                                    i = next + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(then_end) = Self::find_block_end(&self.lines, i) {
                        let mut else_start = then_end + 1;
                        while else_start < self.lines.len()
                            && self.lines[else_start].trim().is_empty()
                        {
                            else_start += 1;
                        }
                        if else_start < self.lines.len()
                            && self.lines[else_start].trim() == "else {"
                        {
                            if let Some(else_end) = Self::find_block_end(&self.lines, else_start) {
                                if Self::block_terminates_at_top_level(&self.lines, i + 1, then_end)
                                {
                                    for idx in i..=then_end {
                                        out.push(self.lines[idx].clone());
                                    }
                                    for idx in else_start + 1..else_end {
                                        out.push(Self::dedent_once(&self.lines[idx]));
                                    }
                                    i = else_end + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }

                    if !cond.contains("flags.") && !cond.contains("/* cond */") {
                        let mut j = i + 1;
                        while j < self.lines.len() && self.lines[j].trim().is_empty() {
                            j += 1;
                        }
                        if j < self.lines.len() && self.lines[j].trim().starts_with("return ") {
                            let then_ret = self.lines[j].trim().to_string();
                            let mut k = j + 1;
                            while k < self.lines.len() && self.lines[k].trim().is_empty() {
                                k += 1;
                            }
                            if k < self.lines.len() && self.lines[k].trim() == "}" {
                                let mut l = k + 1;
                                while l < self.lines.len() && self.lines[l].trim().is_empty() {
                                    l += 1;
                                }
                                if l < self.lines.len() && self.lines[l].trim() == "else {" {
                                    let mut m = l + 1;
                                    while m < self.lines.len() && self.lines[m].trim().is_empty() {
                                        m += 1;
                                    }
                                    if m < self.lines.len() && self.lines[m].trim() == then_ret {
                                        let mut n = m + 1;
                                        while n < self.lines.len()
                                            && self.lines[n].trim().is_empty()
                                        {
                                            n += 1;
                                        }
                                        if n < self.lines.len() && self.lines[n].trim() == "}" {
                                            let indent = cur
                                                .chars()
                                                .take_while(|c| c.is_whitespace())
                                                .count();
                                            out.push(format!("{}{}", " ".repeat(indent), then_ret));
                                            i = n + 1;
                                            changed = true;
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let mut j = i + 1;
                    while j < self.lines.len() && self.lines[j].trim().is_empty() {
                        j += 1;
                    }
                    if j < self.lines.len() && self.lines[j].trim() == "}" {
                        let mut k = j + 1;
                        while k < self.lines.len() && self.lines[k].trim().is_empty() {
                            k += 1;
                        }
                        if k < self.lines.len() && self.lines[k].trim() == "else {" {
                            let mut depth = 0i32;
                            let mut m = None;
                            for idx in k..self.lines.len() {
                                let line = &self.lines[idx];
                                depth += line.chars().filter(|&c| c == '{').count() as i32;
                                depth -= line.chars().filter(|&c| c == '}').count() as i32;
                                if depth == 0 {
                                    m = Some(idx);
                                    break;
                                }
                            }
                            if let Some(m) = m {
                                if let Some(cond) = cur_trim
                                    .strip_prefix("if (")
                                    .and_then(|s| s.strip_suffix(") {"))
                                {
                                    let indent =
                                        cur.chars().take_while(|c| c.is_whitespace()).count();
                                    out.push(format!("{}if (!({})) {{", " ".repeat(indent), cond));
                                    for line in &self.lines[k + 1..m] {
                                        out.push(line.clone());
                                    }
                                    out.push(self.lines[m].clone());
                                    i = m + 1;
                                    changed = true;
                                    continue;
                                }
                            }
                        }
                    }
                }

                if cur_trim == "else {" {
                    let mut j = i + 1;
                    while j < self.lines.len() && self.lines[j].trim().is_empty() {
                        j += 1;
                    }
                    if j < self.lines.len() && self.lines[j].trim() == "}" {
                        i = j + 1;
                        changed = true;
                        continue;
                    }
                }

                if cur_trim == "return null;"
                    && out
                        .last()
                        .is_some_and(|p: &String| p.trim() == "return null;")
                {
                    i += 1;
                    changed = true;
                    continue;
                }

                out.push(cur.clone());
                i += 1;
            }

            self.lines = out;
            if !changed {
                break;
            }
        }
    }

    fn is_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    fn dedent_once(line: &str) -> String {
        line.strip_prefix("  ").unwrap_or(line).to_string()
    }

    fn leading_indent(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }

    fn find_block_end(lines: &[String], start: usize) -> Option<usize> {
        let mut depth = 0i32;
        for (idx, line) in lines.iter().enumerate().skip(start) {
            depth += line.chars().filter(|&c| c == '{').count() as i32;
            depth -= line.chars().filter(|&c| c == '}').count() as i32;
            if depth == 0 {
                return Some(idx);
            }
        }
        None
    }

    fn if_condition(line_trim: &str) -> Option<&str> {
        Some(
            line_trim
                .strip_prefix("if (")
                .and_then(|s| s.strip_suffix(") {"))?
                .trim(),
        )
    }

    fn parse_simple_cmp(cond: &str) -> Option<(String, String, String)> {
        let c = cond.trim();
        if c.contains("||") || c.contains("&&") {
            return None;
        }
        for op in [">=", "<=", "==", "!=", ">", "<"] {
            if let Some((lhs, rhs)) = c.split_once(op) {
                return Some((
                    lhs.trim().to_string(),
                    op.to_string(),
                    rhs.trim().to_string(),
                ));
            }
        }
        None
    }

    fn parse_int_literal(s: &str) -> Option<i64> {
        let t = s.trim().trim_start_matches('#');
        if let Some(hex) = t.strip_prefix("0x") {
            return i64::from_str_radix(hex, 16).ok();
        }
        t.parse::<i64>().ok()
    }

    fn retry_decl_var(line_trim: &str) -> Option<String> {
        let rest = line_trim.strip_prefix("bool ")?;
        let var = rest.strip_suffix(" = true;")?.trim();
        if var.is_empty() || !var.chars().all(Self::is_ident_char) {
            return None;
        }
        Some(var.to_string())
    }

    fn while_var(line_trim: &str) -> Option<String> {
        let var = line_trim
            .strip_prefix("while (")
            .and_then(|s| s.strip_suffix(") {"))?
            .trim();
        if var.is_empty() || !var.chars().all(Self::is_ident_char) {
            return None;
        }
        Some(var.to_string())
    }

    fn null_checked_ident(line_trim: &str) -> Option<String> {
        let cond = Self::if_condition(line_trim)?;
        let (lhs, rhs) = cond.split_once("==")?;
        let lhs = lhs.trim();
        let rhs = rhs.trim();
        let ident = if rhs == "null" {
            lhs
        } else if lhs == "null" {
            rhs
        } else {
            return None;
        };
        if ident.is_empty() || !ident.chars().all(Self::is_ident_char) {
            return None;
        }
        Some(ident.to_string())
    }

    fn assigns_ident(line: &str, ident: &str) -> bool {
        let t = line.trim();
        if t.starts_with("if (") {
            return false;
        }
        let mut i = 0usize;
        let bytes = t.as_bytes();
        while i + ident.len() <= t.len() {
            if t[i..].starts_with(ident) {
                let prev_ok = if i == 0 {
                    true
                } else {
                    !Self::is_ident_char(bytes[i - 1] as char)
                };
                let next_i = i + ident.len();
                let next_ok = if next_i >= t.len() {
                    true
                } else {
                    !Self::is_ident_char(bytes[next_i] as char)
                };
                if prev_ok && next_ok {
                    let rest = t[next_i..].trim_start();
                    if rest.starts_with('=') && !rest.starts_with("==") {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    fn block_terminates_at_top_level(lines: &[String], start: usize, end: usize) -> bool {
        if start >= end || end > lines.len() {
            return false;
        }

        let mut rel_depth = 1i32;
        let mut last_top_level_stmt = None;
        for line in lines.iter().take(end).skip(start) {
            let t = line.trim();
            if rel_depth == 1 && !t.is_empty() {
                last_top_level_stmt = Some(t.to_string());
            }
            rel_depth += line.chars().filter(|&c| c == '{').count() as i32;
            rel_depth -= line.chars().filter(|&c| c == '}').count() as i32;
        }

        let Some(stmt) = last_top_level_stmt else {
            return false;
        };
        stmt.starts_with("return ") || stmt == "continue;" || stmt == "break;"
    }

    fn single_top_level_return(lines: &[String], start: usize, end: usize) -> Option<String> {
        let only = Self::single_top_level_stmt(lines, start, end)?;
        if only.starts_with("return ") {
            Some(only)
        } else {
            None
        }
    }

    fn single_top_level_stmt(lines: &[String], start: usize, end: usize) -> Option<String> {
        if start >= end || end > lines.len() {
            return None;
        }

        let mut rel_depth = 1i32;
        let mut top_level: Vec<String> = Vec::new();
        for line in lines.iter().take(end).skip(start) {
            let t = line.trim();
            if rel_depth == 1 && !t.is_empty() {
                top_level.push(t.to_string());
            }
            rel_depth += line.chars().filter(|&c| c == '{').count() as i32;
            rel_depth -= line.chars().filter(|&c| c == '}').count() as i32;
        }

        if top_level.len() != 1 {
            return None;
        }
        Some(top_level.remove(0))
    }

    fn replace_identifier_token(line: &str, from: &str, to: &str) -> String {
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

    fn collect_ident_stats(lines: &[String], id: &str) -> IdentStats {
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

    fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
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

    fn apply_name_and_type_hints(&mut self, fn_name: &str) {
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

    fn field_expr(base: &str, off: i64) -> String {
        let b = if base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            base.to_string()
        } else {
            format!("({base})")
        };

        if b == "sp" || b == "stack" {
            return format!("{b}[{}]", fmt_int(off));
        }

        if off == -1 {
            format!("{b}._tag")
        } else if off >= 0 {
            format!("{b}.f{off}")
        } else {
            format!("{b}.m{}", -off)
        }
    }

    fn rewrite_bitfield_classid(input: &str) -> String {
        let mut out = String::new();
        let bytes = input.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            if input[i..].starts_with("bitField(") {
                let start = i + "bitField(".len();
                let mut j = start;
                let mut depth = 1i32;
                while j < bytes.len() {
                    let c = bytes[j] as char;
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < bytes.len() {
                    let inside = input[start..j].trim();
                    if let Some(prefix) = inside.strip_suffix(", 0xc, 0x14") {
                        let base = prefix.trim().strip_suffix("._tag").unwrap_or(prefix.trim());
                        out.push_str(&format!("classId({})", base));
                        i = j + 1;
                        continue;
                    }
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    fn clean_expr(expr: String) -> String {
        let mut s = expr;
        s = s.replace(" + x28 /* lsl #32 */", "");
        s = s.replace(" + x28", "");
        s = Self::rewrite_bitfield_classid(&s);
        s
    }

    fn lookup_reg(&self, token: &str) -> String {
        if is_zero_reg(token) {
            return "0".to_string();
        }
        if let Some(reg) = canonical_reg(token) {
            return Self::clean_expr(self.state.reg_values.get(&reg).cloned().unwrap_or(reg));
        }
        Self::clean_expr(token.trim().trim_start_matches('#').to_string())
    }

    fn operand_expr(&self, token: &str) -> String {
        if is_zero_reg(token) {
            return "0".to_string();
        }
        if let Some(reg) = canonical_reg(token) {
            return Self::clean_expr(self.state.reg_values.get(&reg).cloned().unwrap_or(reg));
        }

        if let Some((base, off)) = parse_mem_operand(token) {
            if base == "x29" {
                if let Some(name) = self.locals.get(&off) {
                    return Self::clean_expr(name.clone());
                }
                return Self::clean_expr(local_name(off));
            }

            let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
            return Self::clean_expr(Self::field_expr(&base_expr, off));
        }

        Self::clean_expr(token.trim().trim_start_matches('#').to_string())
    }

    fn apply_other_lift(&mut self, ins_src: &str, indent: usize) {
        let (mnemonic, ops) = split_instruction(ins_src);

        match mnemonic.as_str() {
            "mov" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let rhs = self.operand_expr(&ops[1]);
                    self.state.reg_values.insert(dst, rhs);
                }
            }
            "add" | "sub" | "mul" | "and" | "orr" | "eor" if ops.len() >= 3 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let lhs = self.operand_expr(&ops[1]);
                    let mut rhs = self.operand_expr(&ops[2]);
                    if ops.len() > 3 {
                        rhs = format!("{} /* {} */", rhs, ops[3..].join(", "));
                    }
                    let op = match mnemonic.as_str() {
                        "add" => "+",
                        "sub" => "-",
                        "mul" => "*",
                        "and" => "&",
                        "orr" => "|",
                        "eor" => "^",
                        _ => "?",
                    };
                    let expr = if mnemonic == "add" || mnemonic == "sub" {
                        simplify_bin_expr(lhs, op, rhs)
                    } else {
                        format!("({} {} {})", lhs, op, rhs)
                    };
                    self.state.reg_values.insert(dst, expr);
                }
            }
            "lsl" | "lsr" | "asr" if ops.len() >= 3 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let lhs = self.operand_expr(&ops[1]);
                    let rhs = self.operand_expr(&ops[2]);
                    let op = match mnemonic.as_str() {
                        "lsl" => "<<",
                        "lsr" => ">>",
                        "asr" => ">>",
                        _ => "?",
                    };
                    self.state
                        .reg_values
                        .insert(dst, format!("({} {} {})", lhs, op, rhs));
                }
            }
            "ubfx" if ops.len() >= 4 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let src = self.operand_expr(&ops[1]);
                    let lsb = self.operand_expr(&ops[2]);
                    let width = self.operand_expr(&ops[3]);
                    self.state
                        .reg_values
                        .insert(dst, format!("bitField({}, {}, {})", src, lsb, width));
                }
            }
            "ldur" | "ldr" if ops.len() >= 2 => {
                if let Some(dst) = canonical_reg(&ops[0]) {
                    let rhs = self.operand_expr(&ops[1]);
                    self.state.reg_values.insert(dst, rhs);
                }
            }
            "stur" | "str" if ops.len() >= 2 => {
                if let Some((base, off)) = parse_mem_operand(&ops[1]) {
                    let rhs = self.operand_expr(&ops[0]);
                    if base == "x29" {
                        let local = self
                            .locals
                            .get(&off)
                            .cloned()
                            .unwrap_or_else(|| local_name(off));
                        self.push_line(indent, &format!("{} = {};", local, rhs));
                    } else {
                        let base_expr = self.state.reg_values.get(&base).cloned().unwrap_or(base);
                        let lhs = Self::field_expr(&base_expr, off);
                        self.push_line(indent, &format!("{} = {};", lhs, rhs));
                    }
                }
            }
            "cmp" if ops.len() >= 2 => {
                let lhs = self.operand_expr(&ops[0]);
                let rhs = self.operand_expr(&ops[1]);
                self.state.last_cmp = Some((lhs, rhs));
            }
            "ret" => {}
            _ => {}
        }
    }

    fn branch_condition(&self, mnemonic: &str, ops: &[String]) -> Option<String> {
        if mnemonic.starts_with("b.") {
            if let Some(cmp) = &self.state.last_cmp {
                return cond_from_cmp(mnemonic, cmp);
            }
            return Some(format!("flags.{}", mnemonic.replace('.', "_")));
        }

        if mnemonic == "cbz" && !ops.is_empty() {
            let v = self.operand_expr(&ops[0]);
            return Some(format!("{} == 0", v));
        }
        if mnemonic == "cbnz" && !ops.is_empty() {
            let v = self.operand_expr(&ops[0]);
            return Some(format!("{} != 0", v));
        }
        if mnemonic == "tbz" && ops.len() >= 2 {
            let v = self.operand_expr(&ops[0]);
            let bit = self.lookup_reg(&ops[1]);
            return Some(format!("(({} >> {}) & 1) == 0", v, bit));
        }
        if mnemonic == "tbnz" && ops.len() >= 2 {
            let v = self.operand_expr(&ops[0]);
            let bit = self.lookup_reg(&ops[1]);
            return Some(format!("(({} >> {}) & 1) != 0", v, bit));
        }

        None
    }

    fn branch_target_block(&self, target: &str) -> Option<usize> {
        let normalized = normalize_target(target);
        let va = normalized.strip_prefix("0x")?;
        let parsed = u64::from_str_radix(va, 16).ok()?;
        self.va_to_id.get(&parsed).copied()
    }

    fn can_inline(&self, to: usize, depth: usize) -> bool {
        if depth >= 12 {
            return false;
        }
        if self.active_stack.contains(&to) {
            return false;
        }
        if self.inline_visits.get(&to).copied().unwrap_or(0) >= self.visit_limit(to) {
            return false;
        }
        self.block_by_id.contains_key(&to)
    }

    fn has_backedge_pred(&self, id: usize) -> bool {
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        for pred in &block.preds {
            if let Some(pb) = self.block_by_id.get(pred) {
                if pb.succs.contains(&id) && pb.start_va >= block.start_va {
                    return true;
                }
            }
        }
        false
    }

    fn has_forward_pred(&self, id: usize) -> bool {
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        for pred in &block.preds {
            if let Some(pb) = self.block_by_id.get(pred) {
                if pb.succs.contains(&id) && pb.start_va < block.start_va {
                    return true;
                }
            }
        }
        false
    }

    fn should_wrap_loop_header(&self, id: usize, depth: usize) -> bool {
        if depth >= 10 {
            return false;
        }
        if !self.loop_context.is_empty() {
            return false;
        }
        if self.loop_context.contains(&id) {
            return false;
        }
        if self.active_stack.contains(&id) {
            return false;
        }
        if self.inline_visits.get(&id).copied().unwrap_or(0) >= self.visit_limit(id) {
            return false;
        }
        let Some(block) = self.block_by_id.get(&id) else {
            return false;
        };
        let tail = block.instrs.last().map(|i| &i.op);
        if !matches!(tail, Some(IROp::Branch)) {
            return false;
        }
        if block.succs.len() < 2 {
            return false;
        }
        self.has_backedge_pred(id) && self.has_forward_pred(id)
    }

    fn emit_wrapped_loop(&mut self, id: usize, indent: usize, depth: usize) {
        self.loop_context.push(id);
        self.push_line(indent, "while (true) {");
        self.emit_block(id, indent + 1, depth + 1);
        self.push_line(indent + 1, "break;");
        self.push_line(indent, "}");
        self.loop_context.pop();
    }

    fn parse_helper_header(line: &str) -> Option<usize> {
        let t = line.trim();
        if !t.starts_with("dynamic _block_") || !t.ends_with("() {") {
            return None;
        }
        let rest = t.strip_prefix("dynamic _block_")?;
        let id_s = rest.strip_suffix("() {")?;
        id_s.parse::<usize>().ok()
    }

    fn parse_helper_call(line: &str) -> Option<usize> {
        let t = line.trim();
        if !t.starts_with("return _block_") || !t.ends_with("();") {
            return None;
        }
        let rest = t.strip_prefix("return _block_")?;
        let id_s = rest.strip_suffix("();")?;
        id_s.parse::<usize>().ok()
    }

    fn scan_helpers(lines: &[String]) -> Vec<HelperMeta> {
        let mut out = Vec::new();
        let mut i = 0usize;

        while i < lines.len() {
            let Some(id) = Self::parse_helper_header(&lines[i]) else {
                i += 1;
                continue;
            };

            let mut depth = 0i32;
            let mut j = i;
            while j < lines.len() {
                let line = &lines[j];
                depth += line.chars().filter(|&c| c == '{').count() as i32;
                depth -= line.chars().filter(|&c| c == '}').count() as i32;
                if depth == 0 {
                    break;
                }
                j += 1;
            }
            if j >= lines.len() {
                break;
            }

            let mut body_lines = Vec::new();
            for line in &lines[i + 1..j] {
                body_lines.push(line.clone());
            }

            let mut statements = Vec::new();
            for line in &lines[i + 1..j] {
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                statements.push(t.to_string());
            }
            let return_expr = if statements.len() == 1 {
                let stmt = &statements[0];
                if stmt.starts_with("return ") && stmt.ends_with(';') {
                    Some(
                        stmt.trim_start_matches("return ")
                            .trim_end_matches(';')
                            .trim()
                            .to_string(),
                    )
                } else {
                    None
                }
            } else {
                None
            };

            out.push(HelperMeta {
                id,
                start: i,
                end: j,
                body_lines,
                return_expr,
            });
            i = j + 1;
        }

        out
    }

    fn token_count(lines: &[String], token: &str) -> usize {
        lines.iter().map(|l| l.matches(token).count()).sum()
    }

    fn leading_spaces(line: &str) -> usize {
        line.chars().take_while(|c| c.is_whitespace()).count()
    }

    fn helper_inline_lines(meta: &HelperMeta) -> Option<InlineHelperPlan> {
        let non_empty: Vec<&String> = meta
            .body_lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .collect();
        if non_empty.is_empty() || non_empty.len() > 28 {
            return None;
        }

        for line in &non_empty {
            let t = line.trim();
            if t.contains("_block_") {
                return None;
            }
        }

        let last = non_empty.last()?.trim();
        let linear_last_return = last.starts_with("return ") && last.ends_with(';');
        let linear_no_braces = non_empty.iter().all(|l| {
            let t = l.trim();
            !t.contains('{') && !t.contains('}')
        });
        if linear_last_return && linear_no_braces {
            return Some(InlineHelperPlan {
                lines: meta.body_lines.clone(),
                append_null_return: false,
            });
        }

        // Single top-level if/else helper:
        // if (...) { ... } else { ... }
        let trimmed: Vec<&str> = non_empty.iter().map(|l| l.trim()).collect();
        if trimmed
            .first()
            .is_some_and(|l| l.starts_with("if (") && l.ends_with('{'))
        {
            let mut depth = 0i32;
            let mut if_end = None;
            for (idx, line) in trimmed.iter().enumerate() {
                depth += line.chars().filter(|&c| c == '{').count() as i32;
                depth -= line.chars().filter(|&c| c == '}').count() as i32;
                if depth == 0 {
                    if_end = Some(idx);
                    break;
                }
            }
            if let Some(if_end) = if_end {
                if if_end + 1 < trimmed.len() && trimmed[if_end + 1].starts_with("else {") {
                    depth = 0;
                    let mut else_end = None;
                    for (idx, line) in trimmed.iter().enumerate().skip(if_end + 1) {
                        depth += line.chars().filter(|&c| c == '{').count() as i32;
                        depth -= line.chars().filter(|&c| c == '}').count() as i32;
                        if depth == 0 {
                            else_end = Some(idx);
                            break;
                        }
                    }
                    if let Some(else_end) = else_end {
                        if else_end == trimmed.len() - 1 {
                            let has_return_if = trimmed
                                .iter()
                                .take(if_end)
                                .skip(1)
                                .any(|l| l.starts_with("return ") && l.ends_with(';'));
                            let has_return_else = trimmed
                                .iter()
                                .take(else_end)
                                .skip(if_end + 2)
                                .any(|l| l.starts_with("return ") && l.ends_with(';'));

                            return Some(InlineHelperPlan {
                                lines: meta.body_lines.clone(),
                                append_null_return: !(has_return_if && has_return_else),
                            });
                        }
                    }
                }
            }
        }

        // Fallback: inline small mixed helpers (setup + branch) without nested _block calls.
        let mut depth = 0i32;
        let mut balanced = true;
        for line in &trimmed {
            depth += line.chars().filter(|&c| c == '{').count() as i32;
            depth -= line.chars().filter(|&c| c == '}').count() as i32;
            if depth < 0 {
                balanced = false;
                break;
            }
        }
        if balanced && depth == 0 {
            return Some(InlineHelperPlan {
                lines: meta.body_lines.clone(),
                append_null_return: true,
            });
        }

        None
    }

    fn inline_helper_calls(&mut self, helper_id: usize, plan: &InlineHelperPlan) {
        let call = format!("return _block_{}();", helper_id);

        let mut i = 0usize;
        while i < self.lines.len() {
            if self.lines[i].trim() != call {
                i += 1;
                continue;
            }

            let call_indent = Self::leading_spaces(&self.lines[i]);
            let base_indent = plan
                .lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| Self::leading_spaces(l))
                .min()
                .unwrap_or(0);

            let mut replacement = Vec::new();
            for line in &plan.lines {
                if line.trim().is_empty() {
                    continue;
                }
                let rel = Self::leading_spaces(line).saturating_sub(base_indent);
                replacement.push(format!(
                    "{}{}",
                    " ".repeat(call_indent + rel),
                    line.trim_start()
                ));
            }
            if replacement.is_empty() {
                replacement.push(format!("{}return null;", " ".repeat(call_indent)));
            }
            if plan.append_null_return {
                replacement.push(format!("{}return null;", " ".repeat(call_indent)));
            }

            self.lines.splice(i..=i, replacement.clone());
            i += replacement.len();
        }
    }

    fn inline_trivial_helpers(&mut self) {
        let first_pass = Self::scan_helpers(&self.lines);
        if first_pass.is_empty() {
            return;
        }

        for h in &first_pass {
            if let Some(expr) = &h.return_expr {
                let call = format!("return _block_{}();", h.id);
                let repl = format!("return {};", expr);
                for line in &mut self.lines {
                    if line.trim() == call {
                        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
                        *line = format!("{}{}", " ".repeat(indent), repl);
                    }
                }
            }
        }

        let second_pass = Self::scan_helpers(&self.lines);
        for h in &second_pass {
            let Some(plan) = Self::helper_inline_lines(h) else {
                continue;
            };
            self.inline_helper_calls(h.id, &plan);
        }

        let final_helpers = Self::scan_helpers(&self.lines);
        let mut remove_ranges = Vec::new();
        for h in &final_helpers {
            let token = format!("_block_{}(", h.id);
            if Self::token_count(&self.lines, &token) <= 1 {
                remove_ranges.push((h.start, h.end));
            }
        }
        remove_ranges.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (start, end) in remove_ranges {
            self.lines.drain(start..=end);
        }
    }

    fn collapse_remaining_helpers(&mut self) {
        let mut omitted_ids = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            let Some(id) = Self::parse_helper_call(&self.lines[i]) else {
                i += 1;
                continue;
            };

            if seen_ids.insert(id) {
                omitted_ids.push(id);
            }
            let indent = Self::leading_spaces(&self.lines[i]);
            let replacement = vec![format!("{}return null;", " ".repeat(indent))];
            self.lines.splice(i..=i, replacement.clone());
            i += replacement.len();
        }

        if !omitted_ids.is_empty() {
            omitted_ids.sort_unstable();
            omitted_ids.dedup();
            let details = omitted_ids
                .iter()
                .map(|id| format!("block {}", id))
                .collect::<Vec<_>>()
                .join(", ");
            let summary = format!("  // omitted complex paths: {}", details);
            let mut insert_idx = 1usize;
            while insert_idx < self.lines.len() {
                let t = self.lines[insert_idx].trim_start();
                if t.starts_with("var ") || t.starts_with("int ") || t.starts_with("dynamic ") {
                    insert_idx += 1;
                    continue;
                }
                if self.lines[insert_idx].trim().is_empty() {
                    insert_idx += 1;
                }
                break;
            }
            self.lines.insert(insert_idx, summary);
        }

        let helpers = Self::scan_helpers(&self.lines);
        if helpers.is_empty() {
            return;
        }

        let mut remove_ranges: Vec<(usize, usize)> =
            helpers.into_iter().map(|h| (h.start, h.end)).collect();
        remove_ranges.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (start, end) in remove_ranges {
            self.lines.drain(start..=end);
        }
    }

    fn insert_loop_summary_comment(&mut self) {
        if self.loop_back_edges.is_empty() || self.lines.is_empty() {
            return;
        }

        let details = self
            .loop_back_edges
            .iter()
            .map(|id| format!("block {}", id))
            .collect::<Vec<_>>()
            .join(", ");
        let summary = format!("  // loop back-edges: {}", details);

        let mut insert_idx = 1usize;
        while insert_idx < self.lines.len() {
            let t = self.lines[insert_idx].trim_start();
            if t.starts_with("var ") || t.starts_with("int ") || t.starts_with("dynamic ") {
                insert_idx += 1;
                continue;
            }
            if self.lines[insert_idx].trim().is_empty() {
                insert_idx += 1;
            }
            break;
        }
        self.lines.insert(insert_idx, summary);
    }

    fn visit_limit(&self, id: usize) -> usize {
        if let Some(block) = self.block_by_id.get(&id) {
            let tail = block.instrs.last().map(|i| &i.op);
            if block.instrs.len() <= 3 && matches!(tail, Some(IROp::Jump | IROp::Return)) {
                return 48;
            }
            if block.preds.len() > 1 {
                return 24;
            }
        }
        14
    }

    fn append_helper_functions(&mut self) {
        let mut generated = BTreeSet::new();
        let mut queue: Vec<usize> = self.omitted_blocks.iter().copied().collect();
        let mut queued: HashSet<usize> = queue.iter().copied().collect();

        while let Some(id) = queue.pop() {
            queued.remove(&id);
            if !generated.insert(id) {
                continue;
            }
            if generated.len() > 64 {
                break;
            }

            let mut helper = FuncEmitter::new(self.ir, self.symbol_names);
            helper.emit_block(id, 1, 0);
            let has_terminator = helper.lines.iter().any(|line| {
                let t = line.trim_start();
                t.starts_with("return ") || t == "continue;"
            });
            let fallback_return = helper
                .state
                .reg_values
                .get("x0")
                .cloned()
                .unwrap_or_else(|| "null".to_string());

            self.lines.push(format!("dynamic _block_{}() {{", id));
            self.lines.extend(helper.lines);
            if !has_terminator {
                self.push_line(1, &format!("return {};", fallback_return));
            }
            self.lines.push("}".to_string());

            for next in helper.omitted_blocks {
                if !generated.contains(&next) && !queued.contains(&next) {
                    queue.push(next);
                    queued.insert(next);
                }
            }
        }
    }

    fn emit_call(&mut self, ins_target: &str, indent: usize) {
        self.total_calls += 1;
        self.state.call_index += 1;

        let tname = format!("t{}", self.state.call_index);
        let args = (0..4)
            .map(|r| {
                self.state
                    .reg_values
                    .get(&format!("x{r}"))
                    .cloned()
                    .unwrap_or_else(|| format!("arg{r}"))
            })
            .collect::<Vec<_>>()
            .join(", ");

        let target = normalize_target(ins_target);
        if target.starts_with('x') {
            self.indirect_calls += 1;
            self.raw_register_calls += 1;
            let named_target = named_indirect_target(&target);
            self.push_line(
                indent,
                &format!(
                    "final {} = dynamicCall({}, [{}]);",
                    tname, named_target, args
                ),
            );
        } else {
            let call_name = if let Some(hex) = target.strip_prefix("0x") {
                if let Ok(va) = u64::from_str_radix(hex, 16) {
                    if let Some(name) = self.symbol_names.get(&va) {
                        sanitize_name(name)
                    } else {
                        format!("fn_{}", target)
                    }
                } else {
                    format!("fn_{}", target)
                }
            } else {
                format!("fn_{}", target)
            };
            self.push_line(
                indent,
                &format!("final {} = {}({});", tname, call_name, args),
            );
        }
        self.state.reg_values.insert("x0".to_string(), tname);
    }

    fn emit_block(&mut self, id: usize, indent: usize, depth: usize) {
        if self.should_wrap_loop_header(id, depth) {
            self.emit_wrapped_loop(id, indent, depth);
            return;
        }
        if depth >= 12 {
            self.push_line(indent, "// depth-limited block");
            return;
        }
        if self.active_stack.contains(&id) {
            if self.loop_context.contains(&id) {
                self.push_line(indent, "continue;");
            } else {
                self.loop_back_edges.insert(id);
            }
            return;
        }
        if self.inline_visits.get(&id).copied().unwrap_or(0) >= self.visit_limit(id) {
            self.emit_omitted_path(indent, Some(id));
            return;
        }

        let block = match self.block_by_id.get(&id) {
            Some(b) => *b,
            None => return,
        };

        self.emitted.insert(id);
        *self.inline_visits.entry(id).or_insert(0) += 1;
        self.active_stack.push(id);

        for ins in &block.instrs {
            match ins.op {
                IROp::Call => {
                    self.emit_call(&ins.target, indent);
                }
                IROp::LoadPool => {
                    let ops = split_operands(&ins.src);
                    if let Some(dst) = ops.first().and_then(|o| canonical_reg(o)) {
                        let rhs = if ins.target.is_empty() {
                            "pool[?]".to_string()
                        } else {
                            ins.target.clone()
                        };
                        self.state.reg_values.insert(dst, Self::clean_expr(rhs));
                    }
                }
                IROp::Branch => {
                    let (mnemonic, ops) = split_instruction(&ins.src);
                    let cond = self.branch_condition(&mnemonic, &ops);
                    let true_id = self.branch_target_block(&ins.target);
                    let false_id = {
                        let mut other = None;
                        for s in &block.succs {
                            if Some(*s) != true_id {
                                other = Some(*s);
                                break;
                            }
                        }
                        other
                    };

                    let cond_str = match cond {
                        Some(c) => Self::clean_expr(c),
                        None => {
                            self.placeholder_ifs += 1;
                            "/* cond */".to_string()
                        }
                    };

                    self.push_line(indent, &format!("if ({}) {{", cond_str));
                    if let Some(tid) = true_id {
                        if self.can_inline(tid, depth + 1) {
                            let saved = self.state.clone();
                            self.emit_block(tid, indent + 1, depth + 1);
                            self.state = saved;
                        } else {
                            self.emit_omitted_path(indent + 1, Some(tid));
                        }
                    } else {
                        let target = normalize_target(&ins.target);
                        if target.starts_with("0x") {
                            self.push_line(indent + 1, "/* external branch */");
                        } else {
                            self.unresolved_cf += 1;
                            self.push_line(indent + 1, "// unresolved branch target");
                        }
                    }
                    self.push_line(indent, "}");

                    if let Some(fid) = false_id {
                        if self.can_inline(fid, depth + 1) {
                            self.push_line(indent, "else {");
                            let saved = self.state.clone();
                            self.emit_block(fid, indent + 1, depth + 1);
                            self.state = saved;
                            self.push_line(indent, "}");
                        } else if !self.emitted.contains(&fid) {
                            self.push_line(indent, "else {");
                            self.emit_omitted_path(indent + 1, Some(fid));
                            self.push_line(indent, "}");
                        }
                    }

                    self.active_stack.pop();
                    return;
                }
                IROp::Jump => {
                    let target_id = self.branch_target_block(&ins.target);
                    if let Some(tid) = target_id {
                        if self.can_inline(tid, depth + 1) {
                            self.emit_block(tid, indent, depth + 1);
                        } else if self.active_stack.contains(&tid) {
                            if self.loop_context.contains(&tid) {
                                self.push_line(indent, "continue;");
                            } else {
                                self.loop_back_edges.insert(tid);
                            }
                        } else if !self.emitted.contains(&tid) {
                            self.emit_omitted_path(indent, Some(tid));
                        }
                    } else {
                        let target = normalize_target(&ins.target);
                        if target.starts_with("0x") {
                            self.push_line(indent, &format!("return tailCall_{}();", target));
                        } else {
                            self.unresolved_cf += 1;
                            self.push_line(indent, "// unresolved jump");
                        }
                    }
                    self.active_stack.pop();
                    return;
                }
                IROp::Return => {
                    let ret = self
                        .state
                        .reg_values
                        .get("x0")
                        .cloned()
                        .unwrap_or_else(|| "null".to_string());
                    self.push_line(indent, &format!("return {};", ret));
                    self.active_stack.pop();
                    return;
                }
                IROp::Other => {
                    self.apply_other_lift(&ins.src, indent);
                }
            }
        }

        self.active_stack.pop();
    }
}

pub fn emit_pseudocode(ir: &FunctionIr, symbol_names: &HashMap<u64, String>) -> PseudocodeArtifact {
    FuncEmitter::new(ir, symbol_names).emit()
}

pub fn emit_program(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
) -> Vec<PseudocodeArtifact> {
    ir.iter()
        .map(|f| emit_pseudocode(f, symbol_names))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};

    fn branch_block(
        id: usize,
        va: u64,
        true_va: u64,
        false_id: usize,
        true_id: usize,
    ) -> BasicBlock {
        BasicBlock {
            id,
            start_va: va,
            instrs: vec![LlirInstr {
                va,
                op: IROp::Branch,
                src: format!("cbz x0, #0x{true_va:x}"),
                target: format!("#0x{true_va:x}"),
            }],
            succs: vec![true_id, false_id],
            preds: Vec::new(),
        }
    }

    fn jump_block(id: usize, va: u64, to_id: usize, to_va: u64) -> BasicBlock {
        BasicBlock {
            id,
            start_va: va,
            instrs: vec![LlirInstr {
                va,
                op: IROp::Jump,
                src: format!("b #0x{to_va:x}"),
                target: format!("#0x{to_va:x}"),
            }],
            succs: vec![to_id],
            preds: Vec::new(),
        }
    }

    #[test]
    fn emits_helper_bodies_for_omitted_paths() {
        let va = |id: usize| 0x1000 + (id as u64) * 4;
        let mut blocks = vec![
            branch_block(0, va(0), va(1), 2, 1),
            branch_block(1, va(1), va(3), 4, 3),
            branch_block(2, va(2), va(5), 6, 5),
            branch_block(3, va(3), va(7), 8, 7),
            branch_block(4, va(4), va(9), 10, 9),
            branch_block(5, va(5), va(11), 12, 11),
            branch_block(6, va(6), va(13), 14, 13),
            jump_block(7, va(7), 15, va(15)),
            jump_block(8, va(8), 15, va(15)),
            jump_block(9, va(9), 15, va(15)),
            jump_block(10, va(10), 15, va(15)),
            jump_block(11, va(11), 15, va(15)),
            jump_block(12, va(12), 15, va(15)),
            jump_block(13, va(13), 15, va(15)),
            jump_block(14, va(14), 15, va(15)),
            BasicBlock {
                id: 15,
                start_va: va(15),
                instrs: vec![LlirInstr {
                    va: va(15),
                    op: IROp::Return,
                    src: "ret".to_string(),
                    target: String::new(),
                }],
                succs: Vec::new(),
                preds: vec![7, 8, 9, 10, 11, 12, 13, 14],
            },
        ];

        for b in &mut blocks {
            b.preds.clear();
        }
        for idx in 0..blocks.len() {
            let pred = blocks[idx].id;
            let succs = blocks[idx].succs.clone();
            for succ in succs {
                if let Some(target) = blocks.iter_mut().find(|b| b.id == succ) {
                    target.preds.push(pred);
                }
            }
        }

        let ir = FunctionIr {
            function_id: 1,
            name: "testFunc".to_string(),
            entry_va: va(0),
            blocks,
        };
        let symbols = HashMap::new();
        let artifact = emit_pseudocode(&ir, &symbols);

        assert!(
            !artifact.source.contains("path omitted"),
            "unexpected placeholder stub:\n{}",
            artifact.source
        );
        if artifact.source.contains("return _block_15();") {
            assert!(artifact.source.contains("dynamic _block_15() {"));
        }
    }

    #[test]
    fn inlines_trivial_return_helpers() {
        let mut blocks = Vec::new();
        for id in 0..12usize {
            blocks.push(jump_block(
                id,
                0x2000 + (id as u64) * 4,
                id + 1,
                0x2000 + ((id + 1) as u64) * 4,
            ));
        }
        blocks.push(BasicBlock {
            id: 12,
            start_va: 0x2000 + 12 * 4,
            instrs: vec![LlirInstr {
                va: 0x2000 + 12 * 4,
                op: IROp::Return,
                src: "ret".to_string(),
                target: String::new(),
            }],
            succs: Vec::new(),
            preds: vec![11],
        });

        for b in &mut blocks {
            b.preds.clear();
        }
        for idx in 0..blocks.len() {
            let pred = blocks[idx].id;
            let succs = blocks[idx].succs.clone();
            for succ in succs {
                if let Some(target) = blocks.iter_mut().find(|b| b.id == succ) {
                    target.preds.push(pred);
                }
            }
        }

        let ir = FunctionIr {
            function_id: 2,
            name: "deepChain".to_string(),
            entry_va: 0x2000,
            blocks,
        };
        let symbols = HashMap::new();
        let artifact = emit_pseudocode(&ir, &symbols);

        assert!(
            !artifact.source.contains("return _block_12();"),
            "trivial helper call should be inlined:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("dynamic _block_12()"),
            "trivial helper should be removed:\n{}",
            artifact.source
        );
    }

    #[test]
    fn inlines_linear_helper_body_at_call_site() {
        let ir = FunctionIr {
            function_id: 3,
            name: "manualInline".to_string(),
            entry_va: 0x3000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic manualInline(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_1();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_1() {".to_string(),
            "  final t1 = fn_0x1(arg0, arg1, arg2, arg3);".to_string(),
            "  return t1;".to_string(),
            "}".to_string(),
        ];

        emitter.inline_trivial_helpers();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("return _block_1();"),
            "call should be inlined:\n{out}"
        );
        assert!(
            !out.contains("dynamic _block_1()"),
            "unused helper should be removed:\n{out}"
        );
        assert!(
            out.contains("final t1 = fn_0x1(arg0, arg1, arg2, arg3);"),
            "linear helper body should be inserted:\n{out}"
        );
    }

    #[test]
    fn inlines_branch_helper_body_with_null_fallback() {
        let ir = FunctionIr {
            function_id: 4,
            name: "manualBranchInline".to_string(),
            entry_va: 0x4000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic manualBranchInline(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_9();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_9() {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    final t1 = fn_0x2(arg0, arg1, arg2, arg3);".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    return arg0;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.inline_trivial_helpers();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("return _block_9();"),
            "call should be inlined:\n{out}"
        );
        assert!(
            !out.contains("dynamic _block_9()"),
            "unused helper should be removed:\n{out}"
        );
        assert!(
            out.contains("if (arg0 == null) {"),
            "branch helper body should be inserted:\n{out}"
        );
        assert!(
            out.contains("return null;"),
            "non-total branch helper should append null fallback:\n{out}"
        );
    }

    #[test]
    fn inlines_placeholder_cond_helper_body() {
        let ir = FunctionIr {
            function_id: 5,
            name: "manualCondInline".to_string(),
            entry_va: 0x5000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic manualCondInline(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_3();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_3() {".to_string(),
            "  if (/* cond */) {".to_string(),
            "    return null;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    final t1 = fn_0x3(arg0, arg1, arg2, arg3);".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.inline_trivial_helpers();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("return _block_3();"),
            "call should be inlined:\n{out}"
        );
        assert!(
            !out.contains("dynamic _block_3()"),
            "unused helper should be removed:\n{out}"
        );
        assert!(
            out.contains("if (/* cond */) {"),
            "placeholder condition helper should be inlined:\n{out}"
        );
    }

    #[test]
    fn compacts_empty_else_and_duplicate_null_returns() {
        let ir = FunctionIr {
            function_id: 6,
            name: "manualCompact".to_string(),
            entry_va: 0x6000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic manualCompact(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return null;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "  }".to_string(),
            "  return null;".to_string(),
            "  return null;".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("else {\n  }"),
            "empty else should be removed:\n{out}"
        );
        assert!(
            !out.contains("return null;\n  return null;"),
            "duplicate null returns should collapse:\n{out}"
        );
    }

    #[test]
    fn emits_flag_predicate_when_cmp_is_missing() {
        let ir = FunctionIr {
            function_id: 7,
            name: "flagFallback".to_string(),
            entry_va: 0x7000,
            blocks: vec![
                BasicBlock {
                    id: 0,
                    start_va: 0x7000,
                    instrs: vec![LlirInstr {
                        va: 0x7000,
                        op: IROp::Branch,
                        src: "b.eq #0x7008".to_string(),
                        target: "#0x7008".to_string(),
                    }],
                    succs: vec![1, 2],
                    preds: Vec::new(),
                },
                BasicBlock {
                    id: 1,
                    start_va: 0x7008,
                    instrs: vec![LlirInstr {
                        va: 0x7008,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    }],
                    succs: Vec::new(),
                    preds: vec![0],
                },
                BasicBlock {
                    id: 2,
                    start_va: 0x7004,
                    instrs: vec![LlirInstr {
                        va: 0x7004,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    }],
                    succs: Vec::new(),
                    preds: vec![0],
                },
            ],
        };
        let symbols = HashMap::new();
        let artifact = emit_pseudocode(&ir, &symbols);
        assert!(
            artifact.source.contains("if (flags.b_eq) {"),
            "missing flag predicate fallback:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("/* cond */"),
            "placeholder cond should not be emitted:\n{}",
            artifact.source
        );
    }

    #[test]
    fn infers_local_names_and_int_types() {
        let ir = FunctionIr {
            function_id: 8,
            name: "manualHints".to_string(),
            entry_va: 0x8000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.locals.insert(-8, "local_m8".to_string());
        emitter.locals.insert(-16, "local_m16".to_string());
        emitter.lines = vec![
            "dynamic manualHints(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  var local_m8;".to_string(),
            "  var local_m16;".to_string(),
            "".to_string(),
            "  local_m8 = (arg2 + 1);".to_string(),
            "  local_m8 = (local_m8 + 2);".to_string(),
            "  local_m8 = (local_m8 << 1);".to_string(),
            "  local_m16 = pool[42];".to_string(),
            "  if (local_m16.f7 == null) {".to_string(),
            "    return local_m8;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.apply_name_and_type_hints("manualHints");
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("local_m8"),
            "stack local should be renamed:\n{out}"
        );
        assert!(
            !out.contains("local_m16"),
            "stack local should be renamed:\n{out}"
        );
        assert!(
            out.contains("int intTmp"),
            "arithmetic local should get int type:\n{out}"
        );
        assert!(
            out.contains("dynamic poolVal"),
            "pool-assigned local should get poolVal naming:\n{out}"
        );
    }

    #[test]
    fn renames_receiver_argument_from_field_usage() {
        let ir = FunctionIr {
            function_id: 9,
            name: "receiverHints".to_string(),
            entry_va: 0x9000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic receiverHints(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0.f7 == null) {".to_string(),
            "    return arg0;".to_string(),
            "  }".to_string(),
            "  return arg0.f11;".to_string(),
            "}".to_string(),
        ];

        emitter.apply_name_and_type_hints("receiverHints");
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("dynamic receiver"),
            "arg0 should be renamed to receiver:\n{out}"
        );
        assert!(
            !out.contains("arg0.f"),
            "field access should use receiver:\n{out}"
        );
    }

    #[test]
    fn renames_receiver_argument_without_field_usage() {
        let ir = FunctionIr {
            function_id: 10,
            name: "receiverDefault".to_string(),
            entry_va: 0xa000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic receiverDefault(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  final t1 = fn_0x10(arg0, arg1, arg2, arg3);".to_string(),
            "  return t1;".to_string(),
            "}".to_string(),
        ];

        emitter.apply_name_and_type_hints("receiverDefault");
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("dynamic receiver"),
            "arg0 should default to receiver:\n{out}"
        );
        assert!(!out.contains("arg0"), "arg0 should be replaced:\n{out}");
        assert!(
            out.contains("dynamic param1"),
            "non-inferred args should use param naming:\n{out}"
        );
    }

    #[test]
    fn aliases_raw_register_names_after_hinting() {
        let ir = FunctionIr {
            function_id: 11,
            name: "regAlias".to_string(),
            entry_va: 0xb000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic regAlias(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  final t1 = invoke(x2, [arg0, arg1, arg2, arg3]);".to_string(),
            "  final t2 = invoke(x30, [arg0, arg1, arg2, arg3]);".to_string(),
            "  return t2;".to_string(),
            "}".to_string(),
        ];

        emitter.apply_name_and_type_hints("regAlias");
        let out = emitter.lines.join("\n");
        assert!(!out.contains("x2"), "x2 should be aliased:\n{out}");
        assert!(!out.contains("x30"), "x30 should be aliased:\n{out}");
        assert!(out.contains("reg2"), "reg2 alias missing:\n{out}");
        assert!(
            out.contains("returnAddress"),
            "x30 should map to returnAddress:\n{out}"
        );
    }

    #[test]
    fn aliases_frame_and_return_registers_with_semantic_names() {
        let ir = FunctionIr {
            function_id: 21,
            name: "frameRegs".to_string(),
            entry_va: 0xf600,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic frameRegs(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  final t1 = x29;".to_string(),
            "  final t2 = x30;".to_string(),
            "  return t2;".to_string(),
            "}".to_string(),
        ];

        emitter.apply_name_and_type_hints("frameRegs");
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("framePointer") && out.contains("returnAddress"),
            "x29/x30 should use semantic aliases:\n{out}"
        );
    }

    #[test]
    fn emits_dynamic_call_for_indirect_targets() {
        let ir = FunctionIr {
            function_id: 12,
            name: "indirectCall".to_string(),
            entry_va: 0xc000,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0xc000,
                instrs: vec![
                    LlirInstr {
                        va: 0xc000,
                        op: IROp::Call,
                        src: "blr x9".to_string(),
                        target: "x9".to_string(),
                    },
                    LlirInstr {
                        va: 0xc004,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        };
        let symbols = HashMap::new();
        let artifact = emit_pseudocode(&ir, &symbols);
        assert!(
            artifact.source.contains("dynamicCall(indirectTarget9"),
            "indirect calls should use named targets:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("invoke(reg9"),
            "legacy invoke label should be absent:\n{}",
            artifact.source
        );
    }

    #[test]
    fn structures_simple_backedge_as_while_loop() {
        let ir = FunctionIr {
            function_id: 13,
            name: "simpleLoop".to_string(),
            entry_va: 0xd000,
            blocks: vec![
                BasicBlock {
                    id: 0,
                    start_va: 0xd000,
                    instrs: vec![LlirInstr {
                        va: 0xd000,
                        op: IROp::Jump,
                        src: "b #0xd004".to_string(),
                        target: "#0xd004".to_string(),
                    }],
                    succs: vec![1],
                    preds: Vec::new(),
                },
                BasicBlock {
                    id: 1,
                    start_va: 0xd004,
                    instrs: vec![
                        LlirInstr {
                            va: 0xd004,
                            op: IROp::Other,
                            src: "add x0, x0, #1".to_string(),
                            target: String::new(),
                        },
                        LlirInstr {
                            va: 0xd008,
                            op: IROp::Branch,
                            src: "cbnz x0, #0xd010".to_string(),
                            target: "#0xd010".to_string(),
                        },
                    ],
                    succs: vec![2, 3],
                    preds: vec![0, 2],
                },
                BasicBlock {
                    id: 2,
                    start_va: 0xd00c,
                    instrs: vec![LlirInstr {
                        va: 0xd00c,
                        op: IROp::Jump,
                        src: "b #0xd004".to_string(),
                        target: "#0xd004".to_string(),
                    }],
                    succs: vec![1],
                    preds: vec![1],
                },
                BasicBlock {
                    id: 3,
                    start_va: 0xd010,
                    instrs: vec![LlirInstr {
                        va: 0xd010,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    }],
                    succs: Vec::new(),
                    preds: vec![1],
                },
            ],
        };

        let artifact = emit_pseudocode(&ir, &HashMap::new());
        assert!(
            artifact.source.contains("while (true) {"),
            "simple backedge should become loop:\n{}",
            artifact.source
        );
        assert!(
            artifact.source.contains("continue;"),
            "loop backedge inside loop should use continue:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("loop back-edges:"),
            "loop summary should not be needed for simple structured loop:\n{}",
            artifact.source
        );
    }

    #[test]
    fn normalizes_zero_register_operands() {
        let ir = FunctionIr {
            function_id: 14,
            name: "zeroRegs".to_string(),
            entry_va: 0xe000,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0xe000,
                instrs: vec![
                    LlirInstr {
                        va: 0xe000,
                        op: IROp::Other,
                        src: "mov w1, wzr".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xe004,
                        op: IROp::Other,
                        src: "stur w1, [x29, #-8]".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xe008,
                        op: IROp::Other,
                        src: "mov x2, xzr".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xe00c,
                        op: IROp::Other,
                        src: "stur x2, [x29, #-16]".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xe010,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        };

        let artifact = emit_pseudocode(&ir, &HashMap::new());
        assert!(
            artifact.source.contains("= 0;"),
            "zero register should normalize to 0:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("xzr") && !artifact.source.contains("wzr"),
            "raw zero registers should not leak:\n{}",
            artifact.source
        );
    }

    #[test]
    fn collapses_helper_calls_into_omitted_path_comments() {
        let ir = FunctionIr {
            function_id: 15,
            name: "helperCollapse".to_string(),
            entry_va: 0xf000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic helperCollapse(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  return _block_3();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_3() {".to_string(),
            "  final t1 = fn_0x3(arg0, arg1, arg2, arg3);".to_string(),
            "  return t1;".to_string(),
            "}".to_string(),
        ];

        emitter.collapse_remaining_helpers();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("_block_"),
            "helper scaffolding should be removed:\n{out}"
        );
        assert!(
            out.contains("omitted complex paths: block 3"),
            "function should include omitted-path summary:\n{out}"
        );
        assert!(
            out.contains("return null;"),
            "call should get a safe fallback return:\n{out}"
        );
    }

    #[test]
    fn summarizes_duplicate_omitted_blocks_once() {
        let ir = FunctionIr {
            function_id: 16,
            name: "helperCollapseDedup".to_string(),
            entry_va: 0xf100,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic helperCollapseDedup(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return _block_9();".to_string(),
            "  }".to_string(),
            "  return _block_9();".to_string(),
            "}".to_string(),
            String::new(),
            "dynamic _block_9() {".to_string(),
            "  return arg0;".to_string(),
            "}".to_string(),
        ];

        emitter.collapse_remaining_helpers();
        let out = emitter.lines.join("\n");
        assert_eq!(
            out.matches("omitted complex paths: block 9").count(),
            1,
            "duplicate omitted blocks should be summarized once:\n{out}"
        );
        assert_eq!(
            out.matches("return null;").count(),
            2,
            "each omitted callsite should become return null:\n{out}"
        );
    }

    #[test]
    fn simplifies_null_base_add_immediate() {
        let ir = FunctionIr {
            function_id: 17,
            name: "nullAdd".to_string(),
            entry_va: 0xf200,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0xf200,
                instrs: vec![
                    LlirInstr {
                        va: 0xf200,
                        op: IROp::Other,
                        src: "add x1, x22, #0x20".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf204,
                        op: IROp::Other,
                        src: "stur x1, [x29, #-8]".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf208,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        };

        let artifact = emit_pseudocode(&ir, &HashMap::new());
        assert!(
            artifact.source.contains("= 0x20;"),
            "null-based add should collapse to literal:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("(null + 0x20)"),
            "legacy null arithmetic should be removed:\n{}",
            artifact.source
        );
    }

    #[test]
    fn folds_nested_stack_offset_arithmetic() {
        let ir = FunctionIr {
            function_id: 18,
            name: "stackOffsetFold".to_string(),
            entry_va: 0xf300,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0xf300,
                instrs: vec![
                    LlirInstr {
                        va: 0xf300,
                        op: IROp::Other,
                        src: "sub x1, x15, #0x20".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf304,
                        op: IROp::Other,
                        src: "add x2, x1, #0x10".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf308,
                        op: IROp::Other,
                        src: "stur x2, [x29, #-8]".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf30c,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        };

        let artifact = emit_pseudocode(&ir, &HashMap::new());
        assert!(
            artifact.source.contains("(sp - 0x10)"),
            "stack-offset expression should fold:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("((sp - 0x20) + 0x10)"),
            "unfolded nested arithmetic should not remain:\n{}",
            artifact.source
        );
    }

    #[test]
    fn does_not_inject_alternative_path_comment() {
        let ir = FunctionIr {
            function_id: 19,
            name: "noAlternativePath".to_string(),
            entry_va: 0xf400,
            blocks: vec![
                BasicBlock {
                    id: 0,
                    start_va: 0xf400,
                    instrs: vec![LlirInstr {
                        va: 0xf400,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    }],
                    succs: Vec::new(),
                    preds: Vec::new(),
                },
                BasicBlock {
                    id: 1,
                    start_va: 0xf404,
                    instrs: vec![LlirInstr {
                        va: 0xf404,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    }],
                    succs: Vec::new(),
                    preds: Vec::new(),
                },
            ],
        };

        let artifact = emit_pseudocode(&ir, &HashMap::new());
        assert!(
            !artifact.source.contains("alternative path"),
            "synthetic alternative-path comment should not be emitted:\n{}",
            artifact.source
        );
    }

    #[test]
    fn renders_stack_access_as_indexed_slot() {
        let ir = FunctionIr {
            function_id: 20,
            name: "stackSlots".to_string(),
            entry_va: 0xf500,
            blocks: vec![BasicBlock {
                id: 0,
                start_va: 0xf500,
                instrs: vec![
                    LlirInstr {
                        va: 0xf500,
                        op: IROp::Other,
                        src: "ldr x1, [x15, #8]".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf504,
                        op: IROp::Other,
                        src: "str x1, [x15, #-8]".to_string(),
                        target: String::new(),
                    },
                    LlirInstr {
                        va: 0xf508,
                        op: IROp::Return,
                        src: "ret".to_string(),
                        target: String::new(),
                    },
                ],
                succs: Vec::new(),
                preds: Vec::new(),
            }],
        };

        let artifact = emit_pseudocode(&ir, &HashMap::new());
        assert!(
            artifact.source.contains("sp[8]"),
            "positive stack slots should use index notation:\n{}",
            artifact.source
        );
        assert!(
            artifact.source.contains("sp[-8]"),
            "negative stack slots should use index notation:\n{}",
            artifact.source
        );
        assert!(
            !artifact.source.contains("sp.f8") && !artifact.source.contains("sp.m8"),
            "legacy stack field notation should not remain:\n{}",
            artifact.source
        );
    }

    #[test]
    fn rewrites_empty_then_else_to_negated_if() {
        let ir = FunctionIr {
            function_id: 22,
            name: "emptyThen".to_string(),
            entry_va: 0xf700,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic emptyThen(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("if (!(arg0 == null)) {"),
            "empty then/else should be rewritten:\n{out}"
        );
        assert!(
            !out.contains("else {"),
            "else branch should be absorbed:\n{out}"
        );
    }

    #[test]
    fn collapses_if_else_with_identical_returns() {
        let ir = FunctionIr {
            function_id: 25,
            name: "sameReturn".to_string(),
            entry_va: 0xfa00,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic sameReturn(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("if (arg0 == null) {"),
            "identical return branches should collapse:\n{out}"
        );
        assert_eq!(
            out.matches("return arg1;").count(),
            1,
            "collapsed output should keep one return:\n{out}"
        );
    }

    #[test]
    fn collapses_if_then_return_followed_by_same_return() {
        let ir = FunctionIr {
            function_id: 34,
            name: "sameReturnNoElse".to_string(),
            entry_va: 0x13000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic sameReturnNoElse(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  return arg1;".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("if (arg0 == null) {"),
            "redundant guarded return should collapse:\n{out}"
        );
        assert_eq!(
            out.matches("return arg1;").count(),
            1,
            "collapsed output should keep one return:\n{out}"
        );
    }

    #[test]
    fn hoists_else_when_then_terminates() {
        let ir = FunctionIr {
            function_id: 26,
            name: "hoistElse".to_string(),
            entry_va: 0xfb00,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic hoistElse(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  else {".to_string(),
            "    final t1 = fn_0x1(arg0, arg1, arg2, arg3);".to_string(),
            "    return t1;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("if (arg0 == null) {\n    return arg1;\n  }\n  final t1 = fn_0x1"),
            "else body should be hoisted after terminating then-branch:\n{out}"
        );
        assert!(!out.contains("else {"), "else should be removed:\n{out}");
    }

    #[test]
    fn merges_nested_single_if_guards() {
        let ir = FunctionIr {
            function_id: 29,
            name: "mergeNestedIf".to_string(),
            entry_va: 0xfe00,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic mergeNestedIf(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 != null) {".to_string(),
            "    if (arg1 != null) {".to_string(),
            "      return arg2;".to_string(),
            "    }".to_string(),
            "  }".to_string(),
            "  return null;".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("if ((arg0 != null) && (arg1 != null)) {"),
            "nested if guards should merge:\n{out}"
        );
        assert!(
            !out.contains("if (arg0 != null) {\n    if (arg1 != null) {"),
            "legacy nested guard shape should be removed:\n{out}"
        );
    }

    #[test]
    fn removes_redundant_null_check_after_terminating_guard() {
        let ir = FunctionIr {
            function_id: 27,
            name: "redundantNull".to_string(),
            entry_va: 0xfc00,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic redundantNull(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    final t1 = fn_0x1(arg0, arg1, arg2, arg3);".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    return t1;".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("if (arg0 == null) {\n      continue;"),
            "redundant second null-check should be removed:\n{out}"
        );
        assert!(
            !out.contains("while (true) {"),
            "removing synthetic continue should allow wrapper unwrap:\n{out}"
        );
    }

    #[test]
    fn keeps_null_check_when_identifier_is_reassigned() {
        let ir = FunctionIr {
            function_id: 28,
            name: "reassignedNull".to_string(),
            entry_va: 0xfd00,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic reassignedNull(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  arg0 = arg1;".to_string(),
            "  if (arg0 == null) {".to_string(),
            "    return arg2;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.matches("if (arg0 == null) {").count() >= 2,
            "second null-check must stay when variable is reassigned:\n{out}"
        );
    }

    #[test]
    fn unwraps_single_iteration_while_without_continue() {
        let ir = FunctionIr {
            function_id: 23,
            name: "loopWrapper".to_string(),
            entry_va: 0xf800,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic loopWrapper(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("while (true) {"),
            "single-iteration wrappers should be removed:\n{out}"
        );
        assert!(
            out.contains("if (arg0 == null) {"),
            "body should remain after unwrap:\n{out}"
        );
    }

    #[test]
    fn keeps_while_wrapper_when_continue_exists() {
        let ir = FunctionIr {
            function_id: 24,
            name: "loopContinue".to_string(),
            entry_va: 0xf900,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic loopContinue(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("while (true) {"),
            "real loop control flow should keep wrapper:\n{out}"
        );
        assert!(out.contains("continue;"), "continue should remain:\n{out}");
    }

    #[test]
    fn rewrites_multi_continue_loop_as_retry_condition() {
        let ir = FunctionIr {
            function_id: 30,
            name: "retryLoop".to_string(),
            entry_va: 0xff00,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic retryLoop(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    if (arg1 == null) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    return arg2;".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("bool retryLoop1 = true;") && out.contains("while (retryLoop1) {"),
            "multi-continue loop should get retry condition:\n{out}"
        );
        assert!(
            out.contains("retryLoop1 = false;"),
            "retry fall-through update should be emitted:\n{out}"
        );
        assert!(
            !out.contains("while (true) {"),
            "generic while(true) should be removed for multi-continue loops:\n{out}"
        );
    }

    #[test]
    fn merges_consecutive_continue_guards() {
        let ir = FunctionIr {
            function_id: 33,
            name: "continueGuards".to_string(),
            entry_va: 0x12000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic continueGuards(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  while (true) {".to_string(),
            "    if (arg0 == 0x85) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    if (arg0 == 0xa0) {".to_string(),
            "      continue;".to_string(),
            "    }".to_string(),
            "    return arg1;".to_string(),
            "    break;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("if ((arg0 == 0x85) || (arg0 == 0xa0)) {"),
            "continue guards should merge:\n{out}"
        );
        assert_eq!(
            out.matches("continue;").count(),
            1,
            "merged guard should keep one continue:\n{out}"
        );
    }

    #[test]
    fn rewrites_return_then_continue_range_pattern() {
        let ir = FunctionIr {
            function_id: 35,
            name: "rangeContinue".to_string(),
            entry_va: 0x14000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic rangeContinue(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  if (arg0 > 0xd) {".to_string(),
            "    return arg1;".to_string(),
            "  }".to_string(),
            "  if (arg0 >= 9) {".to_string(),
            "    continue;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            out.contains("if ((arg0 >= 9) && (arg0 <= 0xd)) {"),
            "range continue guard should be emitted:\n{out}"
        );
        assert!(
            out.contains("if (arg0 > 0xd) {\n    return arg1;\n  }"),
            "upper tail return branch should remain:\n{out}"
        );
    }

    #[test]
    fn unwraps_retry_loop_when_no_retry_paths_remain() {
        let ir = FunctionIr {
            function_id: 31,
            name: "retryCleanup".to_string(),
            entry_va: 0x10000,
            blocks: Vec::new(),
        };
        let symbols = HashMap::new();
        let mut emitter = FuncEmitter::new(&ir, &symbols);
        emitter.lines = vec![
            "dynamic retryCleanup(dynamic arg0, dynamic arg1, dynamic arg2, dynamic arg3, dynamic arg4, dynamic arg5, dynamic arg6, dynamic arg7) {".to_string(),
            "  bool retryLoop1 = true;".to_string(),
            "  while (retryLoop1) {".to_string(),
            "    retryLoop1 = false;".to_string(),
            "    if (arg0 == null) {".to_string(),
            "      return arg1;".to_string(),
            "    }".to_string(),
            "    return arg2;".to_string(),
            "  }".to_string(),
            "}".to_string(),
        ];

        emitter.compact_lines();
        let out = emitter.lines.join("\n");
        assert!(
            !out.contains("retryLoop1"),
            "one-shot retry wrappers should collapse:\n{out}"
        );
        assert!(
            out.contains("if (arg0 == null) {"),
            "loop body should remain:\n{out}"
        );
    }

    #[test]
    fn normalize_target_prefers_last_hex_operand() {
        let got = normalize_target("x0, #0x3f, #0x2008");
        assert_eq!(got, "0x2008");
    }
}
