use capstone::arch::arm64::ArchMode;
use capstone::prelude::*;
use flutterdec_adapter::{FunctionInfo, ProgramModel};
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

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
    pub function_name: String,
    pub owner_class: String,
    pub entry_va: u64,
    pub size: u64,
    pub instructions: Vec<AsmInstruction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionPriorityComponent {
    pub name: String,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionPriorityBreakdown {
    pub function_id: u64,
    pub function_name: String,
    pub owner_class: String,
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

fn maybe_pool_annotation(mnemonic: &str, op_str: &str) -> Option<String> {
    if mnemonic != "ldr" {
        return None;
    }
    let lower = op_str.to_ascii_lowercase();
    if !lower.contains("[x27") {
        return None;
    }
    let re = Regex::new(r"\[x27,\s*#?(0x[0-9a-fA-F]+|[0-9]+)\]").ok()?;
    let caps = re.captures(op_str)?;
    let raw = caps.get(1)?.as_str();
    let imm = if let Some(hex) = raw.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()?
    } else {
        raw.parse::<u64>().ok()?
    };
    Some(format!("pool[{imm}]"))
}

fn annotation_for(mnemonic: &str, op_str: &str) -> String {
    if mnemonic == "bl" || mnemonic == "blr" {
        return "call".to_string();
    }
    if mnemonic == "ret" {
        return "return".to_string();
    }
    if mnemonic == "b" {
        return "jump".to_string();
    }
    if mnemonic.starts_with("b.")
        || mnemonic == "cbz"
        || mnemonic == "cbnz"
        || mnemonic == "tbz"
        || mnemonic == "tbnz"
    {
        return "branch".to_string();
    }
    if let Some(pp) = maybe_pool_annotation(mnemonic, op_str) {
        return pp;
    }
    String::new()
}

fn decode_function(
    func: &FunctionInfo,
    iso_instr: &[u8],
    iso_base_va: u64,
    cs: Option<&Capstone>,
) -> Option<FunctionDisassembly> {
    if func.entry_va < iso_base_va {
        return None;
    }
    let rel = (func.entry_va - iso_base_va) as usize;
    if rel >= iso_instr.len() {
        return None;
    }

    let requested = usize::try_from(func.size).unwrap_or(0);
    let size = requested.min(iso_instr.len() - rel);
    if size < 4 {
        return None;
    }

    let code = &iso_instr[rel..rel + size];
    let mut instructions = Vec::new();

    if let Some(cs) = cs {
        if let Ok(insns) = cs.disasm_all(code, func.entry_va) {
            for ins in insns.iter() {
                let bytes = ins.bytes();
                let word = if bytes.len() >= 4 {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    0
                };
                let mnemonic = ins.mnemonic().unwrap_or("word").to_string();
                let op_str = ins.op_str().unwrap_or("").to_string();
                let annotation = annotation_for(&mnemonic, &op_str);
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
            let pc = func.entry_va + off as u64;
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
        function_id: func.id,
        function_name: func.name.clone(),
        owner_class: func.owner_class.clone(),
        entry_va: func.entry_va,
        size: size as u64,
        instructions,
    })
}

fn build_owner_library_lookup(model: &ProgramModel) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for class in &model.classes {
        out.entry(class.name.clone())
            .or_insert_with(|| class.library_uri.clone());
    }
    out
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

fn entrypoint_signal_score(entry: &flutterdec_adapter::ObjectPoolEntry) -> i32 {
    let mut score = 0i32;
    let decoded_kind_lower = entry
        .decoded_kind
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    let library_lower = entry
        .library_uri
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();

    score += match decoded_kind_lower.as_str() {
        "entrypointcandidate" => 5000,
        "bootmaincandidate" => 6200,
        "bootrunappcandidate" => 3800,
        "deeplinkhandlercandidate" => 2600,
        "activityhandlercandidate" => 2400,
        "bootstrapinitcandidate" => 1800,
        "manifestmaincandidate" => 5400,
        "manifestrunappcandidate" => 3200,
        "manifestdeeplinkcandidate" => 2100,
        "manifestactivitycandidate" => 1900,
        "manifestbootstrapcandidate" => 1400,
        _ => 0,
    };

    let value_lower = entry.value.trim().to_ascii_lowercase();
    if value_lower.starts_with("entrypoint:") {
        score += 1800;
    } else if value_lower.starts_with("bootflow:main:") {
        score += 2200;
    } else if value_lower.starts_with("bootflow:runapp:") {
        score += 1200;
    } else if value_lower.starts_with("bootflow:deeplink:") {
        score += 900;
    } else if value_lower.starts_with("bootflow:activity:") {
        score += 800;
    } else if value_lower.starts_with("bootflow:init:") {
        score += 700;
    } else if value_lower.starts_with("manifest:main") {
        score += 1800;
    } else if value_lower.starts_with("manifest:runapp") {
        score += 1000;
    } else if value_lower.starts_with("manifest:deeplink") {
        score += 700;
    } else if value_lower.starts_with("manifest:activity") {
        score += 650;
    } else if value_lower.starts_with("manifest:bootstrap") {
        score += 520;
    }

    if let Some(selector) = entry.selector.as_deref() {
        let selector_lower = selector.trim().to_ascii_lowercase();
        if is_main_like_name(&selector_lower) {
            score += 3200;
        } else if selector_lower == "runapp" || selector_lower.ends_with(".runapp") {
            score += 500;
        }
    }

    // Framework/stdlib metadata is useful for graph seeding, but app-owned
    // handlers should dominate capped reverse-engineering output.
    let framework_weighted_kind = matches!(
        decoded_kind_lower.as_str(),
        "deeplinkhandlercandidate"
            | "activityhandlercandidate"
            | "bootstrapinitcandidate"
            | "manifestdeeplinkcandidate"
            | "manifestactivitycandidate"
            | "manifestbootstrapcandidate"
    );
    if framework_weighted_kind && library_lower.starts_with("package:flutter/") {
        score /= 4;
    } else if framework_weighted_kind && library_lower.starts_with("dart:") {
        score /= 5;
    }

    score
}

fn selector_signal_score(entry: &flutterdec_adapter::ObjectPoolEntry) -> i32 {
    let selector = entry
        .selector
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_lowercase());
    let Some(selector) = selector else {
        return 0;
    };

    let mut score = match selector.as_str() {
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
    if score == 0 {
        return 0;
    }

    let lib = entry
        .library_uri
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    if lib.starts_with("package:flutter/") {
        score /= 4;
    } else if lib.starts_with("dart:") {
        score /= 5;
    }
    score
}

fn build_target_va_priority_hints(model: &ProgramModel) -> HashMap<u64, i32> {
    let mut out = HashMap::new();
    for entry in &model.object_pool {
        let Some(target_va) = entry.target_va else {
            continue;
        };
        let mut score = 0i32;
        score += entrypoint_signal_score(entry);
        score += selector_signal_score(entry);
        score += deep_link_signal_score(&entry.value);
        score += entry
            .decoded_kind
            .as_deref()
            .map(deep_link_signal_score)
            .unwrap_or(0);
        score += entry
            .selector
            .as_deref()
            .map(deep_link_signal_score)
            .unwrap_or(0);
        score += entry
            .owner_class
            .as_deref()
            .map(deep_link_signal_score)
            .unwrap_or(0);
        score += entry
            .library_uri
            .as_deref()
            .map(deep_link_signal_score)
            .unwrap_or(0);
        if score <= 0 {
            continue;
        }
        let clamped = score.min(6000);
        let slot = out.entry(target_va).or_insert(0);
        if clamped > *slot {
            *slot = clamped;
        }
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

fn build_app_package_boosts(
    model: &ProgramModel,
    owner_library: &HashMap<String, String>,
) -> HashMap<String, i32> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &model.functions {
        let Some(uri) = owner_library.get(&f.owner_class) else {
            continue;
        };
        let Some(pkg) = package_name_from_library_uri(uri) else {
            continue;
        };
        *counts.entry(pkg).or_insert(0) += 1;
    }

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
    out
}

fn collect_direct_call_targets(
    func: &FunctionInfo,
    iso_instr: &[u8],
    iso_base_va: u64,
    known_entries: &HashSet<u64>,
) -> Vec<u64> {
    if func.entry_va < iso_base_va {
        return Vec::new();
    }
    let rel = (func.entry_va - iso_base_va) as usize;
    if rel >= iso_instr.len() {
        return Vec::new();
    }
    let requested = usize::try_from(func.size).unwrap_or(0);
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
        let pc = func.entry_va + off as u64;
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
    candidates: &[&FunctionInfo],
    iso_instr: &[u8],
    iso_base_va: u64,
) -> HashMap<u64, Vec<u64>> {
    let known_entries: HashSet<u64> = candidates.iter().map(|f| f.entry_va).collect();
    if known_entries.is_empty() {
        return HashMap::new();
    }

    let mut adjacency: HashMap<u64, Vec<u64>> = HashMap::new();
    for f in candidates {
        let callees = collect_direct_call_targets(f, iso_instr, iso_base_va, &known_entries);
        if !callees.is_empty() {
            adjacency.insert(f.entry_va, callees);
        }
    }
    adjacency
}

fn build_entrypoint_frontier_scores(
    candidates: &[&FunctionInfo],
    target_va_hints: &HashMap<u64, i32>,
    adjacency: &HashMap<u64, Vec<u64>>,
) -> HashMap<u64, i32> {
    let mut seeds = HashSet::new();
    for f in candidates {
        let name_lower = f.name.to_ascii_lowercase();
        if is_main_like_name(&name_lower)
            || name_lower.contains("runapp")
            || name_lower.contains("ensureinitialized")
            || target_va_hints.get(&f.entry_va).copied().unwrap_or(0) >= 1200
        {
            seeds.insert(f.entry_va);
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

fn function_priority(
    func: &FunctionInfo,
    owner_library: &HashMap<String, String>,
    target_va_hints: &HashMap<u64, i32>,
    app_package_boosts: &HashMap<String, i32>,
    entrypoint_frontier_scores: &HashMap<u64, i32>,
    call_out_degree: usize,
    name_occurrences: usize,
) -> (i32, Vec<(String, i32)>) {
    let mut score = 0i32;
    let mut components = Vec::new();
    let name_lower = func.name.to_ascii_lowercase();
    let owner_lower = func.owner_class.to_ascii_lowercase();

    if has_no_isolate_marker(&name_lower) || has_no_isolate_marker(&owner_lower) {
        score -= 650;
        push_component(&mut components, "no_isolate_marker_penalty", -650);
    }

    if looks_generic_name(&name_lower) {
        score -= 40;
        push_component(&mut components, "generic_name_penalty", -40);
    } else {
        score += 10;
        push_component(&mut components, "named_function_bonus", 10);
        if name_occurrences > 1 {
            let repeated_name_penalty = (name_occurrences.saturating_sub(1).min(10) as i32) * 240;
            score -= repeated_name_penalty;
            push_component(
                &mut components,
                format!("repeated_name_penalty:{name_occurrences}"),
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
    if func.size <= 16 && looks_generic_name(&name_lower) {
        score -= 80;
        push_component(&mut components, "tiny_generic_penalty", -80);
    }
    if func.size <= 8 && (name_lower.starts_with("closure_") || name_lower.starts_with('_')) {
        score -= 220;
        push_component(&mut components, "tiny_wrapper_penalty", -220);
    }
    let size_bonus = function_size_bonus(func.size);
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

    if let Some(uri) = owner_library.get(&func.owner_class) {
        let uri_lower = uri.to_ascii_lowercase();
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
    if let Some(extra) = target_va_hints.get(&func.entry_va) {
        score += *extra;
        push_component(&mut components, "pool_target_va_hint", *extra);
    }
    if let Some(extra) = entrypoint_frontier_scores.get(&func.entry_va) {
        if name_lower.starts_with("closure_") {
            let boosted = *extra / 8;
            score += boosted;
            push_component(&mut components, "entrypoint_frontier_boost", boosted);
        } else {
            score += *extra;
            push_component(&mut components, "entrypoint_frontier_boost", *extra);
        }
    }
    if call_out_degree > 0 {
        let mut call_bonus = (call_out_degree.min(6) as i32) * 60;
        if func.size <= 16 {
            call_bonus /= 2;
        }
        score += call_bonus;
        push_component(
            &mut components,
            format!("call_out_degree_bonus:{call_out_degree}"),
            call_bonus,
        );
    }

    (score, components)
}

struct RankedCandidate<'a> {
    index: usize,
    func: &'a FunctionInfo,
    score: i32,
    out_degree: usize,
    components: Vec<(String, i32)>,
}

fn to_breakdown(candidate: &RankedCandidate<'_>) -> FunctionPriorityBreakdown {
    FunctionPriorityBreakdown {
        function_id: candidate.func.id,
        function_name: candidate.func.name.clone(),
        owner_class: candidate.func.owner_class.clone(),
        entry_va: candidate.func.entry_va,
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
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> Vec<RankedCandidate<'a>> {
    let owner_library = build_owner_library_lookup(model);
    let target_va_hints = build_target_va_priority_hints(model);
    let app_package_boosts = build_app_package_boosts(model, &owner_library);
    let candidates = model
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            if let Some(prefix) = focus_prefix {
                f.name.starts_with(prefix) || f.owner_class.starts_with(prefix)
            } else {
                true
            }
        })
        .collect::<Vec<_>>();
    let mut name_occurrences: HashMap<String, usize> = HashMap::new();
    for (_, func) in &candidates {
        *name_occurrences
            .entry(func.name.to_ascii_lowercase())
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
            if max_functions.is_some() {
                let (score, components) = function_priority(
                    func,
                    &owner_library,
                    &target_va_hints,
                    &app_package_boosts,
                    &frontier_scores,
                    call_out_degree.get(&func.entry_va).copied().unwrap_or(0),
                    name_occurrences
                        .get(&func.name.to_ascii_lowercase())
                        .copied()
                        .unwrap_or(1),
                );
                RankedCandidate {
                    index,
                    func,
                    score,
                    out_degree: call_out_degree.get(&func.entry_va).copied().unwrap_or(0),
                    components,
                }
            } else {
                RankedCandidate {
                    index,
                    func,
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
                .then(b.func.size.cmp(&a.func.size))
                .then(b.out_degree.cmp(&a.out_degree))
                .then(a.index.cmp(&b.index))
        });
    }
    ranked
}

pub fn rank_program_functions(
    model: &ProgramModel,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> Vec<FunctionPriorityBreakdown> {
    rank_candidates(model, iso_instr, iso_base_va, focus_prefix, max_functions)
        .iter()
        .map(to_breakdown)
        .collect()
}

pub fn disassemble_program(
    model: &ProgramModel,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> Vec<FunctionDisassembly> {
    disassemble_program_with_priorities(model, iso_instr, iso_base_va, focus_prefix, max_functions)
        .0
}

pub fn disassemble_program_with_priorities(
    model: &ProgramModel,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> (Vec<FunctionDisassembly>, Vec<FunctionPriorityBreakdown>) {
    let mut out = Vec::new();
    let mut priorities = Vec::new();
    let cs = build_capstone();
    let ranked = rank_candidates(model, iso_instr, iso_base_va, focus_prefix, max_functions);

    if let Some(max) = max_functions {
        const DIVERSITY_FIRST_PASS_MAX_PER_NAME: usize = 2;
        const DIVERSITY_FIRST_PASS_MAX_PER_OWNER_NAME: usize = 1;

        let mut selected_name_counts: HashMap<String, usize> = HashMap::new();
        let mut selected_owner_name_counts: HashMap<String, usize> = HashMap::new();
        let mut deferred = Vec::new();

        for candidate in ranked {
            if out.len() >= max {
                break;
            }
            let name_key = candidate.func.name.to_ascii_lowercase();
            let owner_name_key = format!(
                "{}::{}",
                candidate.func.owner_class.to_ascii_lowercase(),
                name_key
            );
            let name_seen = selected_name_counts.get(&name_key).copied().unwrap_or(0);
            let owner_name_seen = selected_owner_name_counts
                .get(&owner_name_key)
                .copied()
                .unwrap_or(0);
            if name_seen >= DIVERSITY_FIRST_PASS_MAX_PER_NAME
                || owner_name_seen >= DIVERSITY_FIRST_PASS_MAX_PER_OWNER_NAME
            {
                deferred.push(candidate);
                continue;
            }
            if let Some(d) = decode_function(candidate.func, iso_instr, iso_base_va, cs.as_ref()) {
                out.push(d);
                priorities.push(to_breakdown(&candidate));
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
            if let Some(d) = decode_function(candidate.func, iso_instr, iso_base_va, cs.as_ref()) {
                out.push(d);
                priorities.push(to_breakdown(&candidate));
            }
        }
    } else {
        for candidate in ranked {
            if let Some(d) = decode_function(candidate.func, iso_instr, iso_base_va, cs.as_ref()) {
                out.push(d);
                priorities.push(to_breakdown(&candidate));
            }
        }
    }

    (out, priorities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flutterdec_adapter::{ClassInfo, LibraryInfo, ObjectPoolEntry};

    #[test]
    fn disassembles_simple_function() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![FunctionInfo {
                id: 0,
                name: "entry".to_string(),
                owner_class: "Global".to_string(),
                entry_va: 0x1000,
                size: 8,
                code_section_va: 0x1000,
            }],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x1000, None, None);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].instructions[0].mnemonic, "ret");
    }

    #[test]
    fn prioritizes_main_like_name_when_max_functions_is_limited() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![
                LibraryInfo {
                    id: 0,
                    uri: "package:flutter/src/widgets/binding.dart".to_string(),
                    name_display: "package:flutter/src/widgets/binding.dart".to_string(),
                },
                LibraryInfo {
                    id: 1,
                    uri: "package:app/main.dart".to_string(),
                    name_display: "package:app/main.dart".to_string(),
                },
            ],
            classes: vec![
                ClassInfo {
                    id: 0,
                    name: "WidgetsBinding".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:flutter/src/widgets/binding.dart".to_string(),
                },
                ClassInfo {
                    id: 1,
                    name: "Global".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
            ],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_1000".to_string(),
                    owner_class: "WidgetsBinding".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 1,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1004,
                    size: 4,
                    code_section_va: 0x1000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "main");
    }

    #[test]
    fn prioritizes_app_main_library_for_generic_names_when_limited() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![
                LibraryInfo {
                    id: 0,
                    uri: "package:flutter/src/widgets/heroes.dart".to_string(),
                    name_display: "package:flutter/src/widgets/heroes.dart".to_string(),
                },
                LibraryInfo {
                    id: 1,
                    uri: "package:app/main.dart".to_string(),
                    name_display: "package:app/main.dart".to_string(),
                },
            ],
            classes: vec![
                ClassInfo {
                    id: 0,
                    name: "RenderErrorBox".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:flutter/src/widgets/heroes.dart".to_string(),
                },
                ClassInfo {
                    id: 1,
                    name: "Global".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/main.dart".to_string(),
                },
            ],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_a000".to_string(),
                    owner_class: "RenderErrorBox".to_string(),
                    entry_va: 0x2000,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_b000".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x2004,
                    size: 4,
                    code_section_va: 0x2000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x2000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_b000");
    }

    #[test]
    fn prioritizes_deeplink_and_activity_handler_names_when_limited() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/navigation.dart".to_string(),
                name_display: "package:app/navigation.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "RouterHost".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/navigation.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_3000".to_string(),
                    owner_class: "RouterHost".to_string(),
                    entry_va: 0x3000,
                    size: 4,
                    code_section_va: 0x3000,
                },
                FunctionInfo {
                    id: 1,
                    name: "handleIncomingIntent".to_string(),
                    owner_class: "RouterHost".to_string(),
                    entry_va: 0x3004,
                    size: 4,
                    code_section_va: 0x3000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x3000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "handleIncomingIntent");
    }

    #[test]
    fn prioritizes_pool_target_va_with_deeplink_selector_when_limited() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_4000".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x4000,
                    size: 4,
                    code_section_va: 0x4000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_4004".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x4004,
                    size: 4,
                    code_section_va: 0x4000,
                },
            ],
            object_pool: vec![
                ObjectPoolEntry {
                    index: 0,
                    kind: "String".to_string(),
                    value: "android.intent.action.VIEW".to_string(),
                    decoded_kind: Some("selector".to_string()),
                    selector: Some("onNewIntent".to_string()),
                    target_va: Some(0x4004),
                    owner_class: Some("MainActivity".to_string()),
                    library_uri: Some("package:app/main.dart".to_string()),
                },
                ObjectPoolEntry {
                    index: 1,
                    kind: "String".to_string(),
                    value: "x".to_string(),
                    decoded_kind: None,
                    selector: None,
                    target_va: None,
                    owner_class: None,
                    library_uri: None,
                },
            ],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x4000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_4004");
    }

    #[test]
    fn prioritizes_entrypoint_candidate_target_va_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_5000".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x5000,
                    size: 4,
                    code_section_va: 0x5000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_5004".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x5004,
                    size: 4,
                    code_section_va: 0x5000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "entrypoint:main".to_string(),
                decoded_kind: Some("EntryPointCandidate".to_string()),
                selector: Some("main".to_string()),
                target_va: Some(0x5004),
                owner_class: Some("Global".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x5000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_5004");
    }

    #[test]
    fn prioritizes_boot_main_candidate_target_va_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_50a0".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x50a0,
                    size: 4,
                    code_section_va: 0x50a0,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_50a4".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x50a4,
                    size: 4,
                    code_section_va: 0x50a0,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "bootflow:main:main".to_string(),
                decoded_kind: Some("BootMainCandidate".to_string()),
                selector: Some("main".to_string()),
                target_va: Some(0x50a4),
                owner_class: Some("Global".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x50a0, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_50a4");
    }

    #[test]
    fn prioritizes_manifest_main_candidate_target_va_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_50aa".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x50aa,
                    size: 4,
                    code_section_va: 0x50aa,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_50ae".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x50ae,
                    size: 4,
                    code_section_va: 0x50aa,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "manifest:main-launcher".to_string(),
                decoded_kind: Some("ManifestMainCandidate".to_string()),
                selector: Some("main".to_string()),
                target_va: Some(0x50ae),
                owner_class: Some("Global".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x50aa, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_50ae");
    }

    #[test]
    fn prioritizes_deeplink_candidate_target_va_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/router.dart".to_string(),
                name_display: "package:app/router.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "RouterHost".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/router.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_50b0".to_string(),
                    owner_class: "RouterHost".to_string(),
                    entry_va: 0x50b0,
                    size: 4,
                    code_section_va: 0x50b0,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_50b4".to_string(),
                    owner_class: "RouterHost".to_string(),
                    entry_va: 0x50b4,
                    size: 4,
                    code_section_va: 0x50b0,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "bootflow:deeplink:onNewIntent".to_string(),
                decoded_kind: Some("DeepLinkHandlerCandidate".to_string()),
                selector: Some("onNewIntent".to_string()),
                target_va: Some(0x50b4),
                owner_class: Some("RouterHost".to_string()),
                library_uri: Some("package:app/router.dart".to_string()),
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x50b0, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_50b4");
    }

    #[test]
    fn prefers_app_deeplink_candidate_over_framework_deeplink_candidate() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![
                LibraryInfo {
                    id: 0,
                    uri: "package:app/router.dart".to_string(),
                    name_display: "package:app/router.dart".to_string(),
                },
                LibraryInfo {
                    id: 1,
                    uri: "package:flutter/src/widgets/app.dart".to_string(),
                    name_display: "package:flutter/src/widgets/app.dart".to_string(),
                },
            ],
            classes: vec![
                ClassInfo {
                    id: 0,
                    name: "AppRouterHost".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app/router.dart".to_string(),
                },
                ClassInfo {
                    id: 1,
                    name: "WidgetsBindingObserver".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:flutter/src/widgets/app.dart".to_string(),
                },
            ],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_50c0".to_string(),
                    owner_class: "AppRouterHost".to_string(),
                    entry_va: 0x50c0,
                    size: 4,
                    code_section_va: 0x50c0,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_50c4".to_string(),
                    owner_class: "WidgetsBindingObserver".to_string(),
                    entry_va: 0x50c4,
                    size: 4,
                    code_section_va: 0x50c0,
                },
            ],
            object_pool: vec![
                ObjectPoolEntry {
                    index: 0,
                    kind: "String".to_string(),
                    value: "bootflow:deeplink:onNewIntent".to_string(),
                    decoded_kind: Some("DeepLinkHandlerCandidate".to_string()),
                    selector: Some("onNewIntent".to_string()),
                    target_va: Some(0x50c0),
                    owner_class: Some("AppRouterHost".to_string()),
                    library_uri: Some("package:app/router.dart".to_string()),
                },
                ObjectPoolEntry {
                    index: 1,
                    kind: "String".to_string(),
                    value: "bootflow:deeplink:didPushRouteInformation".to_string(),
                    decoded_kind: Some("DeepLinkHandlerCandidate".to_string()),
                    selector: Some("didPushRouteInformation".to_string()),
                    target_va: Some(0x50c4),
                    owner_class: Some("WidgetsBindingObserver".to_string()),
                    library_uri: Some("package:flutter/src/widgets/app.dart".to_string()),
                },
            ],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x50c0, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_50c0");
    }

    #[test]
    fn prioritizes_lifecycle_selector_target_va_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:spotube/main.dart".to_string(),
                name_display: "package:spotube/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:spotube/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_5100".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x5100,
                    size: 4,
                    code_section_va: 0x5100,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_5104".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x5104,
                    size: 4,
                    code_section_va: 0x5100,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "createState".to_string(),
                decoded_kind: Some("BlutterUnlinkedCall".to_string()),
                selector: Some("createState".to_string()),
                target_va: Some(0x5104),
                owner_class: Some("MyApp".to_string()),
                library_uri: Some("package:spotube/main.dart".to_string()),
            }],
        };
        let bytes = vec![0xc0, 0x03, 0x5f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6];
        let d = disassemble_program(&model, &bytes, 0x5100, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_5104");
    }

    #[test]
    fn prioritizes_top_app_package_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![
                LibraryInfo {
                    id: 0,
                    uri: "package:other_pkg/core.dart".to_string(),
                    name_display: "package:other_pkg/core.dart".to_string(),
                },
                LibraryInfo {
                    id: 1,
                    uri: "package:app_pkg/feature.dart".to_string(),
                    name_display: "package:app_pkg/feature.dart".to_string(),
                },
            ],
            classes: vec![
                ClassInfo {
                    id: 0,
                    name: "OtherCls".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:other_pkg/core.dart".to_string(),
                },
                ClassInfo {
                    id: 1,
                    name: "AppCls".to_string(),
                    super_name: "Object".to_string(),
                    library_uri: "package:app_pkg/feature.dart".to_string(),
                },
            ],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_2000".to_string(),
                    owner_class: "OtherCls".to_string(),
                    entry_va: 0x2000,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_2004".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x2004,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 2,
                    name: "sub_2008".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x2008,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 3,
                    name: "sub_200c".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x200c,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 4,
                    name: "sub_2010".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x2010,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 5,
                    name: "sub_2014".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x2014,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 6,
                    name: "sub_2018".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x2018,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 7,
                    name: "sub_201c".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x201c,
                    size: 4,
                    code_section_va: 0x2000,
                },
                FunctionInfo {
                    id: 8,
                    name: "sub_2020".to_string(),
                    owner_class: "AppCls".to_string(),
                    entry_va: 0x2020,
                    size: 4,
                    code_section_va: 0x2000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(9);
        let d = disassemble_program(&model, &bytes, 0x2000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].owner_class, "AppCls");
    }

    #[test]
    fn prioritizes_larger_function_when_names_are_generic_and_scores_tie() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_1000".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 8,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_1010".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1010,
                    size: 0x100,
                    code_section_va: 0x1000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(68);
        let d = disassemble_program(&model, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_1010");
    }

    #[test]
    fn prioritizes_hub_function_by_call_out_degree_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_1000".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 12,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_1010".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1010,
                    size: 4,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 2,
                    name: "sub_1020".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1020,
                    size: 4,
                    code_section_va: 0x1000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
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
        let d = disassemble_program(&model, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "sub_1000");
    }

    #[test]
    fn penalizes_repeated_named_functions_for_capped_selection() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "processUpdate".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 32,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 1,
                    name: "processUpdate".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1020,
                    size: 32,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 2,
                    name: "processUpdate".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1040,
                    size: 32,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 3,
                    name: "startCLI".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1060,
                    size: 32,
                    code_section_va: 0x1000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(40);
        let d = disassemble_program(&model, &bytes, 0x1000, None, Some(1));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].function_name, "startCLI");
    }

    #[test]
    fn penalizes_no_isolate_markers_in_name_or_owner() {
        let clean = FunctionInfo {
            id: 0,
            name: "sub_6000".to_string(),
            owner_class: "Global".to_string(),
            entry_va: 0x6000,
            size: 32,
            code_section_va: 0x6000,
        };
        let noisy = FunctionInfo {
            id: 1,
            name: "sub_6010".to_string(),
            owner_class: "Global no isolate".to_string(),
            entry_va: 0x6010,
            size: 32,
            code_section_va: 0x6000,
        };

        let mut owner_library = HashMap::new();
        owner_library.insert("Global".to_string(), "package:app/main.dart".to_string());
        owner_library.insert(
            "Global no isolate".to_string(),
            "package:app/main.dart".to_string(),
        );

        let (clean_score, clean_components) = function_priority(
            &clean,
            &owner_library,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            0,
            1,
        );
        let (noisy_score, noisy_components) = function_priority(
            &noisy,
            &owner_library,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            0,
            1,
        );

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
        let isolate_func = FunctionInfo {
            id: 0,
            name: "sub_6100".to_string(),
            owner_class: "IsolateWorker".to_string(),
            entry_va: 0x6100,
            size: 32,
            code_section_va: 0x6100,
        };
        let core_func = FunctionInfo {
            id: 1,
            name: "sub_6110".to_string(),
            owner_class: "CoreWorker".to_string(),
            entry_va: 0x6110,
            size: 32,
            code_section_va: 0x6100,
        };

        let mut owner_library = HashMap::new();
        owner_library.insert(
            "IsolateWorker".to_string(),
            "dart:isolate-patch/isolate_patch.dart".to_string(),
        );
        owner_library.insert(
            "CoreWorker".to_string(),
            "dart:core-patch/core_patch.dart".to_string(),
        );

        let (isolate_score, isolate_components) = function_priority(
            &isolate_func,
            &owner_library,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            0,
            1,
        );
        let (core_score, core_components) = function_priority(
            &core_func,
            &owner_library,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            0,
            1,
        );

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
    fn prioritizes_entrypoint_frontier_callee_when_names_are_generic() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "sub_1000".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1000,
                    size: 4,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 1,
                    name: "sub_1004".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1004,
                    size: 4,
                    code_section_va: 0x1000,
                },
                FunctionInfo {
                    id: 2,
                    name: "sub_1008".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x1008,
                    size: 4,
                    code_section_va: 0x1000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "entrypoint:main".to_string(),
                decoded_kind: Some("EntryPointCandidate".to_string()),
                selector: Some("main".to_string()),
                target_va: Some(0x1000),
                owner_class: Some("Global".to_string()),
                library_uri: Some("package:app/main.dart".to_string()),
            }],
        };
        let bytes = vec![
            0x02, 0x00, 0x00, 0x94, // bl #0x1008
            0xc0, 0x03, 0x5f, 0xd6, // ret
            0xc0, 0x03, 0x5f, 0xd6, // ret
        ];
        let d = disassemble_program(&model, &bytes, 0x1000, None, Some(2));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].function_name, "sub_1000");
        assert_eq!(d[1].function_name, "sub_1008");
    }

    #[test]
    fn capped_selection_prefers_diversity_before_duplicate_owner_name() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x7000,
                    size: 4,
                    code_section_va: 0x7000,
                },
                FunctionInfo {
                    id: 1,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x7004,
                    size: 4,
                    code_section_va: 0x7000,
                },
                FunctionInfo {
                    id: 2,
                    name: "startCLI".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x7008,
                    size: 4,
                    code_section_va: 0x7000,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(3);
        let d = disassemble_program(&model, &bytes, 0x7000, None, Some(2));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].function_name, "main");
        assert_eq!(d[1].function_name, "startCLI");
    }

    #[test]
    fn capped_selection_backfills_deferred_duplicates_when_needed() {
        let model = ProgramModel {
            schema_version: 2,
            adapter_kind: "test".to_string(),
            dart_version: "unknown".to_string(),
            snapshot_hash: "h".to_string(),
            arch: "arm64".to_string(),
            libraries: vec![LibraryInfo {
                id: 0,
                uri: "package:app/main.dart".to_string(),
                name_display: "package:app/main.dart".to_string(),
            }],
            classes: vec![ClassInfo {
                id: 0,
                name: "Global".to_string(),
                super_name: "Object".to_string(),
                library_uri: "package:app/main.dart".to_string(),
            }],
            functions: vec![
                FunctionInfo {
                    id: 0,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x7100,
                    size: 4,
                    code_section_va: 0x7100,
                },
                FunctionInfo {
                    id: 1,
                    name: "main".to_string(),
                    owner_class: "Global".to_string(),
                    entry_va: 0x7104,
                    size: 4,
                    code_section_va: 0x7100,
                },
            ],
            object_pool: vec![ObjectPoolEntry {
                index: 0,
                kind: "String".to_string(),
                value: "x".to_string(),
                decoded_kind: None,
                selector: None,
                target_va: None,
                owner_class: None,
                library_uri: None,
            }],
        };
        let bytes = [0xc0u8, 0x03, 0x5f, 0xd6].repeat(2);
        let d = disassemble_program(&model, &bytes, 0x7100, None, Some(2));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].function_name, "main");
        assert_eq!(d[1].function_name, "main");
    }
}
