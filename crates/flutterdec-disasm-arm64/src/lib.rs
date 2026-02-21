use capstone::arch::arm64::ArchMode;
use capstone::prelude::*;
use flutterdec_adapter::{FunctionInfo, ProgramModel};
use regex::Regex;
use serde::Serialize;

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

pub fn disassemble_program(
    model: &ProgramModel,
    iso_instr: &[u8],
    iso_base_va: u64,
    focus_prefix: Option<&str>,
    max_functions: Option<usize>,
) -> Vec<FunctionDisassembly> {
    let mut out = Vec::new();
    let cs = build_capstone();

    for f in &model.functions {
        if let Some(prefix) = focus_prefix {
            if !f.name.starts_with(prefix) && !f.owner_class.starts_with(prefix) {
                continue;
            }
        }

        if let Some(d) = decode_function(f, iso_instr, iso_base_va, cs.as_ref()) {
            out.push(d);
            if let Some(max) = max_functions {
                if out.len() >= max {
                    break;
                }
            }
        }
    }

    out
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
}
