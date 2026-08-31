use capstone::arch::arm64::ArchMode;
use capstone::prelude::*;
use flutterdec_adapter::model::{
    Function, PoolEntryKind, PoolGeometry, PoolIndexSpace, ProgramModel,
};
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

pub mod hints;
pub use hints::{Hint, HintKind, HintOrigin, HintProvenance, ProgramHints};

#[derive(Debug, Clone, Serialize)]
pub struct AsmInstruction {
    pub va: u64,
    pub word: u32,
    pub mnemonic: String,
    pub op_str: String,
    pub annotation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDisassembly {
    pub function_id: u64,
    /// `None` when the model recovered no name. Downstream renders a label from
    /// the entry address instead of the model handing one over, so an
    /// address-derived string can never be mistaken for a recovered name.
    pub function_name: Option<String>,
    /// `None` for a top-level function and for one whose owner was not
    /// recovered. The model distinguishes those; this record does not need to.
    pub owner_class: Option<String>,
    pub entry_va: u64,
    pub size: u64,
    pub instructions: Vec<AsmInstruction>,
}

impl FunctionDisassembly {
    /// A label to print. Derived from the entry address when there is no name,
    /// which is a fact about where the code is, not a claim about what it is.
    pub fn display_name(&self) -> String {
        match &self.function_name {
            Some(name) => name.clone(),
            None => format!("fn_0x{:x}", self.entry_va),
        }
    }

    /// The owner to print, or the empty string when there is none.
    pub fn owner_label(&self) -> &str {
        self.owner_class.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionPriorityComponent {
    pub name: String,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionPriorityBreakdown {
    pub function_id: u64,
    pub function_name: Option<String>,
    pub owner_class: Option<String>,
    pub library_uri: Option<String>,
    pub entry_va: u64,
    pub total_score: i32,
    pub components: Vec<FunctionPriorityComponent>,
}

fn build_capstone() -> Option<Capstone> {
    Capstone::new()
        .arm64()
        .mode(ArchMode::Arm)
        .detail(false)
        .build()
        .ok()
}

/// `ldr xN, [x27, #imm]`: a pool load off the object-pool register directly.
static POOL_DIRECT_LOAD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[wx]\d+,\s*\[(x\d+)(?:,\s*#?(0x[0-9a-fA-F]+|[0-9]+))?\]$").unwrap()
});

/// `add xD, x27, #K` / `add xD, x27, #K, lsl #S`: materialise a pool "page" base.
/// Dart emits this pair whenever the entry displacement exceeds the `ldr` immediate range.
static POOL_PAGE_BASE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(x\d+),\s*x27,\s*#?(0x[0-9a-fA-F]+|[0-9]+)(?:,\s*lsl\s*#?(\d+))?$").unwrap()
});

/// Leading register operands, used to invalidate stale pool bases on redefinition.
/// Two are matched because load-pair forms write two destinations.
static FIRST_OPERAND_REG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[wx](\d+)\s*,\s*(?:[wx](\d+)\s*,)?").unwrap());

fn parse_u64_literal(raw: &str) -> Option<u64> {
    match raw.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => raw.parse::<u64>().ok(),
    }
}

/// Resolves object-pool references while walking one function's instructions.
///
/// Two forms reach the same slot, and both must be recognised: the direct
/// `ldr xN, [x27, #disp]`, and the page pair `add xD, x27, #K, lsl #S` followed by
/// `ldr xN, [xD, #off]` for displacements past the load-immediate range. On real
/// binaries the page form is the *majority* of pool traffic, so ignoring it loses
/// most of the pool.
///
/// Bases are tracked conservatively: any write to a register drops its base, and
/// control flow drops all of them, so a stale base can never fabricate a slot.
struct PoolRefResolver {
    geometry: Option<PoolGeometry>,
    bases: HashMap<u32, u64>,
}

impl PoolRefResolver {
    fn new(geometry: Option<PoolGeometry>) -> Self {
        Self {
            geometry,
            bases: HashMap::new(),
        }
    }

    fn reset(&mut self) {
        self.bases.clear();
    }

    /// Render a PP-relative displacement.
    ///
    /// With geometry we can name the actual entry, so callers get `pool[<index>]` in
    /// the pool's own index space. Without it the displacement is all we honestly
    /// know, and it is emitted as `poolOff[...]` so it cannot be mistaken for an
    /// index, or looked up as one.
    fn render(&self, displacement: u64) -> String {
        match self
            .geometry
            .and_then(|g| g.index_for_displacement(displacement))
        {
            Some(index) => format!("pool[{index}]"),
            None => format!("poolOff[{displacement}]"),
        }
    }

    /// Feed one instruction; returns the pool annotation when it is a pool load.
    ///
    /// A pool load reads its base register and writes its destination, and for
    /// `ldr x0, [x0, ...]` those are the same register, so the base must be
    /// resolved before the destination is invalidated.
    fn observe(&mut self, mnemonic: &str, op_str: &str) -> Option<String> {
        if mnemonic == "add" {
            if let Some(caps) = POOL_PAGE_BASE_RE.captures(op_str) {
                let dst = caps[1][1..].parse::<u32>().ok()?;
                let imm = parse_u64_literal(&caps[2])?;
                let shift = caps
                    .get(3)
                    .map(|m| m.as_str().parse::<u32>().unwrap_or(0))
                    .unwrap_or(0);
                match imm.checked_shl(shift) {
                    Some(base) => self.bases.insert(dst, base),
                    None => self.bases.remove(&dst),
                };
                return None;
            }
            self.invalidate_written_registers(op_str);
            return None;
        }

        if mnemonic != "ldr" {
            self.invalidate_written_registers(op_str);
            return None;
        }

        let Some(caps) = POOL_DIRECT_LOAD_RE.captures(op_str) else {
            self.invalidate_written_registers(op_str);
            return None;
        };
        let base_reg = caps[1][1..].parse::<u32>().ok();
        let off = caps
            .get(2)
            .and_then(|m| parse_u64_literal(m.as_str()))
            .unwrap_or(0);
        let displacement = match base_reg {
            Some(27) => Some(off),
            Some(reg) => self.bases.get(&reg).and_then(|b| b.checked_add(off)),
            None => None,
        };

        self.invalidate_written_registers(op_str);
        displacement.map(|d| self.render(d))
    }

    /// Drop the pool bases of the registers an instruction writes.
    ///
    /// Load-pair forms write two: `ldp x0, x1, [sp, #16]` must clear both, or a later
    /// `ldr xN, [x1, #off]` resolves against a base `x1` no longer holds. Store-pair
    /// forms name sources rather than destinations, so clearing there is merely
    /// conservative.
    fn invalidate_written_registers(&mut self, op_str: &str) {
        let Some(caps) = FIRST_OPERAND_REG_RE.captures(op_str) else {
            return;
        };
        for group in [1, 2] {
            if let Some(reg) = caps.get(group).and_then(|m| m.as_str().parse::<u32>().ok()) {
                self.bases.remove(&reg);
            }
        }
    }
}

fn annotation_for(mnemonic: &str, op_str: &str, pool: &mut PoolRefResolver) -> String {
    if mnemonic == "bl" || mnemonic == "blr" {
        pool.reset();
        return "call".to_string();
    }
    if mnemonic == "ret" {
        pool.reset();
        return "return".to_string();
    }
    if mnemonic == "b" {
        pool.reset();
        return "jump".to_string();
    }
    if mnemonic.starts_with("b.")
        || mnemonic == "cbz"
        || mnemonic == "cbnz"
        || mnemonic == "tbz"
        || mnemonic == "tbnz"
    {
        pool.reset();
        return "branch".to_string();
    }
    if let Some(pp) = pool.observe(mnemonic, op_str) {
        return pp;
    }
    String::new()
}

fn decode_function(
    model: &ProgramModel,
    func: &Function,
    iso_instr: &[u8],
    iso_base_va: u64,
    cs: Option<&Capstone>,
    pool_geometry: Option<PoolGeometry>,
) -> Option<FunctionDisassembly> {
    if func.code.start_va < iso_base_va {
        return None;
    }
    let rel = (func.code.start_va - iso_base_va) as usize;
    if rel >= iso_instr.len() {
        return None;
    }

    let requested = usize::try_from(func.code.size).unwrap_or(0);
    let size = requested.min(iso_instr.len() - rel);
    if size < 4 {
        return None;
    }

    let code = &iso_instr[rel..rel + size];
    let mut instructions = Vec::new();
    let mut pool = PoolRefResolver::new(pool_geometry);

    if let Some(cs) = cs {
        if let Ok(insns) = cs.disasm_all(code, func.code.start_va) {
            for ins in insns.iter() {
                let bytes = ins.bytes();
                let word = if bytes.len() >= 4 {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    0
                };
                let mnemonic = ins.mnemonic().unwrap_or("word").to_string();
                let op_str = ins.op_str().unwrap_or("").to_string();
                let annotation = annotation_for(&mnemonic, &op_str, &mut pool);
                instructions.push(AsmInstruction {
                    va: ins.address(),
                    word,
                    mnemonic,
                    op_str,
                    annotation,
                });
            }
        }
    }

    // Fallback if capstone is unavailable or disassembly failed.
    if instructions.is_empty() {
        let mut off = 0usize;
        while off + 4 <= size {
            let word = u32::from_le_bytes([
                iso_instr[rel + off],
                iso_instr[rel + off + 1],
                iso_instr[rel + off + 2],
                iso_instr[rel + off + 3],
            ]);
            let pc = func.code.start_va + off as u64;
            instructions.push(AsmInstruction {
                va: pc,
                word,
                mnemonic: "word".to_string(),
                op_str: format!("0x{word:08x}"),
                annotation: String::new(),
            });
            off += 4;
        }
    }

    Some(FunctionDisassembly {
        function_id: u64::from(func.id.0),
        function_name: func.name_text().map(str::to_string),
        owner_class: model.owner_name(func).map(str::to_string),
        entry_va: func.code.start_va,
        size: size as u64,
        instructions,
    })
}

/// The library URI a function's owning class belongs to, when both are known.
///
/// v3 keyed this on the owner *name* through a string map, so two classes with
/// the same name in different libraries collided and whichever was seen first
/// won. Typed ids remove the question.
fn function_library_uri<'a>(model: &'a ProgramModel, func: &Function) -> Option<&'a str> {
    model.owner_library_uri(func)
}

