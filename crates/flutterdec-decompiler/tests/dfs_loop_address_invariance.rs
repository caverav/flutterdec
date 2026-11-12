//! DFS loop classification is a graph relation, never an address relation.
//!
//! Each topology below is rebuilt with ascending, descending, and permuted
//! block addresses. The public direct-DFS surface must return the same complete
//! artifact after immutable block addresses in accounting are mapped back to
//! block ids. The ordinary public surface is checked too, including the
//! structured decline on irreducible input.

use flutterdec_decompiler::{emit_pseudocode, emit_pseudocode_direct_dfs, PseudocodeArtifact};
use flutterdec_ir::{rebuild_edges, BasicBlock, FunctionIr, IROp, LlirInstr};
use serde_json::Value;
use std::collections::HashMap;

const ASCENDING: &[u64] = &[0x1000, 0x1100, 0x1200, 0x1300, 0x1400, 0x1500, 0x1600];
const DESCENDING: &[u64] = &[0x7000, 0x6000, 0x5000, 0x4000, 0x3000, 0x2000, 0x1000];
// In every loop topology, entry block 0 and latch block 2 or 3 are below the
// higher-address header block 1. The old address predicate therefore sees no
// back edge at all.
const PERMUTED: &[u64] = &[0x1000, 0x7000, 0x2000, 0x3000, 0x6000, 0x4000, 0x5000];

struct Topology {
    name: &'static str,
    succs: &'static [&'static [usize]],
    loops: usize,
    breaks: usize,
    continues: usize,
    follow: Option<usize>,
    irreducible: bool,
}

const SIMPLE: Topology = Topology {
    name: "simple_loop",
    succs: &[&[1], &[3, 2], &[1], &[]],
    loops: 1,
    breaks: 0,
    continues: 1,
    follow: Some(3),
    irreducible: false,
};

const NESTED: Topology = Topology {
    name: "nested_loop",
    succs: &[&[1], &[2], &[4, 3], &[2], &[5, 1], &[]],
    loops: 1,
    breaks: 1,
    continues: 1,
    follow: Some(5),
    irreducible: false,
};

const MULTI_EXIT: Topology = Topology {
    name: "multi_exit_loop",
    succs: &[&[1], &[2, 4], &[3, 5], &[1], &[6], &[6], &[]],
    loops: 1,
    breaks: 0,
    continues: 1,
    follow: Some(6),
    irreducible: false,
};

const IRREDUCIBLE: Topology = Topology {
    name: "irreducible_cycle",
    succs: &[&[1, 2], &[2, 3], &[1, 3], &[]],
    loops: 0,
    breaks: 0,
    continues: 0,
    follow: None,
    irreducible: true,
};

fn marker_va(id: usize) -> u64 {
    0x90000 + 0x10 * id as u64
}

fn instruction(va: u64, op: IROp, src: String, target: String) -> LlirInstr {
    LlirInstr {
        va,
        op,
        src,
        target,
    }
}

fn fixture(topology: &Topology, addresses: &[u64]) -> (FunctionIr, HashMap<u64, String>) {
    assert!(addresses.len() >= topology.succs.len());
    let blocks = topology
        .succs
        .iter()
        .enumerate()
        .map(|(id, succs)| {
            let start = addresses[id];
            let mut instrs = vec![instruction(
                start,
                IROp::Call,
                format!("bl #{:#x}", marker_va(id)),
                format!("#{:#x}", marker_va(id)),
            )];
            match *succs {
                [] => instrs.push(instruction(
                    start + 4,
                    IROp::Return,
                    "ret".to_string(),
                    String::new(),
                )),
                [only] => instrs.push(instruction(
                    start + 4,
                    IROp::Jump,
                    format!("b #{:#x}", addresses[*only]),
                    format!("#{:#x}", addresses[*only]),
                )),
                [_fallthrough, taken] => instrs.push(instruction(
                    start + 4,
                    IROp::Branch,
                    format!("cbz x0, #{:#x}", addresses[*taken]),
                    format!("#{:#x}", addresses[*taken]),
                )),
                _ => panic!("{} has an unsupported successor count", topology.name),
            }
            BasicBlock {
                id,
                start_va: start,
                instrs,
                succs: succs.to_vec(),
                preds: Vec::new(),
            }
        })
        .collect();
    let mut ir = FunctionIr {
        function_id: 9500,
        name: topology.name.to_string(),
        entry_va: addresses[0],
        blocks,
    };
    rebuild_edges(&mut ir.blocks);
    let names = (0..topology.succs.len())
        .map(|id| (marker_va(id), format!("mark{id}")))
        .collect();
    (ir, names)
}

