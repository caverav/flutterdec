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
    pub semantic_direct_calls: usize,
    pub semantic_indirect_calls: usize,
    pub dispatch_selector_calls: usize,
    pub target_va_symbol_calls: usize,
}

#[derive(Debug, Clone, Default)]
pub struct PoolSemanticHint {
    pub selector: Option<String>,
    pub owner_class: Option<String>,
    pub library_uri: Option<String>,
    pub target_va: Option<u64>,
}

#[derive(Debug, Default, Clone)]
struct LiftState {
    reg_values: HashMap<String, String>,
    selector_hints: HashMap<String, String>,
    last_cmp: Option<(String, String)>,
    call_index: usize,
}

struct FuncEmitter<'a> {
    ir: &'a FunctionIr,
    symbol_names: &'a HashMap<u64, String>,
    pool_value_hints: HashMap<u64, String>,
    pool_semantic_hints: HashMap<u64, PoolSemanticHint>,
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
    semantic_direct_calls: usize,
    semantic_indirect_calls: usize,
    dispatch_selector_calls: usize,
    target_va_symbol_calls: usize,
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

mod control_flow;
mod helper_flow;
mod helpers;
mod passes;

use helpers::*;

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
            pool_value_hints: HashMap::new(),
            pool_semantic_hints: HashMap::new(),
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
            semantic_direct_calls: 0,
            semantic_indirect_calls: 0,
            dispatch_selector_calls: 0,
            target_va_symbol_calls: 0,
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
        self.extract_minus_one_aliases();

        PseudocodeArtifact {
            function_id: self.ir.function_id,
            function_name: fn_name,
            source: self.lines.join("\n"),
            placeholder_ifs: self.placeholder_ifs,
            unresolved_cf: self.unresolved_cf,
            raw_register_calls: self.raw_register_calls,
            total_calls: self.total_calls,
            indirect_calls: self.indirect_calls,
            semantic_direct_calls: self.semantic_direct_calls,
            semantic_indirect_calls: self.semantic_indirect_calls,
            dispatch_selector_calls: self.dispatch_selector_calls,
            target_va_symbol_calls: self.target_va_symbol_calls,
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
}

pub fn emit_pseudocode(ir: &FunctionIr, symbol_names: &HashMap<u64, String>) -> PseudocodeArtifact {
    FuncEmitter::new(ir, symbol_names).emit()
}

pub fn emit_pseudocode_with_pool_hints(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
) -> PseudocodeArtifact {
    let empty = HashMap::new();
    emit_pseudocode_with_pool_context(ir, symbol_names, pool_value_hints, &empty)
}

pub fn emit_pseudocode_with_pool_context(
    ir: &FunctionIr,
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
) -> PseudocodeArtifact {
    let mut emitter = FuncEmitter::new(ir, symbol_names);
    emitter.pool_value_hints = pool_value_hints.clone();
    emitter.pool_semantic_hints = pool_semantic_hints.clone();
    emitter.emit()
}

pub fn emit_program(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
) -> Vec<PseudocodeArtifact> {
    let empty = HashMap::new();
    emit_program_with_pool_hints(ir, symbol_names, &empty)
}

pub fn emit_program_with_pool_hints(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
) -> Vec<PseudocodeArtifact> {
    let empty = HashMap::new();
    emit_program_with_pool_context(ir, symbol_names, pool_value_hints, &empty)
}

pub fn emit_program_with_pool_context(
    ir: &[FunctionIr],
    symbol_names: &HashMap<u64, String>,
    pool_value_hints: &HashMap<u64, String>,
    pool_semantic_hints: &HashMap<u64, PoolSemanticHint>,
) -> Vec<PseudocodeArtifact> {
    let mut artifacts = ir
        .iter()
        .map(|f| {
            emit_pseudocode_with_pool_context(
                f,
                symbol_names,
                pool_value_hints,
                pool_semantic_hints,
            )
        })
        .collect::<Vec<_>>();
    apply_program_level_generic_call_rewrites(&mut artifacts);
    artifacts
}