fn decode_bl_target(pc: u64, word: u32) -> Option<u64> {
    if ((word >> 26) & 0x3F) != 0b100101 {
        return None;
    }
    let mut imm26 = (word & 0x03FF_FFFF) as i64;
    if (imm26 & (1 << 25)) != 0 {
        imm26 -= 1 << 26;
    }
    Some(((pc as i64) + (imm26 << 2)) as u64)
}

fn looks_generic_name(name: &str) -> bool {
    name.starts_with("sub_") || name.starts_with("fn_0x")
}

fn has_no_isolate_marker(text_lower: &str) -> bool {
    text_lower.contains("no isolate")
        || text_lower.contains("no_isolate")
        || text_lower.contains("no-isolate")
}

fn is_main_like_name(name_lower: &str) -> bool {
    name_lower == "main"
        || name_lower.ends_with(".main")
        || name_lower.ends_with("::main")
        || name_lower.ends_with("_main")
}

fn deep_link_signal_score_lower(text_lower: &str) -> i32 {
    if text_lower.is_empty() {
        return 0;
    }

    let high_confidence = [
        "android.intent.action.view",
        "onnewintent",
        "handleintent",
        "deeplink",
        "deep_link",
        "applink",
        "app_link",
        "universallink",
        "universal_link",
        "didpushrouteinformation",
        "didpushroute",
        "setnewroutepath",
        "parserouteinformation",
        "ongenerateroute",
        "getinitialuri",
        "urilinkstream",
        "firebase_dynamic_links",
        "uni_links",
        "app_links",
    ];
    let medium_confidence = [
        "routeinformation",
        "route_info",
        "dynamiclink",
        "dynamic_link",
        "intent",
        "activity",
        "android",
    ];

    let mut score = 0i32;
    for needle in high_confidence {
        if text_lower.contains(needle) {
            score += 220;
        }
    }
    for needle in medium_confidence {
        if text_lower.contains(needle) {
            score += 90;
        }
    }
    if text_lower.contains("://")
        || text_lower.contains("http://")
        || text_lower.contains("https://")
    {
        score += 120;
    }
    score
}

fn deep_link_signal_score(text: &str) -> i32 {
    deep_link_signal_score_lower(&text.to_ascii_lowercase())
}

/// How strongly one host-derived hint argues that its address is worth
/// disassembling first.
///
/// Scored from the hint's typed kind and origin. v3 read the same signal out of
/// a `decoded_kind` string and a `value` prefix on a synthetic pool entry, which
/// meant any producer that wrote `"BootMainCandidate"` into a pool slot could
/// set analysis priority. A hint cannot be produced by an adapter at all.
fn hint_signal_score(hint: &Hint) -> i32 {
    let mut score = match (hint.kind, hint.origin) {
        (HintKind::BootMain, HintOrigin::AndroidManifest) => 5400,
        (HintKind::BootMain, _) => 6200,
        (HintKind::EntryPoint, _) => 5000,
        (HintKind::BootRunApp, HintOrigin::AndroidManifest) => 3200,
        (HintKind::BootRunApp, _) => 3800,
        (HintKind::DeepLinkHandler, HintOrigin::AndroidManifest) => 2100,
        (HintKind::DeepLinkHandler, _) => 2600,
        (HintKind::ActivityHandler, HintOrigin::AndroidManifest) => 1900,
        (HintKind::ActivityHandler, _) => 2400,
        (HintKind::BootstrapInit, HintOrigin::AndroidManifest) => 1400,
        (HintKind::BootstrapInit, _) => 1800,
    };

    let selector_lower = hint.selector.trim().to_ascii_lowercase();
    if is_main_like_name(&selector_lower) {
        score += 3200;
    } else if selector_is_runapp_like(&selector_lower) {
        score += 500;
    }

    // Framework and stdlib handlers are useful for graph seeding, but app-owned
    // ones should dominate capped reverse-engineering output.
    let framework_weighted = matches!(
        hint.kind,
        HintKind::DeepLinkHandler | HintKind::ActivityHandler | HintKind::BootstrapInit
    );
    let library_lower = hint
        .library_uri
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if framework_weighted && library_lower.starts_with("package:flutter/") {
        score /= 4;
    } else if framework_weighted && library_lower.starts_with("dart:") {
        score /= 5;
    }
    score
}

/// How strongly a pool entry's own value argues for its target address.
///
/// Only entries the adapter authored, only their real decoded value, and only
/// when the pool actually addresses code. Unlike a hint, a pool entry carries no
/// library, so the framework/stdlib de-weighting that applies to hints has
/// nothing to key on here.
fn pool_selector_signal_score(value: &str) -> i32 {
    let selector = value.trim().to_ascii_lowercase();
    if selector.is_empty() {
        return 0;
    }
    let score = match selector.as_str() {
        "runapp" => 1600,
        "createstate" => 1500,
        "build" => 1200,
        "initstate" | "dispose" => 1100,
        "didupdatewidget" | "didchangedependencies" => 900,
        "didpushrouteinformation"
        | "didpushroute"
        | "didpoproute"
        | "setnewroutepath"
        | "parserouteinformation"
        | "ongenerateroute"
        | "onunknownroute"
        | "onnewintent"
        | "handleintent" => 1400,
        "oncreate" | "onstart" | "onresume" | "onpause" | "onstop" => 900,
        "ensureinitialized" | "nativeensureinitialized" | "ensureinitializationcomplete" => 700,
        "main" => 1800,
        _ => 0,
    };
    score
}

const BOOTFLOW_SEED_CATEGORY_ORDER: [&str; 5] =
    ["main", "runapp", "deeplink", "activity", "bootstrap"];

fn selector_is_runapp_like(selector_lower: &str) -> bool {
    selector_lower == "runapp" || selector_lower.ends_with(".runapp")
}

fn selector_is_deeplink_like(selector_lower: &str) -> bool {
    matches!(
        selector_lower,
        "didpushrouteinformation"
            | "didpushroute"
            | "didpoproute"
            | "setnewroutepath"
            | "parserouteinformation"
            | "ongenerateroute"
            | "onunknownroute"
            | "onnewintent"
            | "handleintent"
    )
}

fn selector_is_activity_like(selector_lower: &str) -> bool {
    matches!(
        selector_lower,
        "onnewintent" | "handleintent" | "oncreate" | "onstart" | "onresume" | "onpause" | "onstop"
    )
}

fn selector_is_bootstrap_like(selector_lower: &str) -> bool {
    matches!(
        selector_lower,
        "ensureinitialized"
            | "nativeensureinitialized"
            | "startinitialization"
            | "ensureinitializationcomplete"
    )
}

/// Which bootflow categories are claimed for each code address.
///
/// Two independent sources, kept apart: host-derived hints, which name a
/// category directly, and pool entries the adapter authored, whose decoded
/// selector implies one. Neither can write into the other.
fn build_target_va_bootflow_categories(
    model: &ProgramModel,
    hints: &ProgramHints,
) -> HashMap<u64, HashSet<&'static str>> {
    let mut out: HashMap<u64, HashSet<&'static str>> = HashMap::new();
    for hint in hints.iter() {
        let Some(target_va) = hint.target_va else {
            continue;
        };
        out.entry(target_va)
            .or_default()
            .insert(hint.kind.category());
    }
    for (target_va, selector) in pool_code_selectors(model) {
        let lower = selector.trim().to_ascii_lowercase();
        let categories = out.entry(target_va).or_default();
        if is_main_like_name(&lower) {
            categories.insert("main");
        }
        if selector_is_runapp_like(&lower) {
            categories.insert("runapp");
        }
        if selector_is_deeplink_like(&lower) {
            categories.insert("deeplink");
        }
        if selector_is_activity_like(&lower) {
            categories.insert("activity");
        }
        if selector_is_bootstrap_like(&lower) {
            categories.insert("bootstrap");
        }
    }
    out.retain(|_, categories| !categories.is_empty());
    out
}

/// Pool entries that both name something and point at code.
///
/// Restricted to a pool whose index space is hardware: an ordinal pool is a
/// producer's own list, and a `target_va` in one is still an address the
/// producer recovered, so it is kept, but nothing here reads the index.
fn pool_code_selectors(model: &ProgramModel) -> Vec<(u64, &str)> {
    model
        .object_pool
        .entries
        .iter()
        .filter(|e| matches!(e.kind, PoolEntryKind::Code | PoolEntryKind::Selector))
        .filter_map(|e| Some((e.target_va?, e.value.as_deref()?)))
        .collect()
}

fn build_target_va_priority_hints(model: &ProgramModel, hints: &ProgramHints) -> HashMap<u64, i32> {
    let mut out: HashMap<u64, i32> = HashMap::new();
    let mut bump = |va: u64, score: i32| {
        if score <= 0 {
            return;
        }
        let clamped = score.min(6000);
        let slot = out.entry(va).or_insert(0);
        if clamped > *slot {
            *slot = clamped;
        }
    };

    for hint in hints.iter() {
        let Some(target_va) = hint.target_va else {
            continue;
        };
        let mut score = hint_signal_score(hint);
        score += deep_link_signal_score(&hint.selector);
        score += hint
            .owner_class
            .as_deref()
            .map(deep_link_signal_score)
            .unwrap_or(0);
        score += hint
            .library_uri
            .as_deref()
            .map(deep_link_signal_score)
            .unwrap_or(0);
        bump(target_va, score);
    }

    for (target_va, value) in pool_code_selectors(model) {
        let mut score = pool_selector_signal_score(value);
        score += deep_link_signal_score(value);
        bump(target_va, score);
    }
    out
}