fn normalize_addresses(value: &mut Value, addresses: &[u64]) {
    match value {
        Value::Number(number) => {
            if let Some(address) = number.as_u64() {
                if let Some(id) = addresses.iter().position(|candidate| *candidate == address) {
                    *value = Value::from(id as u64);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_addresses(value, addresses);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                normalize_addresses(value, addresses);
            }
        }
        _ => {}
    }
}

fn exact_artifact(artifact: &PseudocodeArtifact, addresses: &[u64]) -> Value {
    let mut value = serde_json::to_value(artifact).expect("artifact serializes");
    normalize_addresses(&mut value, addresses);
    value
}

fn statement_count(source: &str, statement: &str) -> usize {
    source
        .lines()
        .filter(|line| line.trim() == statement)
        .count()
}

fn assert_shape(topology: &Topology, artifact: &PseudocodeArtifact) {
    assert_eq!(
        source_count(&artifact.source, "while (true) {"),
        topology.loops,
        "{} loop count:\n{}",
        topology.name,
        artifact.source
    );
    assert_eq!(
        statement_count(&artifact.source, "break;"),
        topology.breaks,
        "{} break count:\n{}",
        topology.name,
        artifact.source
    );
    assert_eq!(
        statement_count(&artifact.source, "continue;"),
        topology.continues,
        "{} continue count:\n{}",
        topology.name,
        artifact.source
    );
    if let Some(follow) = topology.follow {
        let loop_end = artifact
            .source
            .find("\n  }\n")
            .expect("the fallback loop has a closing brace");
        let follow_marker = artifact
            .source
            .rfind(&format!("mark{follow}()"))
            .expect("the loop follow is emitted");
        assert!(
            follow_marker < loop_end,
            "{} keeps its exact fallback follow placement:\n{}",
            topology.name,
            artifact.source
        );
    }
    artifact
        .emission
        .validate()
        .unwrap_or_else(|error| panic!("{} accounting: {error}", topology.name));
}

fn source_count(source: &str, needle: &str) -> usize {
    source.matches(needle).count()
}

#[test]
fn public_dfs_loop_artifacts_ignore_block_address_order() {
    for topology in [&SIMPLE, &NESTED, &MULTI_EXIT, &IRREDUCIBLE] {
        let (reference_ir, reference_names) = fixture(topology, ASCENDING);
        let reference = emit_pseudocode_direct_dfs(&reference_ir, &reference_names);
        assert_shape(topology, &reference);
        let reference = exact_artifact(&reference, ASCENDING);

        for (layout, addresses) in [("descending", DESCENDING), ("permuted", PERMUTED)] {
            let (ir, names) = fixture(topology, addresses);
            let artifact = emit_pseudocode_direct_dfs(&ir, &names);
            assert_shape(topology, &artifact);
            assert_eq!(
                exact_artifact(&artifact, addresses),
                reference,
                "{} changed under the {layout} address layout",
                topology.name
            );
        }
    }
}

#[test]
fn public_auto_emission_preserves_isomorphic_meaning_and_declines_irreducible_input() {
    for topology in [&SIMPLE, &NESTED, &MULTI_EXIT, &IRREDUCIBLE] {
        let mut reference = None;
        for addresses in [ASCENDING, DESCENDING, PERMUTED] {
            let (ir, names) = fixture(topology, addresses);
            let artifact = emit_pseudocode(&ir, &names);
            artifact
                .emission
                .validate()
                .unwrap_or_else(|error| panic!("{} accounting: {error}", topology.name));
            assert_eq!(
                artifact.emission.decline().is_some(),
                topology.irreducible,
                "{} took the wrong public emission path:\n{}",
                topology.name,
                artifact.source
            );
            let artifact = exact_artifact(&artifact, addresses);
            if let Some(reference) = &reference {
                assert_eq!(
                    &artifact, reference,
                    "{} auto output changed under an address permutation",
                    topology.name
                );
            } else {
                reference = Some(artifact);
            }
        }
    }
}