fn apply_program_level_generic_call_rewrites(artifacts: &mut [PseudocodeArtifact]) {
    let aliases = collect_generic_symbol_aliases(artifacts);
    if aliases.is_empty() {
        return;
    }
    for artifact in artifacts {
        let mut changed = false;
        let mut lines = Vec::new();
        for line in artifact.source.lines() {
            if let Some(rewritten) = rewrite_generic_call_line(line, &aliases) {
                lines.push(rewritten);
                changed = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if changed {
            artifact.source = lines.join("\n");
        }
    }
}

fn collect_generic_symbol_aliases(artifacts: &[PseudocodeArtifact]) -> HashMap<String, String> {
    let mut candidates: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut generic_reference_counts: HashMap<String, usize> = HashMap::new();
    for artifact in artifacts {
        for line in artifact.source.lines() {
            let Some((callee, original)) = extract_rewrite_evidence(line) else {
                if let Some(callee) = extract_call_callee(line) {
                    if is_generic_call_name(callee) {
                        *generic_reference_counts
                            .entry(callee.to_string())
                            .or_insert(0) += 1;
                    }
                }
                continue;
            };
            *generic_reference_counts
                .entry(original.clone())
                .or_insert(0) += 1;
            let by_name = candidates.entry(original).or_default();
            *by_name.entry(callee).or_insert(0) += 1;
        }
    }

    let mut aliases = HashMap::new();
    for (original, by_name) in candidates {
        let mut ranked = by_name.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|(a_name, a_count), (b_name, b_count)| {
            b_count
                .cmp(a_count)
                .then_with(|| semantic_name_score(b_name).cmp(&semantic_name_score(a_name)))
                .then_with(|| a_name.cmp(b_name))
        });
        let Some((best_name, best_count)) = ranked.first().cloned() else {
            continue;
        };
        if best_count < 2 {
            continue;
        }
        let total_references = generic_reference_counts
            .get(&original)
            .copied()
            .unwrap_or(best_count);
        if best_count * 2 < total_references {
            continue;
        }
        if let Some((_, second_count)) = ranked.get(1) {
            if *second_count * 2 >= best_count {
                continue;
            }
        }
        aliases.insert(original, best_name);
    }
    aliases
}

fn semantic_name_score(name: &str) -> usize {
    if name.starts_with("flutter.") {
        return 5;
    }
    if name.starts_with("package:") {
        return 4;
    }
    if name.starts_with("dart.") {
        return 3;
    }
    if name.starts_with("dart_vm.") {
        return 2;
    }
    1
}

fn extract_rewrite_evidence(line: &str) -> Option<(String, String)> {
    if !line.contains("was: ") {
        return None;
    }
    let eq = line.find("= ")?;
    let call_start = eq + 2;
    let open = line[call_start..].find('(')? + call_start;
    let callee = line[call_start..open].trim().to_string();
    if callee.is_empty() || callee.starts_with("sub_") || callee.starts_with("fn_0x") {
        return None;
    }
    let was_idx = line.find("was: ")? + 5;
    let tail = &line[was_idx..];
    let original = tail
        .split([',', ' ', ')'])
        .find(|s| !s.trim().is_empty())?
        .trim()
        .to_string();
    if !original.starts_with("sub_") && !original.starts_with("fn_0x") {
        return None;
    }
    Some((callee, original))
}

fn rewrite_generic_call_line(line: &str, aliases: &HashMap<String, String>) -> Option<String> {
    if line.contains("was: ") {
        return None;
    }
    let eq = line.find("= ")?;
    let call_start = eq + 2;
    let open = line[call_start..].find('(')? + call_start;
    let original = line[call_start..open].trim();
    if !original.starts_with("sub_") && !original.starts_with("fn_0x") {
        return None;
    }
    let alias = aliases.get(original)?;

    let mut rewritten = String::new();
    rewritten.push_str(&line[..call_start]);
    rewritten.push_str(alias);
    rewritten.push_str(&line[open..]);
    if let Some(comment_idx) = rewritten.find("//") {
        rewritten.insert_str(comment_idx + 2, &format!(" inferred from: {}, ", original));
    } else {
        rewritten.push_str(&format!(" // inferred from: {}", original));
    }
    Some(rewritten)
}

fn extract_call_callee(line: &str) -> Option<&str> {
    let eq = line.find("= ")?;
    let call_start = eq + 2;
    let open = line[call_start..].find('(')? + call_start;
    Some(line[call_start..open].trim())
}

fn is_generic_call_name(name: &str) -> bool {
    name.starts_with("sub_") || name.starts_with("fn_0x")
}

#[cfg(test)]
mod tests;