fn push_component(out: &mut Vec<(String, i32)>, name: impl Into<String>, score: i32) {
    if score != 0 {
        out.push((name.into(), score));
    }
}

fn package_name_from_library_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("package:")?;
    let name = rest
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if name.is_empty() || name == "flutter" {
        None
    } else {
        Some(name)
    }
}

fn normalize_package_filter(raw: &str) -> Option<String> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }
    let token = token.strip_prefix("package:").unwrap_or(token);
    let name = token
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if name.is_empty() || name == "flutter" {
        return None;
    }
    Some(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PriorityLibraryKind {
    App,
    Framework,
    Stdlib,
    Unknown,
}

fn priority_library_kind(uri: &str) -> PriorityLibraryKind {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return PriorityLibraryKind::Unknown;
    }
    if trimmed.starts_with("package:flutter/") {
        return PriorityLibraryKind::Framework;
    }
    if trimmed.starts_with("dart:") {
        return PriorityLibraryKind::Stdlib;
    }
    if trimmed.starts_with("package:") || trimmed.ends_with(".dart") {
        return PriorityLibraryKind::App;
    }
    PriorityLibraryKind::Unknown
}

fn is_bootstrap_like_name(name_lower: &str) -> bool {
    name_lower.contains("ensureinitialized")
        || name_lower.contains("nativeensureinitialized")
        || name_lower.contains("ensureinitializationcomplete")
        || name_lower.contains("startinitialization")
        || name_lower.contains("attachtonative")
}

fn build_app_package_boosts(
    model: &ProgramModel,
    preferred_packages: &HashSet<String>,
) -> HashMap<String, i32> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &model.functions {
        let Some(uri) = function_library_uri(model, f) else {
            continue;
        };
        let Some(pkg) = package_name_from_library_uri(uri) else {
            continue;
        };
        *counts.entry(pkg).or_insert(0) += 1;
    }
    let counts_snapshot = counts.clone();

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(a_name, a_count), (b_name, b_count)| {
        b_count.cmp(a_count).then(a_name.cmp(b_name))
    });

    let mut out = HashMap::new();
    for (rank, (pkg, count)) in ranked.into_iter().enumerate().take(6) {
        if count < 8 {
            continue;
        }
        let mut bonus = match rank {
            0 => 900,
            1 => 650,
            2 => 500,
            3 => 380,
            4 => 300,
            _ => 240,
        };
        if pkg == "app" {
            bonus += 160;
        }
        out.insert(pkg, bonus);
    }

    // Let caller-provided package hints (for example, manifest-derived app package)
    // override count-only ranking so capped runs stay app-centric.
    for pkg in preferred_packages {
        let seen = counts_snapshot.get(pkg).copied().unwrap_or(0);
        let forced = if seen >= 12 {
            1800
        } else if seen >= 3 {
            1500
        } else {
            1200
        };
        let slot = out.entry(pkg.clone()).or_insert(forced);
        if forced > *slot {
            *slot = forced;
        }
    }
    out
}

fn collect_direct_call_targets(
    func: &Function,
    iso_instr: &[u8],
    iso_base_va: u64,
    known_entries: &HashSet<u64>,
) -> Vec<u64> {
    if func.code.start_va < iso_base_va {
        return Vec::new();
    }
    let rel = (func.code.start_va - iso_base_va) as usize;
    if rel >= iso_instr.len() {
        return Vec::new();
    }
    let requested = usize::try_from(func.code.size).unwrap_or(0);
    let max_scan = requested.min(iso_instr.len() - rel).min(0x400);
    if max_scan < 4 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut off = 0usize;
    while off + 4 <= max_scan {
        let word = u32::from_le_bytes([
            iso_instr[rel + off],
            iso_instr[rel + off + 1],
            iso_instr[rel + off + 2],
            iso_instr[rel + off + 3],
        ]);
        let pc = func.code.start_va + off as u64;
        if let Some(target) = decode_bl_target(pc, word) {
            if known_entries.contains(&target) && seen.insert(target) {
                out.push(target);
            }
        }
        off += 4;
    }
    out
}

fn build_call_adjacency(
    candidates: &[&Function],
    iso_instr: &[u8],
    iso_base_va: u64,
) -> HashMap<u64, Vec<u64>> {
    let known_entries: HashSet<u64> = candidates.iter().map(|f| f.code.start_va).collect();
    if known_entries.is_empty() {
        return HashMap::new();
    }

    let mut adjacency: HashMap<u64, Vec<u64>> = HashMap::new();
    for f in candidates {
        let callees = collect_direct_call_targets(f, iso_instr, iso_base_va, &known_entries);
        if !callees.is_empty() {
            adjacency.insert(f.code.start_va, callees);
        }
    }
    adjacency
}

fn build_entrypoint_frontier_scores(
    candidates: &[&Function],
    target_va_hints: &HashMap<u64, i32>,
    adjacency: &HashMap<u64, Vec<u64>>,
) -> HashMap<u64, i32> {
    let mut seeds = HashSet::new();
    for f in candidates {
        // An unnamed function seeds only on an address hint. There is no name to
        // pattern-match, and inventing one is what put `main` on arbitrary code.
        let name_lower = f.name_text().unwrap_or("").to_ascii_lowercase();
        if is_main_like_name(&name_lower)
            || name_lower.contains("runapp")
            || name_lower.contains("ensureinitialized")
            || target_va_hints.get(&f.code.start_va).copied().unwrap_or(0) >= 1200
        {
            seeds.insert(f.code.start_va);
        }
    }
    if seeds.is_empty() {
        return HashMap::new();
    }

    let depth_score = |depth: usize| -> i32 {
        match depth {
            0 => 2200,
            1 => 1300,
            2 => 760,
            3 => 420,
            _ => 0,
        }
    };

    let mut out = HashMap::new();
    let mut visited_depth: HashMap<u64, usize> = HashMap::new();
    let mut q = VecDeque::new();
    for seed in seeds {
        visited_depth.insert(seed, 0);
        q.push_back((seed, 0usize));
    }

    while let Some((va, depth)) = q.pop_front() {
        let score = depth_score(depth);
        if score > 0 {
            let slot = out.entry(va).or_insert(0);
            if score > *slot {
                *slot = score;
            }
        }
        if depth >= 3 {
            continue;
        }
        let Some(next) = adjacency.get(&va) else {
            continue;
        };
        for &callee in next {
            let next_depth = depth + 1;
            let seen_better = visited_depth
                .get(&callee)
                .copied()
                .is_some_and(|d| d <= next_depth);
            if seen_better {
                continue;
            }
            visited_depth.insert(callee, next_depth);
            q.push_back((callee, next_depth));
        }
    }
    out
}

fn function_size_bonus(size: u64) -> i32 {
    match size {
        0..=16 => 0,
        17..=32 => 20,
        33..=64 => 55,
        65..=128 => 95,
        129..=256 => 150,
        257..=512 => 220,
        _ => 280,
    }
}

struct FunctionScoreStats {
    call_out_degree: usize,
    name_occurrences: usize,
}

fn function_priority(
    model: &ProgramModel,
    func: &Function,
    target_va_hints: &HashMap<u64, i32>,
    app_package_boosts: &HashMap<String, i32>,
    preferred_packages: &HashSet<String>,
    entrypoint_frontier_scores: &HashMap<u64, i32>,
    stats: FunctionScoreStats,
) -> (i32, Vec<(String, i32)>) {
    let mut score = 0i32;
    let mut components = Vec::new();
    // An unrecovered name scores as an empty string rather than as a
    // stand-in: `sub_1000` used to earn a generic-name penalty *and* look like
    // a name, and neither was true.
    let name_lower = func.name_text().unwrap_or("").to_ascii_lowercase();
    let owner_lower = model.owner_name(func).unwrap_or("").to_ascii_lowercase();
    let mut library_kind = PriorityLibraryKind::Unknown;

    if has_no_isolate_marker(&name_lower) || has_no_isolate_marker(&owner_lower) {
        score -= 650;
        push_component(&mut components, "no_isolate_marker_penalty", -650);
    }

    if name_lower.is_empty() || looks_generic_name(&name_lower) {
        score -= 40;
        push_component(&mut components, "generic_name_penalty", -40);
    } else {
        score += 10;
        push_component(&mut components, "named_function_bonus", 10);
        if stats.name_occurrences > 1 {
            let repeated_name_penalty =
                (stats.name_occurrences.saturating_sub(1).min(10) as i32) * 240;
            score -= repeated_name_penalty;
            push_component(
                &mut components,
                format!("repeated_name_penalty:{}", stats.name_occurrences),
                -repeated_name_penalty,
            );
        }
    }
    if name_lower.starts_with("closure_") {
        score -= 900;
        push_component(&mut components, "closure_name_penalty", -900);
    }
    if name_lower.starts_with('_') {
        score -= 90;
        push_component(&mut components, "private_name_penalty", -90);
    }
    if func.code.size <= 16 && (name_lower.is_empty() || looks_generic_name(&name_lower)) {
        score -= 80;
        push_component(&mut components, "tiny_generic_penalty", -80);
    }
    if func.code.size <= 8 && (name_lower.starts_with("closure_") || name_lower.starts_with('_')) {
        score -= 220;
        push_component(&mut components, "tiny_wrapper_penalty", -220);
    }
    let size_bonus = function_size_bonus(func.code.size);
    score += size_bonus;
    push_component(&mut components, "function_size_bonus", size_bonus);
    if is_main_like_name(&name_lower) {
        score += 900;
        push_component(&mut components, "main_name_bonus", 900);
    }
    if name_lower.contains("runapp") {
        score += 700;
        push_component(&mut components, "runapp_name_bonus", 700);
    }
    if name_lower.contains("ensureinitialized") {
        score += 400;
        push_component(&mut components, "ensure_initialized_bonus", 400);
    }
    if owner_lower.contains("main") {
        score += 80;
        push_component(&mut components, "owner_main_bonus", 80);
    }
    let name_deeplink = deep_link_signal_score_lower(&name_lower);
    score += name_deeplink;
    push_component(&mut components, "name_deeplink_signal", name_deeplink);
    let owner_deeplink = deep_link_signal_score_lower(&owner_lower);
    score += owner_deeplink;
    push_component(&mut components, "owner_deeplink_signal", owner_deeplink);

    if let Some(uri) = function_library_uri(model, func) {
        let uri_lower = uri.to_ascii_lowercase();
        library_kind = priority_library_kind(&uri_lower);
        if uri_lower.ends_with("/main.dart") || uri_lower.ends_with("main.dart") {
            score += 700;
            push_component(&mut components, "library_main_bonus", 700);
        }
        if uri_lower.contains("generated_plugin_registrant.dart") {
            score += 300;
            push_component(&mut components, "plugin_registrant_bonus", 300);
        }
        if uri_lower.starts_with("package:flutter/") {
            score -= 280;
            push_component(&mut components, "framework_library_penalty", -280);
        } else if uri_lower.starts_with("dart:") {
            score -= 360;
            push_component(&mut components, "stdlib_library_penalty", -360);
            if uri_lower.starts_with("dart:isolate") {
                score -= 420;
                push_component(&mut components, "dart_isolate_library_penalty", -420);
            }
        } else if uri_lower.starts_with("package:") {
            score += 220;
            push_component(&mut components, "package_library_bonus", 220);
            if uri_lower.contains("/src/") {
                score -= 50;
                push_component(&mut components, "package_src_penalty", -50);
            }
            if let Some(pkg) = package_name_from_library_uri(&uri_lower) {
                if preferred_packages.contains(&pkg) {
                    score += 420;
                    push_component(
                        &mut components,
                        format!("preferred_package_bonus:{pkg}"),
                        420,
                    );
                } else if !preferred_packages.is_empty() && pkg != "app" {
                    score -= 220;
                    push_component(
                        &mut components,
                        format!("non_preferred_package_penalty:{pkg}"),
                        -220,
                    );
                }
                if let Some(extra) = app_package_boosts.get(&pkg) {
                    score += *extra;
                    push_component(&mut components, format!("app_package_boost:{pkg}"), *extra);
                }
            }
        }
        let library_deeplink = deep_link_signal_score_lower(&uri_lower);
        score += library_deeplink;
        push_component(&mut components, "library_deeplink_signal", library_deeplink);
    }
    if is_bootstrap_like_name(&name_lower) {
        match library_kind {
            PriorityLibraryKind::App => {
                score += 260;
                push_component(&mut components, "app_bootstrap_context_bonus", 260);
            }
            PriorityLibraryKind::Framework => {
                score -= 280;
                push_component(&mut components, "framework_bootstrap_context_penalty", -280);
            }
            PriorityLibraryKind::Stdlib => {
                score -= 360;
                push_component(&mut components, "stdlib_bootstrap_context_penalty", -360);
            }
            PriorityLibraryKind::Unknown => {}
        }
    }
    if let Some(extra) = target_va_hints.get(&func.code.start_va) {
        score += *extra;
        push_component(&mut components, "pool_target_va_hint", *extra);
    }
    if let Some(extra) = entrypoint_frontier_scores.get(&func.code.start_va) {
        if name_lower.starts_with("closure_") {
            let boosted = *extra / 8;
            score += boosted;
            push_component(&mut components, "entrypoint_frontier_boost", boosted);
        } else {
            score += *extra;
            push_component(&mut components, "entrypoint_frontier_boost", *extra);
        }
        match library_kind {
            PriorityLibraryKind::App => {
                score += 220;
                push_component(&mut components, "app_frontier_context_bonus", 220);
            }
            PriorityLibraryKind::Framework => {
                score -= 180;
                push_component(&mut components, "framework_frontier_context_penalty", -180);
            }
            PriorityLibraryKind::Stdlib => {
                score -= 240;
                push_component(&mut components, "stdlib_frontier_context_penalty", -240);
            }
            PriorityLibraryKind::Unknown => {}
        }
    }
    if stats.call_out_degree > 0 {
        let mut call_bonus = (stats.call_out_degree.min(6) as i32) * 60;
        if func.code.size <= 16 {
            call_bonus /= 2;
        }
        score += call_bonus;
        push_component(
            &mut components,
            format!("call_out_degree_bonus:{}", stats.call_out_degree),
            call_bonus,
        );
    }

    (score, components)
}

struct RankedCandidate<'a> {
    index: usize,
    func: &'a Function,
    owner_class: Option<String>,
    library_uri: Option<String>,
    score: i32,
    out_degree: usize,
    components: Vec<(String, i32)>,
}

impl RankedCandidate<'_> {
    fn entry_va(&self) -> u64 {
        self.func.code.start_va
    }

    fn name_key(&self) -> String {
        self.func.name_text().unwrap_or("").to_ascii_lowercase()
    }

    fn owner_name_key(&self) -> String {
        format!(
            "{}::{}",
            self.owner_class
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase(),
            self.name_key()
        )
    }
}

fn to_breakdown(candidate: &RankedCandidate<'_>) -> FunctionPriorityBreakdown {
    FunctionPriorityBreakdown {
        function_id: u64::from(candidate.func.id.0),
        function_name: candidate.func.name_text().map(str::to_string),
        owner_class: candidate.owner_class.clone(),
        library_uri: candidate.library_uri.clone(),
        entry_va: candidate.entry_va(),
        total_score: candidate.score,
        components: candidate
            .components
            .iter()
            .map(|(name, score)| FunctionPriorityComponent {
                name: name.clone(),
                score: *score,
            })
            .collect(),
    }
}

fn rank_candidates<'a>(
    model: &'a ProgramModel,
    hints: &ProgramHints,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
    preferred_packages: &[String],
) -> Vec<RankedCandidate<'a>> {
    let target_va_hints = build_target_va_priority_hints(model, hints);
    let preferred_package_set = preferred_packages
        .iter()
        .filter_map(|v| normalize_package_filter(v))
        .collect::<HashSet<_>>();
    let app_package_boosts = build_app_package_boosts(model, &preferred_package_set);
    let candidates = model
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            let Some(prefix) = focus_prefix else {
                return true;
            };
            // A focus prefix filters on recovered names only. An unnamed
            // function matches nothing rather than matching a synthesized label.
            f.name_text().is_some_and(|n| n.starts_with(prefix))
                || model.owner_name(f).is_some_and(|o| o.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    let mut name_occurrences: HashMap<String, usize> = HashMap::new();
    for (_, func) in &candidates {
        let Some(name) = func.name_text() else {
            continue;
        };
        *name_occurrences
            .entry(name.to_ascii_lowercase())
            .or_insert(0) += 1;
    }
    let (frontier_scores, call_out_degree) = if max_functions.is_some() {
        let funcs = candidates.iter().map(|(_, f)| *f).collect::<Vec<_>>();
        let adjacency = build_call_adjacency(&funcs, iso_instr, iso_base_va);
        let out_degree = adjacency
            .iter()
            .map(|(entry, callees)| (*entry, callees.len()))
            .collect::<HashMap<_, _>>();
        (
            build_entrypoint_frontier_scores(&funcs, &target_va_hints, &adjacency),
            out_degree,
        )
    } else {
        (HashMap::new(), HashMap::new())
    };

    let mut ranked = candidates
        .into_iter()
        .map(|(index, func)| {
            let owner_class = model.owner_name(func).map(str::to_string);
            let library_uri = function_library_uri(model, func).map(str::to_string);
            let out_degree = call_out_degree
                .get(&func.code.start_va)
                .copied()
                .unwrap_or(0);
            if max_functions.is_some() {
                let (score, components) = function_priority(
                    model,
                    func,
                    &target_va_hints,
                    &app_package_boosts,
                    &preferred_package_set,
                    &frontier_scores,
                    FunctionScoreStats {
                        call_out_degree: out_degree,
                        name_occurrences: func
                            .name_text()
                            .and_then(|n| name_occurrences.get(&n.to_ascii_lowercase()).copied())
                            .unwrap_or(1),
                    },
                );
                RankedCandidate {
                    index,
                    func,
                    owner_class,
                    library_uri,
                    score,
                    out_degree,
                    components,
                }
            } else {
                RankedCandidate {
                    index,
                    func,
                    owner_class,
                    library_uri,
                    score: 0,
                    out_degree: 0,
                    components: Vec::new(),
                }
            }
        })
        .collect::<Vec<_>>();

    if max_functions.is_some() {
        ranked.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(b.func.code.size.cmp(&a.func.code.size))
                .then(b.out_degree.cmp(&a.out_degree))
                .then(a.index.cmp(&b.index))
        });
    }
    ranked
}

fn collect_bootflow_seed_entry_vas(
    ranked: &[RankedCandidate<'_>],
    target_va_bootflow_categories: &HashMap<u64, HashSet<&'static str>>,
) -> Vec<u64> {
    let mut selected = HashSet::new();
    let mut out = Vec::new();

    for category in BOOTFLOW_SEED_CATEGORY_ORDER {
        let Some(candidate) = ranked.iter().find(|candidate| {
            if selected.contains(&candidate.entry_va()) {
                return false;
            }
            target_va_bootflow_categories
                .get(&candidate.entry_va())
                .is_some_and(|categories| categories.contains(category))
        }) else {
            continue;
        };
        selected.insert(candidate.entry_va());
        out.push(candidate.entry_va());
    }

    out
}

pub fn rank_program_functions(
    model: &ProgramModel,
    hints: &ProgramHints,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> Vec<FunctionPriorityBreakdown> {
    rank_candidates(
        model,
        hints,
        iso_instr,
        iso_base_va,
        focus_prefix,
        max_functions,
        &[],
    )
    .iter()
    .map(to_breakdown)
    .collect()
}

pub fn disassemble_program(
    model: &ProgramModel,
    hints: &ProgramHints,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> Vec<FunctionDisassembly> {
    disassemble_program_with_priorities_and_package_hints(
        model,
        hints,
        iso_instr,
        iso_base_va,
        focus_prefix,
        max_functions,
        &[],
        true,
    )
    .0
}

/// The pool geometry a pool reference may be resolved through.
///
/// `None` unless the producer claimed a hardware index space *and* supplied the
/// layout. An ordinal pool resolves to nothing, which is why disassembly prints
/// `poolOff[...]` rather than attaching whichever string happened to sit at that
/// position in the producer's list.
fn resolvable_pool_geometry(model: &ProgramModel) -> Option<PoolGeometry> {
    match model.object_pool.index_space {
        PoolIndexSpace::Hardware => model.object_pool.geometry,
        PoolIndexSpace::Ordinal => None,
    }
}

// Eight parameters, all of them independent inputs a caller genuinely chooses:
// bundling them into a struct would move the same list one level down without
// making any call site clearer.
#[allow(clippy::too_many_arguments)]
pub fn disassemble_program_with_priorities_and_package_hints(
    model: &ProgramModel,
    hints: &ProgramHints,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
    preferred_packages: &[String],
    seed_bootflow_categories: bool,
) -> (Vec<FunctionDisassembly>, Vec<FunctionPriorityBreakdown>) {
    let mut out = Vec::new();
    let mut priorities = Vec::new();
    let cs = build_capstone();
    let pool_geometry = resolvable_pool_geometry(model);
    let ranked = rank_candidates(
        model,
        hints,
        iso_instr,
        iso_base_va,
        focus_prefix,
        max_functions,
        preferred_packages,
    );

    if let Some(max) = max_functions {
        const DIVERSITY_FIRST_PASS_MAX_PER_NAME: usize = 2;
        const DIVERSITY_FIRST_PASS_MAX_PER_OWNER_NAME: usize = 1;

        let bootflow_seed_entry_vas = if seed_bootflow_categories {
            let target_va_bootflow_categories = build_target_va_bootflow_categories(model, hints);
            collect_bootflow_seed_entry_vas(&ranked, &target_va_bootflow_categories)
        } else {
            Vec::new()
        };

        let mut selected_entry_vas = HashSet::new();
        let mut selected_name_counts: HashMap<String, usize> = HashMap::new();
        let mut selected_owner_name_counts: HashMap<String, usize> = HashMap::new();
        let mut deferred = Vec::new();

        for seed_entry_va in bootflow_seed_entry_vas {
            if out.len() >= max {
                break;
            }
            let Some(candidate) = ranked
                .iter()
                .find(|candidate| candidate.entry_va() == seed_entry_va)
            else {
                continue;
            };
            if let Some(d) = decode_function(
                model,
                candidate.func,
                iso_instr,
                iso_base_va,
                cs.as_ref(),
                pool_geometry,
            ) {
                out.push(d);
                priorities.push(to_breakdown(candidate));
                selected_entry_vas.insert(seed_entry_va);
                *selected_name_counts
                    .entry(candidate.name_key())
                    .or_insert(0) += 1;
                *selected_owner_name_counts
                    .entry(candidate.owner_name_key())
                    .or_insert(0) += 1;
            }
        }

        for candidate in ranked {
            if out.len() >= max {
                break;
            }
            if selected_entry_vas.contains(&candidate.entry_va()) {
                continue;
            }
            let name_key = candidate.name_key();
            let owner_name_key = candidate.owner_name_key();
            // Unnamed functions have no name to be diverse about, so the
            // per-name caps do not apply to them.
            let capped = !name_key.is_empty();
            let name_seen = selected_name_counts.get(&name_key).copied().unwrap_or(0);
            let owner_name_seen = selected_owner_name_counts
                .get(&owner_name_key)
                .copied()
                .unwrap_or(0);
            if capped
                && (name_seen >= DIVERSITY_FIRST_PASS_MAX_PER_NAME
                    || owner_name_seen >= DIVERSITY_FIRST_PASS_MAX_PER_OWNER_NAME)
            {
                deferred.push(candidate);
                continue;
            }
            if let Some(d) = decode_function(
                model,
                candidate.func,
                iso_instr,
                iso_base_va,
                cs.as_ref(),
                pool_geometry,
            ) {
                out.push(d);
                priorities.push(to_breakdown(&candidate));
                selected_entry_vas.insert(candidate.entry_va());
                *selected_name_counts.entry(name_key).or_insert(0) += 1;
                *selected_owner_name_counts
                    .entry(owner_name_key)
                    .or_insert(0) += 1;
            }
        }

        for candidate in deferred {
            if out.len() >= max {
                break;
            }
            if selected_entry_vas.contains(&candidate.entry_va()) {
                continue;
            }
            if let Some(d) = decode_function(
                model,
                candidate.func,
                iso_instr,
                iso_base_va,
                cs.as_ref(),
                pool_geometry,
            ) {
                out.push(d);
                priorities.push(to_breakdown(&candidate));
                selected_entry_vas.insert(candidate.entry_va());
            }
        }
    } else {
        for candidate in ranked {
            if let Some(d) = decode_function(
                model,
                candidate.func,
                iso_instr,
                iso_base_va,
                cs.as_ref(),
                pool_geometry,
            ) {
                out.push(d);
                priorities.push(to_breakdown(&candidate));
            }
        }
    }

    (out, priorities)
}

pub fn disassemble_program_with_priorities(
    model: &ProgramModel,
    hints: &ProgramHints,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> (Vec<FunctionDisassembly>, Vec<FunctionPriorityBreakdown>) {
    disassemble_program_with_priorities_and_package_hints(
        model,
        hints,
        iso_instr,
        iso_base_va,
        focus_prefix,
        max_functions,
        &[],
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_adapter::model::{
        Capabilities, CapabilityLevel, Class, ClassId, CodeRange, CompatibilityBinding, Function,
        FunctionId, InputRegion, InputRegionName, Library, LibraryId, Name, ObjectPool,
        ObservedInput, PoolEntry, PoolEntryKind, PoolIndexSpace, Producer, ProducerTrust,
        Provenance, MODEL_VERSION,
    };
    use flutterdec_adapter::primitives::Sha256Digest;
    use flutterdec_loader::identity::{SnapshotIdentity, SnapshotKind, TargetArch};

    /// Fixture builders for v4 models.
    ///
    /// The disassembler never validates a model, so these skip the host-selected
    /// bookkeeping that [`flutterdec_adapter::validate`] checks and populate only
    /// what ranking and decoding read. What they cannot do is invent a name or an
    /// owner: an unnamed function is `fun(id, None, ...)`, which is the case these
    /// tests exist to cover.
    fn lib(id: u32, uri: &str) -> Library {
        Library {
            id: LibraryId(id),
            uri: uri.to_string(),
            display_name: None,
            provenance: Provenance::Exact,
        }
    }

    fn cls(id: u32, name: &str, library: Option<u32>) -> Class {
        Class {
            id: ClassId(id),
            name: name.to_string(),
            library: library.map(LibraryId),
            super_class: None,
            provenance: Provenance::Exact,
        }
    }

    fn fun(id: u32, name: Option<&str>, owner: Option<u32>, start_va: u64, size: u64) -> Function {
        Function {
            id: FunctionId(id),
            name: name.map(Name::exact),
            owner: owner.map(ClassId),
            code: CodeRange { start_va, size },
            code_section_va: start_va,
            provenance: Provenance::Exact,
        }
    }

    fn pool_string(index: u64, value: &str) -> PoolEntry {
        PoolEntry {
            index,
            kind: PoolEntryKind::String,
            value: Some(value.to_string()),
            target_va: None,
            provenance: Provenance::Exact,
            confidence: None,
        }
    }

    fn pool_selector(index: u64, selector: &str, target_va: u64) -> PoolEntry {
        PoolEntry {
            index,
            kind: PoolEntryKind::Selector,
            value: Some(selector.to_string()),
            target_va: Some(target_va),
            provenance: Provenance::Exact,
            confidence: None,
        }
    }

    fn ordinal_pool(entries: Vec<PoolEntry>) -> ObjectPool {
        ObjectPool {
            index_space: PoolIndexSpace::Ordinal,
            geometry: None,
            entries,
        }
    }

    fn hardware_pool(entries: Vec<PoolEntry>) -> ObjectPool {
        ObjectPool {
            index_space: PoolIndexSpace::Hardware,
            geometry: Some(ARM64_POOL_GEOMETRY),
            entries,
        }
    }

    fn hint(
        kind: HintKind,
        origin: HintOrigin,
        selector: &str,
        target_va: Option<u64>,
        owner_class: Option<&str>,
        library_uri: Option<&str>,
    ) -> Hint {
        Hint {
            kind,
            origin,
            provenance: HintProvenance::Heuristic,
            selector: selector.to_string(),
            target_va,
            owner_class: owner_class.map(str::to_string),
            library_uri: library_uri.map(str::to_string),
            detail: String::new(),
        }
    }

    fn program_hints(entries: Vec<Hint>) -> ProgramHints {
        let mut hints = ProgramHints::new();
        for entry in entries {
            hints.push(entry);
        }
        hints
    }

    /// Score one function the way `rank_candidates` would, resolving its owner
    /// and library through the model instead of through a name-keyed side table.
    fn score(
        model: &ProgramModel,
        id: u32,
        frontier: &HashMap<u64, i32>,
        preferred: &HashSet<String>,
    ) -> (i32, Vec<(String, i32)>) {
        let func = model
            .functions
            .iter()
            .find(|f| f.id == FunctionId(id))
            .expect("fixture function");
        function_priority(
            model,
            func,
            &HashMap::new(),
            &HashMap::new(),
            preferred,
            frontier,
            FunctionScoreStats {
                call_out_degree: 0,
                name_occurrences: 1,
            },
        )
    }

    fn test_model(
        libraries: Vec<Library>,
        classes: Vec<Class>,
        functions: Vec<Function>,
        object_pool: ObjectPool,
    ) -> ProgramModel {
        let digest = Sha256Digest::of(b"disasm fixture");
        ProgramModel {
            model_version: MODEL_VERSION,
            producer: Producer {
                id: "disasm-fixture".to_string(),
                version: "0".to_string(),
                artifact_sha256: digest.clone(),
                trust: ProducerTrust::Untrusted,
            },
            input: ObservedInput {
                identity: SnapshotIdentity::from_header(
                    TargetArch::Arm64,
                    "80a49c7111088100a233b2ae788e1f48",
                    SnapshotKind::FullAot,
                    "product arm64 compressed-pointers",
                ),
                regions: vec![InputRegion {
                    region: InputRegionName::IsolateInstructions,
                    size: u64::MAX / 2,
                    sha256: digest.clone(),
                    virtual_address: Some(0),
                    executable: true,
                }],
            },
            compatibility: CompatibilityBinding {
                record_sha256: digest.clone(),
                parser_family_id: "fixture".to_string(),
                profile_id: "fixture".to_string(),
                profile_sha256: digest,
            },
            capabilities: Capabilities {
                libraries: CapabilityLevel::Partial,
                classes: CapabilityLevel::Partial,
                class_relationships: CapabilityLevel::Unavailable,
                functions: CapabilityLevel::Partial,
                function_names: CapabilityLevel::Partial,
                object_pool: CapabilityLevel::Partial,
                pool_index_space: CapabilityLevel::Unavailable,
            },
            libraries,
            classes,
            functions,
            object_pool,
            diagnostics: Vec::new(),
            extensions: Default::default(),
        }
    }

    /// Word encodings lifted from a real Dart 3.9.2 `libapp.so`; ground-truth pool
    /// indices were cross-checked against an independent ObjectPool decoder.
    mod pool_words {
        /// `ldr x1, [x27, #0xef8]`: direct load, PP displacement 0xef8.
        pub const LDR_X1_PP_0XEF8: u32 = 0xF947_7F61;
        /// `add x0, x27, #0x23, lsl #12`: page base at PP + 0x23000.
        pub const ADD_X0_PP_PAGE_0X23: u32 = 0x9140_8F60;
        /// `ldr x0, [x0, #0xa90]`: completes the page pair, displacement 0x23a90.
        pub const LDR_X0_X0_0XA90: u32 = 0xF945_4800;
        pub const RET: u32 = 0xD65F_03C0;
    }

    fn pool_probe_model(pool_geometry: Option<PoolGeometry>) -> ProgramModel {
        let pool = match pool_geometry {
            Some(_) => hardware_pool(Vec::new()),
            None => ordinal_pool(Vec::new()),
        };
        test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![fun(0, Some("poolProbe"), Some(0), 0x1000, 16)],
            pool,
        )
    }

    fn annotations_for_words(
        words: &[u32],
        geometry: Option<PoolGeometry>,
    ) -> Vec<(String, String)> {
        let mut model = pool_probe_model(geometry);
        model.functions[0].code.size = (words.len() * 4) as u64;
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let d = disassemble_program(&model, &ProgramHints::new(), &bytes, 0x1000, None, None);
        d.first()
            .map(|f| {
                f.instructions
                    .iter()
                    .map(|i| (format!("{} {}", i.mnemonic, i.op_str), i.annotation.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    const ARM64_POOL_GEOMETRY: PoolGeometry = PoolGeometry {
        entries_offset: 0x10,
        word_size: 8,
    };

    #[test]
    fn direct_pool_load_resolves_displacement_to_entry_index() {
        let out = annotations_for_words(
            &[pool_words::LDR_X1_PP_0XEF8, pool_words::RET],
            Some(ARM64_POOL_GEOMETRY),
        );
        assert_eq!(
            out[0].0, "ldr x1, [x27, #0xef8]",
            "encoding drifted: {out:?}"
        );
        // 0xef8 is a byte displacement, not an index: (0xef8 - 0x10) / 8 == 477.
        assert_eq!(out[0].1, "pool[477]");
    }

    #[test]
    fn paged_pool_load_resolves_across_the_add_ldr_pair() {
        let out = annotations_for_words(
            &[
                pool_words::ADD_X0_PP_PAGE_0X23,
                pool_words::LDR_X0_X0_0XA90,
                pool_words::RET,
            ],
            Some(ARM64_POOL_GEOMETRY),
        );
        assert_eq!(
            out[0].0, "add x0, x27, #0x23, lsl #12",
            "encoding drifted: {out:?}"
        );
        assert_eq!(
            out[1].0, "ldr x0, [x0, #0xa90]",
            "encoding drifted: {out:?}"
        );
        // (0x23 << 12) + 0xa90 == 0x23a90; (0x23a90 - 0x10) / 8 == 18256.
        assert_eq!(out[1].1, "pool[18256]");
    }

    #[test]
    fn pool_loads_report_raw_displacement_without_geometry() {
        let out = annotations_for_words(
            &[
                pool_words::LDR_X1_PP_0XEF8,
                pool_words::ADD_X0_PP_PAGE_0X23,
                pool_words::LDR_X0_X0_0XA90,
                pool_words::RET,
            ],
            None,
        );
        // No geometry means no index space; emitting `pool[N]` here would invite the
        // hint layer to join on an index that does not exist.
        assert_eq!(out[0].1, "poolOff[3832]");
        assert_eq!(out[2].1, "poolOff[146064]"); // 0x23a90
    }

    #[test]
    fn control_flow_invalidates_a_pending_pool_page_base() {
        let out = annotations_for_words(
            &[
                pool_words::ADD_X0_PP_PAGE_0X23,
                pool_words::RET,
                pool_words::LDR_X0_X0_0XA90,
            ],
            Some(ARM64_POOL_GEOMETRY),
        );
        assert_eq!(
            out[2].1, "",
            "a base that did not survive control flow must not annotate a slot"
        );
    }

    #[test]
    fn redefining_the_base_register_invalidates_it() {
        // `add x0, x27, #0x23, lsl #12` then `add x0, x27, #1, lsl #12` must use the
        // second base, not the first.
        let second_page = 0x9140_0760u32; // add x0, x27, #1, lsl #12
        let out = annotations_for_words(
            &[
                pool_words::ADD_X0_PP_PAGE_0X23,
                second_page,
                pool_words::LDR_X0_X0_0XA90,
                pool_words::RET,
            ],
            Some(ARM64_POOL_GEOMETRY),
        );
        assert_eq!(
            out[1].0, "add x0, x27, #1, lsl #12",
            "encoding drifted: {out:?}"
        );
        // (1 << 12) + 0xa90 == 0x1a90; (0x1a90 - 0x10) / 8 == 848.
        assert_eq!(out[2].1, "pool[848]");
    }
    /// `ldp` writes two registers. Clearing only the first leaves the second holding a
    /// base it no longer has, which a later load would turn into a fabricated slot.
    #[test]
    fn load_pair_invalidates_both_destination_registers() {
        // add x1, x27, #0x23, lsl #12   (x1 gets a page base)
        // ldp x0, x1, [sp, #16]         (x1 is overwritten)
        // ldr x0, [x1, #0xa90]          (must not resolve)
        let add_x1_page = 0x9140_8F61u32;
        let ldp_x0_x1_sp16 = 0xA941_07E0u32;
        let ldr_x0_x1_0xa90 = 0xF945_4820u32;
        let out = annotations_for_words(
            &[
                add_x1_page,
                ldp_x0_x1_sp16,
                ldr_x0_x1_0xa90,
                pool_words::RET,
            ],
            Some(ARM64_POOL_GEOMETRY),
        );
        assert_eq!(
            out[0].0, "add x1, x27, #0x23, lsl #12",
            "encoding drifted: {out:?}"
        );
        assert_eq!(
            out[1].0, "ldp x0, x1, [sp, #0x10]",
            "encoding drifted: {out:?}"
        );
        assert_eq!(
            out[2].0, "ldr x0, [x1, #0xa90]",
            "encoding drifted: {out:?}"
        );
        assert_eq!(
            out[2].1, "",
            "x1 was overwritten by the load pair, so its stale base must not resolve"
        );
    }

    #[test]
    fn disassembles_simple_function() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![fun(0, Some("entry"), Some(0), 0x1000, 8)],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x1000, None, None);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].instructions[0].mnemonic, "ret");
    }

    #[test]
    fn prioritizes_main_like_name_when_max_functions_is_limited() {
        let model = test_model(
            vec![
                lib(0, "package:flutter/src/widgets/binding.dart"),
                lib(1, "package:app/main.dart"),
            ],
            vec![
                cls(0, "WidgetsBinding", Some(0)),
                cls(1, "AppRoot", Some(1)),
            ],
            vec![
                fun(0, None, Some(0), 0x1000, 4),
                fun(1, Some("main"), Some(1), 0x1004, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name.as_deref(), Some("main"));
    }

    #[test]
    fn prioritizes_app_main_library_for_generic_names_when_limited() {
        let model = test_model(
            vec![
                lib(0, "package:flutter/src/widgets/heroes.dart"),
                lib(1, "package:app/main.dart"),
            ],
            vec![
                cls(0, "RenderErrorBox", Some(0)),
                cls(1, "AppRoot", Some(1)),
            ],
            vec![
                fun(0, None, Some(0), 0x2000, 4),
                fun(1, None, Some(1), 0x2004, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x2000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x2004);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_deeplink_and_activity_handler_names_when_limited() {
        let model = test_model(
            vec![lib(0, "package:app/navigation.dart")],
            vec![cls(0, "RouterHost", Some(0))],
            vec![
                fun(0, None, Some(0), 0x3000, 4),
                fun(1, Some("handleIncomingIntent"), Some(0), 0x3004, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x3000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name.as_deref(), Some("handleIncomingIntent"));
    }

    #[test]
    fn prioritizes_pool_target_va_with_deeplink_selector_when_limited() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x4000, 4),
                fun(1, None, Some(0), 0x4004, 4),
            ],
            ordinal_pool(vec![
                pool_selector(0, "onNewIntent", 0x4004),
                pool_string(1, "x"),
            ]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x4000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x4004);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_entrypoint_candidate_target_va_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x5000, 4),
                fun(1, None, Some(0), 0x5004, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![hint(
            HintKind::EntryPoint,
            HintOrigin::ModelNamePattern,
            "main",
            Some(0x5004),
            Some("AppRoot"),
            Some("package:app/main.dart"),
        )]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x5000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x5004);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_boot_main_candidate_target_va_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x50a0, 4),
                fun(1, None, Some(0), 0x50a4, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![hint(
            HintKind::BootMain,
            HintOrigin::ModelNamePattern,
            "main",
            Some(0x50a4),
            Some("AppRoot"),
            Some("package:app/main.dart"),
        )]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x50a0, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x50a4);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_manifest_main_candidate_target_va_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x50aa, 4),
                fun(1, None, Some(0), 0x50ae, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![hint(
            HintKind::BootMain,
            HintOrigin::AndroidManifest,
            "main",
            Some(0x50ae),
            Some("AppRoot"),
            Some("package:app/main.dart"),
        )]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x50aa, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x50ae);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_deeplink_candidate_target_va_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:app/router.dart")],
            vec![cls(0, "RouterHost", Some(0))],
            vec![
                fun(0, None, Some(0), 0x50b0, 4),
                fun(1, None, Some(0), 0x50b4, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![hint(
            HintKind::DeepLinkHandler,
            HintOrigin::ModelNamePattern,
            "onNewIntent",
            Some(0x50b4),
            Some("RouterHost"),
            Some("package:app/router.dart"),
        )]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x50b0, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x50b4);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prefers_app_deeplink_candidate_over_framework_deeplink_candidate() {
        let model = test_model(
            vec![
                lib(0, "package:app/router.dart"),
                lib(1, "package:flutter/src/widgets/app.dart"),
            ],
            vec![
                cls(0, "AppRouterHost", Some(0)),
                cls(1, "WidgetsBindingObserver", Some(1)),
            ],
            vec![
                fun(0, None, Some(0), 0x50c0, 4),
                fun(1, None, Some(1), 0x50c4, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![
            hint(
                HintKind::DeepLinkHandler,
                HintOrigin::ModelNamePattern,
                "onNewIntent",
                Some(0x50c0),
                Some("AppRouterHost"),
                Some("package:app/router.dart"),
            ),
            hint(
                HintKind::DeepLinkHandler,
                HintOrigin::ModelNamePattern,
                "didPushRouteInformation",
                Some(0x50c4),
                Some("WidgetsBindingObserver"),
                Some("package:flutter/src/widgets/app.dart"),
            ),
        ]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x50c0, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x50c0);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn seeds_bootflow_categories_in_capped_selection() {
        let model = test_model(
            vec![lib(0, "package:app/router.dart")],
            vec![cls(0, "RouterHost", Some(0))],
            vec![
                fun(0, None, Some(0), 0x5200, 4),
                fun(1, None, Some(0), 0x5204, 4),
                fun(2, None, Some(0), 0x5208, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![
            hint(
                HintKind::BootMain,
                HintOrigin::ModelNamePattern,
                "main",
                Some(0x5200),
                Some("RouterHost"),
                Some("package:app/router.dart"),
            ),
            hint(
                HintKind::BootMain,
                HintOrigin::ModelNamePattern,
                "main",
                Some(0x5204),
                Some("RouterHost"),
                Some("package:app/router.dart"),
            ),
            hint(
                HintKind::DeepLinkHandler,
                HintOrigin::ModelNamePattern,
                "onNewIntent",
                Some(0x5208),
                Some("RouterHost"),
                Some("package:app/router.dart"),
            ),
        ]);
        let bytes = vec![
            0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6,
        ];
        let d = disassemble_program(&model, &hints, &bytes, 0x5200, None, Some(2));
        assert_eq!(d.len(), 2);
        let selected = d.iter().map(|f| f.entry_va).collect::<HashSet<_>>();
        assert!(
            selected.contains(&0x5208),
            "deeplink bootflow candidate should be seeded into capped output: {selected:?}"
        );
        assert!(
            selected.contains(&0x5200) || selected.contains(&0x5204),
            "a main bootflow candidate should also be seeded: {selected:?}"
        );
    }

    #[test]
    fn can_disable_bootflow_category_seeding() {
        let model = test_model(
            vec![lib(0, "package:app/router.dart")],
            vec![cls(0, "RouterHost", Some(0))],
            vec![
                fun(0, None, Some(0), 0x5300, 4),
                fun(1, None, Some(0), 0x5304, 4),
                fun(2, None, Some(0), 0x5308, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![
            hint(
                HintKind::BootMain,
                HintOrigin::ModelNamePattern,
                "main",
                Some(0x5300),
                Some("RouterHost"),
                Some("package:app/router.dart"),
            ),
            hint(
                HintKind::BootMain,
                HintOrigin::ModelNamePattern,
                "main",
                Some(0x5304),
                Some("RouterHost"),
                Some("package:app/router.dart"),
            ),
            hint(
                HintKind::DeepLinkHandler,
                HintOrigin::ModelNamePattern,
                "onNewIntent",
                Some(0x5308),
                Some("RouterHost"),
                Some("package:app/router.dart"),
            ),
        ]);
        let bytes = vec![
            0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6,
        ];
        let (d, _) = disassemble_program_with_priorities_and_package_hints(
            &model,
            &hints,
            &bytes,
            0x5300,
            None,
            Some(2),
            &[],
            false,
        );
        assert_eq!(d.len(), 2);
        let selected = d.iter().map(|f| f.entry_va).collect::<HashSet<_>>();
        assert!(selected.contains(&0x5300));
        assert!(selected.contains(&0x5304));
        assert!(
            !selected.contains(&0x5308),
            "deeplink target should only be forced when bootflow seeding is enabled: {selected:?}"
        );
    }

    #[test]
    fn prioritizes_lifecycle_selector_target_va_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:spotube/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x5100, 4),
                fun(1, None, Some(0), 0x5104, 4),
            ],
            ordinal_pool(vec![pool_selector(0, "createState", 0x5104)]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &hints, &bytes, 0x5100, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x5104);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_top_app_package_when_names_are_generic() {
        let model = test_model(
            vec![
                lib(0, "package:other_pkg/core.dart"),
                lib(1, "package:app_pkg/feature.dart"),
            ],
            vec![cls(0, "OtherCls", Some(0)), cls(1, "AppCls", Some(1))],
            vec![
                fun(0, None, Some(0), 0x2000, 4),
                fun(1, None, Some(1), 0x2004, 4),
                fun(2, None, Some(1), 0x2008, 4),
                fun(3, None, Some(1), 0x200c, 4),
                fun(4, None, Some(1), 0x2010, 4),
                fun(5, None, Some(1), 0x2014, 4),
                fun(6, None, Some(1), 0x2018, 4),
                fun(7, None, Some(1), 0x201c, 4),
                fun(8, None, Some(1), 0x2020, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(9);
        let d = disassemble_program(&model, &hints, &bytes, 0x2000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].owner_class.as_deref(), Some("AppCls"));
    }

    #[test]
    fn prioritizes_larger_function_when_names_are_generic_and_scores_tie() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x1000, 8),
                fun(1, None, Some(0), 0x1010, 0x100),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(68);
        let d = disassemble_program(&model, &hints, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x1010);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn prioritizes_hub_function_by_call_out_degree_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x1000, 12),
                fun(1, None, Some(0), 0x1010, 4),
                fun(2, None, Some(0), 0x1020, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = vec![
            0x04, 0x00, 0x00, 0x94, // bl #0x1010
            0x07, 0x00, 0x00, 0x94, // bl #0x1020
            0xc0, 0x03, 0x5f, 0xd6, // ret
            0xc0, 0x03, 0x5f, 0xd6, // filler
            0xc0, 0x03, 0x5f, 0xd6, // 0x1010 ret
            0xc0, 0x03, 0x5f, 0xd6, // filler
            0xc0, 0x03, 0x5f, 0xd6, // filler
            0xc0, 0x03, 0x5f, 0xd6, // filler
            0xc0, 0x03, 0x5f, 0xd6, // 0x1020 ret
        ];
        let d = disassemble_program(&model, &hints, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry_va, 0x1000);
        assert_eq!(d[0].function_name, None);
    }

    #[test]
    fn penalizes_repeated_named_functions_for_capped_selection() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, Some("processUpdate"), Some(0), 0x1000, 32),
                fun(1, Some("processUpdate"), Some(0), 0x1020, 32),
                fun(2, Some("processUpdate"), Some(0), 0x1040, 32),
                fun(3, Some("startCLI"), Some(0), 0x1060, 32),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(40);
        let d = disassemble_program(&model, &hints, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name.as_deref(), Some("startCLI"));
    }

    #[test]
    fn penalizes_no_isolate_markers_in_name_or_owner() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![
                cls(0, "AppRoot", Some(0)),
                cls(1, "AppRoot no isolate", Some(0)),
            ],
            vec![
                fun(0, None, Some(0), 0x6000, 32),
                fun(1, None, Some(1), 0x6010, 32),
            ],
            ordinal_pool(Vec::new()),
        );
        let (clean_score, clean_components) = score(&model, 0, &HashMap::new(), &HashSet::new());
        let (noisy_score, noisy_components) = score(&model, 1, &HashMap::new(), &HashSet::new());

        assert!(
            noisy_components
                .iter()
                .any(|(name, score)| name == "no_isolate_marker_penalty" && *score == -650),
            "no-isolate marker should add an explicit penalty component: {noisy_components:?}"
        );
        assert!(
            clean_components
                .iter()
                .all(|(name, _)| name != "no_isolate_marker_penalty"),
            "clean function should not get no-isolate penalty: {clean_components:?}"
        );
        assert!(
            noisy_score < clean_score,
            "no-isolate marker should lower score (clean={clean_score}, noisy={noisy_score})"
        );
    }

    #[test]
    fn penalizes_dart_isolate_library_more_than_generic_stdlib() {
        let model = test_model(
            vec![
                lib(0, "dart:isolate-patch/isolate_patch.dart"),
                lib(1, "dart:core-patch/core_patch.dart"),
            ],
            vec![
                cls(0, "IsolateWorker", Some(0)),
                cls(1, "CoreWorker", Some(1)),
            ],
            vec![
                fun(0, None, Some(0), 0x6100, 32),
                fun(1, None, Some(1), 0x6110, 32),
            ],
            ordinal_pool(Vec::new()),
        );
        let (isolate_score, isolate_components) =
            score(&model, 0, &HashMap::new(), &HashSet::new());
        let (core_score, core_components) = score(&model, 1, &HashMap::new(), &HashSet::new());

        assert!(
            isolate_components
                .iter()
                .any(|(name, score)| name == "dart_isolate_library_penalty" && *score == -420),
            "dart:isolate library should add isolate-specific penalty: {isolate_components:?}"
        );
        assert!(
            core_components
                .iter()
                .all(|(name, _)| name != "dart_isolate_library_penalty"),
            "non-isolate stdlib function should not get isolate-specific penalty: {core_components:?}"
        );
        assert!(
            isolate_score < core_score,
            "dart:isolate functions should rank below other stdlib functions when tied (isolate={isolate_score}, core={core_score})"
        );
    }

    #[test]
    fn preferred_package_bonus_beats_non_preferred_package_for_generic_names() {
        let model = test_model(
            vec![
                lib(0, "package:spotube/main.dart"),
                lib(1, "package:provider/src/provider.dart"),
            ],
            vec![
                cls(0, "SpotubeCore", Some(0)),
                cls(1, "ProviderCore", Some(1)),
            ],
            vec![
                fun(0, None, Some(0), 0x7100, 64),
                fun(1, None, Some(1), 0x7200, 64),
            ],
            ordinal_pool(Vec::new()),
        );
        let preferred = HashSet::from(["spotube".to_string()]);
        let (preferred_score, preferred_components) = score(&model, 0, &HashMap::new(), &preferred);
        let (dep_score, dep_components) = score(&model, 1, &HashMap::new(), &preferred);

        assert!(
            preferred_components
                .iter()
                .any(|(name, score)| name == "preferred_package_bonus:spotube" && *score == 420),
            "preferred package bonus should be applied: {preferred_components:?}"
        );
        assert!(
            dep_components.iter().any(|(name, score)| name
                == "non_preferred_package_penalty:provider"
                && *score == -220),
            "non-preferred package penalty should be applied: {dep_components:?}"
        );
        assert!(
            preferred_score > dep_score,
            "preferred package should outrank dependency package (preferred={preferred_score}, dep={dep_score})"
        );
    }

    #[test]
    fn app_bootstrap_context_outranks_framework_bootstrap_context() {
        let model = test_model(
            vec![
                lib(0, "package:app/main.dart"),
                lib(1, "package:flutter/src/widgets/binding.dart"),
            ],
            vec![
                cls(0, "AppBootstrap", Some(0)),
                cls(1, "WidgetsFlutterBinding", Some(1)),
            ],
            vec![
                fun(0, Some("ensureInitialized"), Some(0), 0x7300, 64),
                fun(1, Some("ensureInitialized"), Some(1), 0x7310, 64),
            ],
            ordinal_pool(Vec::new()),
        );
        let (app_score, app_components) = score(&model, 0, &HashMap::new(), &HashSet::new());
        let (framework_score, framework_components) =
            score(&model, 1, &HashMap::new(), &HashSet::new());

        assert!(app_components
            .iter()
            .any(|(name, score)| { name == "app_bootstrap_context_bonus" && *score == 260 }));
        assert!(framework_components.iter().any(|(name, score)| {
            name == "framework_bootstrap_context_penalty" && *score == -280
        }));
        assert!(
            app_score > framework_score,
            "app-owned bootstrap function should outrank framework bootstrap helper (app={app_score}, framework={framework_score})"
        );
    }

    #[test]
    fn app_frontier_context_outranks_framework_frontier_context() {
        let model = test_model(
            vec![
                lib(0, "package:app/main.dart"),
                lib(1, "package:flutter/src/widgets/app.dart"),
            ],
            vec![cls(0, "AppRoot", Some(0)), cls(1, "FrameworkRoot", Some(1))],
            vec![
                fun(0, None, Some(0), 0x7400, 64),
                fun(1, None, Some(1), 0x7410, 64),
            ],
            ordinal_pool(Vec::new()),
        );
        let frontier = HashMap::from([(0x7400, 900), (0x7410, 900)]);
        let (app_score, app_components) = score(&model, 0, &frontier, &HashSet::new());
        let (framework_score, framework_components) = score(&model, 1, &frontier, &HashSet::new());

        assert!(app_components
            .iter()
            .any(|(name, score)| { name == "app_frontier_context_bonus" && *score == 220 }));
        assert!(framework_components.iter().any(|(name, score)| {
            name == "framework_frontier_context_penalty" && *score == -180
        }));
        assert!(
            app_score > framework_score,
            "entrypoint-frontier app code should outrank frontier framework code (app={app_score}, framework={framework_score})"
        );
    }

    #[test]
    fn preferred_package_boost_overrides_count_only_package_ranking() {
        let mut functions = Vec::new();
        for i in 0..20u32 {
            functions.push(fun(
                i,
                Some(&format!("dep_{i}")),
                Some(1),
                0x8000 + u64::from(i) * 4,
                32,
            ));
        }
        for i in 0..3u32 {
            functions.push(fun(
                100 + i,
                Some(&format!("app_{i}")),
                Some(0),
                0x9000 + u64::from(i) * 4,
                32,
            ));
        }
        let model = test_model(
            vec![
                lib(0, "package:spotube/main.dart"),
                lib(1, "package:provider/src/provider.dart"),
            ],
            vec![cls(0, "AppCore", Some(0)), cls(1, "ProviderCore", Some(1))],
            functions,
            ordinal_pool(Vec::new()),
        );
        let preferred = HashSet::from(["spotube".to_string()]);
        let boosts = build_app_package_boosts(&model, &preferred);
        let provider = boosts.get("provider").copied().unwrap_or(0);
        let spotube = boosts.get("spotube").copied().unwrap_or(0);
        assert!(
            spotube > provider,
            "preferred package should dominate count-only ranking (spotube={spotube}, provider={provider})"
        );
    }

    #[test]
    fn prioritizes_entrypoint_frontier_callee_when_names_are_generic() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, None, Some(0), 0x1000, 4),
                fun(1, None, Some(0), 0x1004, 4),
                fun(2, None, Some(0), 0x1008, 4),
            ],
            ordinal_pool(vec![]),
        );
        let hints = program_hints(vec![hint(
            HintKind::EntryPoint,
            HintOrigin::ModelNamePattern,
            "main",
            Some(0x1000),
            Some("AppRoot"),
            Some("package:app/main.dart"),
        )]);
        let bytes = vec![
            0x02, 0x00, 0x00, 0x94, // bl #0x1008
            0xc0, 0x03, 0x5f, 0xd6, // ret
            0xc0, 0x03, 0x5f, 0xd6, // ret
        ];
        let d = disassemble_program(&model, &hints, &bytes, 0x1000, None, Some(2));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].entry_va, 0x1000);
        assert_eq!(d[0].function_name, None);
        assert_eq!(d[1].entry_va, 0x1008);
        assert_eq!(d[1].function_name, None);
    }

    #[test]
    fn capped_selection_prefers_diversity_before_duplicate_owner_name() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, Some("main"), Some(0), 0x7000, 4),
                fun(1, Some("main"), Some(0), 0x7004, 4),
                fun(2, Some("startCLI"), Some(0), 0x7008, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(3);
        let d = disassemble_program(&model, &hints, &bytes, 0x7000, None, Some(2));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].function_name.as_deref(), Some("main"));
        assert_eq!(d[1].function_name.as_deref(), Some("startCLI"));
    }

    #[test]
    fn capped_selection_backfills_deferred_duplicates_when_needed() {
        let model = test_model(
            vec![lib(0, "package:app/main.dart")],
            vec![cls(0, "AppRoot", Some(0))],
            vec![
                fun(0, Some("main"), Some(0), 0x7100, 4),
                fun(1, Some("main"), Some(0), 0x7104, 4),
            ],
            ordinal_pool(vec![pool_string(0, "x")]),
        );
        let hints = program_hints(vec![]);
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(2);
        let d = disassemble_program(&model, &hints, &bytes, 0x7100, None, Some(2));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].function_name.as_deref(), Some("main"));
        assert_eq!(d[1].function_name.as_deref(), Some("main"));
    }
}
