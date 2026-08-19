//! The pre-call provenance audit, end to end, plus the demonstration that its
//! checker detects a real violation.
//!
//! This lives outside the unit tests on purpose. The audit path is read once per
//! process, so a test that sets it has to own the process; a `#[test]` inside the
//! library would race whichever unit test emitted first and silently observe no
//! audit at all.
//!
//! Only one test here emits: the audit file is append-only and shared, so two
//! tests writing it concurrently would each see the other's records. The loader
//! guard below is safe to sit alongside it because it emits nothing, sets no
//! environment variable, and only reads the source tree.

use flutterdec_decompiler::{
    emit_program_with_runtime_stubs, RuntimeStubEffect, PRE_CALL_ANNOTATION,
};
use flutterdec_ir::{BasicBlock, FunctionIr, IROp, LlirInstr};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::Command;

fn stmt(va: u64, src: &str) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Other,
        src: src.to_string(),
        target: String::new(),
    }
}

fn call_to(va: u64, target: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Call,
        src: format!("bl #0x{target:x}"),
        target: format!("#0x{target:x}"),
    }
}

fn ret(va: u64) -> LlirInstr {
    LlirInstr {
        va,
        op: IROp::Return,
        src: "ret".to_string(),
        target: String::new(),
    }
}

/// Two calls, each clobbering x9 while it holds a different value, and an
/// unresolved read after each. Two annotations, two snapshots, and the values
/// are distinguishable, so a record citing the wrong snapshot is visible rather
/// than merely wrong.
fn fixture() -> FunctionIr {
    FunctionIr {
        function_id: 0x4242,
        name: "auditedClobber".to_string(),
        entry_va: 0x1000,
        blocks: vec![BasicBlock {
            id: 0,
            start_va: 0x1000,
            instrs: vec![
                stmt(0x1000, "ldur x20, [x2, #15]"),
                stmt(0x1004, "ldur x9, [x1, #7]"),
                call_to(0x1008, 0x9000),
                stmt(0x100c, "stur x9, [x19, #7]"),
                stmt(0x1010, "ldur x9, [x20, #7]"),
                stmt(0x1014, "stur x9, [x23, #7]"),
                call_to(0x1018, 0x9000),
                stmt(0x101c, "stur x9, [x24, #7]"),
                ret(0x1020),
            ],
            succs: Vec::new(),
            preds: Vec::new(),
        }],
    }
}

fn checker() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/check-annotation-provenance.py")
}

/// A crude field read, adequate because every value here is emitter-generated
/// and contains no escape or brace.
fn field<'a>(row: &'a str, name: &str) -> &'a str {
    let key = format!("\"{name}\":");
    let start = row.find(&key).expect("field present") + key.len();
    let rest = &row[start..];
    if let Some(quoted) = rest.strip_prefix('"') {
        &quoted[..quoted.find('"').expect("terminated string")]
    } else {
        let end = rest.find([',', '}']).expect("terminated value");
        &rest[..end]
    }
}

const PROTOCOL: &str = "docs/oracle-protocol-ir-cfg-emitter.md";
/// The two lanes that must name the integration test targets explicitly: the
/// local parity script and the GitHub job, which runs only a subset of it.
const CI_LANES: [&str; 2] = ["scripts/ci-check.sh", ".github/workflows/ci.yml"];
/// The compiled-inventory checker, which is the correctness oracle for whether a
/// protected file is compiled at all. It has to be reached from a real lane, or
/// it protects nothing.
const INVENTORY_CHECKER: &str = "scripts/check-oracle-inventory.py";
const DECOMPILER_MANIFEST: &str = "crates/flutterdec-decompiler/Cargo.toml";
const CORE_MANIFEST: &str = "crates/flutterdec-core/Cargo.toml";
const IR_MANIFEST: &str = "crates/flutterdec-ir/Cargo.toml";
const DECOMPILER_LOADER: &str = "crates/flutterdec-decompiler/src/tests.rs";
/// The control-flow loader. Unlike the four loaders under `src/tests/`, this one
/// also pulls in five product modules, so its `include!` count is not a fixed
/// number this guard can pin: adding a control-flow module is ordinary work.
const CONTROL_FLOW_LOADER: &str = "crates/flutterdec-decompiler/src/control_flow.rs";

/// The anchor sentence that opens the protocol's Oracle test files table. The
/// guard parses that one table and no other: the other section 7 tables are
/// goldens, scripts, and fixtures, which no loader pulls into a test target.
const ORACLE_TABLE_ANCHOR: &str =
    "Oracle test files. Adding a case to one of these is expected work;";

/// How a protected oracle file reaches a compiled test target. Nothing else does:
/// a protected file with no live hook is dead weight whose digest still matches.
enum Hook {
    /// A `#[cfg(test)]` module declaration, needed verbatim in `file`.
    Module {
        file: &'static str,
        decl: &'static str,
    },
    /// An `include!` of this path, relative to `loader`'s own directory.
    ///
    /// `exclusive` says whether `loader` holds nothing but protected oracle
    /// includes. When it does, the total `include!` count is pinned below, so the
    /// loader cannot grow an oracle section 7 does not record. When it does not -
    /// `control_flow.rs` loads five product modules beside its one oracle - that
    /// count is ordinary work and pinning it would fire on legitimate change.
    Include {
        loader: &'static str,
        exclusive: bool,
    },
    /// Cargo's automatic integration-test discovery for `manifest`'s crate, which
    /// `autotests = false` would switch off for every file under `tests/`.
    Autotest { manifest: &'static str },
}

/// Every row of the protocol's Oracle test files table, with the hook that
/// compiles it. Kept in this guard rather than in the protocol table because the
/// hooks live in product source and manifests, which later work must edit, so a
/// whole-file digest for any of them would fire on legitimate change.
fn loader_map() -> Vec<(String, Hook)> {
    let mut map = vec![
        (
            DECOMPILER_LOADER.to_string(),
            Hook::Module {
                file: "crates/flutterdec-decompiler/src/lib.rs",
                decl: "#[cfg(test)]\nmod tests;",
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/provenance_audit.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/loop_entry_provenance_audit.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-core/src/pipeline/runners/tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-core/src/pipeline/runners.rs",
                decl: "#[cfg(test)]\n#[path = \"runners/tests.rs\"]\nmod runners_tests;",
            },
        ),
        (
            "crates/flutterdec-core/src/pipeline/symbol_map/tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-core/src/pipeline/symbol_map.rs",
                decl: "#[cfg(test)]\n#[path = \"symbol_map/tests.rs\"]\nmod tests;",
            },
        ),
        (
            "crates/flutterdec-decompiler/src/control_flow/emission_taxonomy_tests.rs"
                .to_string(),
            Hook::Module {
                file: "crates/flutterdec-decompiler/src/control_flow/emission_taxonomy.rs",
                decl: "#[cfg(test)]\n#[path = \"emission_taxonomy_tests.rs\"]\nmod emission_taxonomy_tests;",
            },
        ),
        (
            "crates/flutterdec-decompiler/src/control_flow/annotation_anchor_tests.rs"
                .to_string(),
            Hook::Module {
                file: "crates/flutterdec-decompiler/src/control_flow/structured.rs",
                decl: "#[cfg(test)]\n#[path = \"annotation_anchor_tests.rs\"]\nmod annotation_anchor_tests;",
            },
        ),
        (
            "crates/flutterdec-decompiler/src/line_identity_tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-decompiler/src/lib.rs",
                decl: "#[cfg(test)]\nmod line_identity_tests;",
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/helper_syntax_boundaries.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/rewrite_boundaries.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/unmodelled_write_effects.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/register_width_provenance.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/atomic_rmw_effects.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/annotation_anchor_identity.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/provenance_accounting.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-core/tests/pipeline_determinism.rs".to_string(),
            Hook::Autotest {
                manifest: CORE_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-ir/tests/branch_target_radix.rs".to_string(),
            Hook::Autotest {
                manifest: IR_MANIFEST,
            },
        ),
        // The IR and CFG boundary oracles. Each was an inline module in the
        // product file beside it until it was moved out: a digest can only
        // protect a file later work is not expected to edit, and `lib.rs`,
        // `validate.rs`, `quality.rs`, `split.rs`, `stubs.rs` and `regions.rs`
        // are all edited by ordinary work.
        (
            "crates/flutterdec-ir/src/tests/control_effects.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-ir/src/lib.rs",
                decl: "#[cfg(test)]\n#[path = \"tests/control_effects.rs\"]\nmod control_effect_tests;",
            },
        ),
        (
            "crates/flutterdec-ir/src/validate/tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-ir/src/validate.rs",
                decl: "#[cfg(test)]\n#[path = \"validate/tests.rs\"]\nmod tests;",
            },
        ),
        (
            "crates/flutterdec-core/src/pipeline/quality/control_effect_tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-core/src/pipeline/quality.rs",
                decl: "#[cfg(test)]\n#[path = \"quality/control_effect_tests.rs\"]\nmod quality_control_effect_tests;",
            },
        ),
        (
            "crates/flutterdec-core/src/pipeline/runners/split/identity_tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-core/src/pipeline/runners/split.rs",
                decl: "#[cfg(test)]\n#[path = \"split/identity_tests.rs\"]\nmod split_identity_tests;",
            },
        ),
        (
            "crates/flutterdec-core/src/pipeline/runners/stubs/identity_tests.rs".to_string(),
            Hook::Module {
                file: "crates/flutterdec-core/src/pipeline/runners/stubs.rs",
                decl: "#[cfg(test)]\n#[path = \"stubs/identity_tests.rs\"]\nmod stubs_identity_tests;",
            },
        ),
        (
            "crates/flutterdec-decompiler/src/control_flow/regions/identity_boundary_tests.rs"
                .to_string(),
            Hook::Module {
                file: "crates/flutterdec-decompiler/src/control_flow/regions.rs",
                decl: "#[cfg(test)]\n#[path = \"regions/identity_boundary_tests.rs\"]\nmod identity_boundary_tests;",
            },
        ),
        // The CFG relation oracle is loaded the way the control-flow product
        // modules are, by an `include!` in a loader that is not exclusively
        // oracle includes.
        (
            "crates/flutterdec-decompiler/src/control_flow/relation_oracle.rs".to_string(),
            Hook::Include {
                loader: CONTROL_FLOW_LOADER,
                exclusive: false,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/arm64_control_effects.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/cfg_identity.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/dfs_loop_address_invariance.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/entry_loop_state_merge.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
        (
            "crates/flutterdec-decompiler/tests/block_ledger_contract.rs".to_string(),
            Hook::Autotest {
                manifest: DECOMPILER_MANIFEST,
            },
        ),
    ];

    // Loader file, then every protected file it includes. A loader's include
    // directory is its own path without the extension, so the expected
    // `include!` text is derived rather than repeated.
    let nested: [(&'static str, &[&str]); 4] = [
        (
            DECOMPILER_LOADER,
            &[
                "shared.rs",
                "emit_and_helpers.rs",
                "cfg_and_stack.rs",
                "compaction_and_aliasing.rs",
                "golden_and_parser.rs",
            ],
        ),
        (
            "crates/flutterdec-decompiler/src/tests/cfg_and_stack.rs",
            &[
                "annotation_caps.rs",
                "call_and_loops.rs",
                "call_annotations.rs",
                "omitted_path_and_stack.rs",
                "dispatch_table.rs",
                "join_capture.rs",
                "order_totality.rs",
                "structuring.rs",
            ],
        ),
        (
            "crates/flutterdec-decompiler/src/tests/compaction_and_aliasing.rs",
            &["control_flow_compaction.rs", "alias_and_expr_cleanup.rs"],
        ),
        (
            "crates/flutterdec-decompiler/src/tests/emit_and_helpers.rs",
            &[
                "annotation_literals.rs",
                "candidate_whitelist.rs",
                "helper_inlining.rs",
                "readability_and_naming.rs",
            ],
        ),
    ];
    for (loader, included) in nested {
        let dir = loader
            .strip_suffix(".rs")
            .expect("a loader is a Rust source file");
        for file in included {
            map.push((
                format!("{dir}/{file}"),
                Hook::Include {
                    loader,
                    exclusive: true,
                },
            ));
        }
    }
    map
}

/// First ancestor holding the protocol, so the guard reads the workspace the same
/// way from a crate directory, the workspace root, or a worktree copy.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|dir| dir.join(PROTOCOL).is_file())
        .unwrap_or_else(|| panic!("an ancestor of this crate must hold {PROTOCOL}"))
        .to_path_buf()
}

/// The backticked paths of exactly the Oracle test files table: everything from
/// its anchor sentence to the end of section 7.
fn oracle_test_file_rows(protocol: &str) -> Vec<String> {
    let after = protocol
        .split_once(ORACLE_TABLE_ANCHOR)
        .unwrap_or_else(|| {
            panic!("{PROTOCOL} must keep the Oracle test files table anchor verbatim")
        })
        .1;
    let section = after
        .split("\n## ")
        .next()
        .expect("splitting always yields a first part");
    section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            line.split('`')
                .nth(1)
                .expect("a row's path is backticked")
                .to_string()
        })
        .collect()
}

/// Every protected oracle file needs a live hook into a compiled test target, and
/// every hook needs a protected file. The hooks are `#[cfg(test)]` module
/// lines, nineteen `include!` lines across four exclusive loaders under
/// `src/tests/`, one more `include!` in `src/control_flow.rs`, which loads five
/// product modules beside it, module declarations across `flutterdec-ir`,
/// `flutterdec-core` and the decompiler's control-flow code, and Cargo's
/// automatic discovery of the fourteen integration
/// tests. Delete any one of them and the affected test binary still prints
/// `test result: ok`, with fewer tests and a whole protected oracle silenced
/// while its digest still matches.
///
/// What this test does *not* do is decide whether a hook is live by looking at
/// its text. It cannot: `/* /* */`, a leading `//`, `#[cfg(any())]`, a feature
/// that no manifest declares, or a macro that swallows its argument all leave the
/// hook's bytes exactly where they were while removing the item from compilation.
/// The hook text is reported here as a diagnostic and nothing more. The
/// correctness oracle is `scripts/check-oracle-inventory.py`, which asks the
/// compiler: it lists each protected target's tests and requires a sentinel that
/// exists only if the file was compiled. This test asserts that the checker is
/// wired into a real lane, so it cannot be quietly dropped.
///
/// The map is compared against the protocol's Oracle test files table in both
/// directions, so a new protected row with no hook fails here, and a hook for a
/// file that left the table fails too.
///
/// This lives in an integration test on purpose: it compiles as its own crate, so
/// it cannot be silenced by the loaders it protects. A `#[test]` inside either
/// library would disappear along with everything else the moment its `mod tests;`
/// went away. `scripts/ci-check.sh` invokes this target by name, so deleting the
/// file or turning off `autotests` fails CI instead of quietly running nothing.
#[test]
fn the_protected_oracle_loader_chain_is_intact() {
    let root = workspace_root();
    let protocol = std::fs::read_to_string(root.join(PROTOCOL)).expect("the protocol is readable");

    let rows = oracle_test_file_rows(&protocol);
    assert!(
        rows.len() > 20,
        "parsed only {} rows from the {PROTOCOL} Oracle test files table, so the parse, \
         not the loader, is what broke",
        rows.len()
    );
    for row in &rows {
        assert!(
            row.ends_with(".rs"),
            "`{row}` is a non-Rust row in the Oracle test files table; this guard only knows \
             how Rust oracles are loaded, so extend it before adding that row"
        );
    }

    let map = loader_map();
    let tabled: BTreeSet<&str> = rows.iter().map(String::as_str).collect();
    let mapped: BTreeSet<&str> = map.iter().map(|(path, _)| path.as_str()).collect();
    let unmapped: Vec<&&str> = tabled.difference(&mapped).collect();
    assert!(
        unmapped.is_empty(),
        "protected oracle rows with no loader hook recorded in this guard: {unmapped:?}. \
         A row that nothing compiles is a digest over dead code"
    );
    let unprotected: Vec<&&str> = mapped.difference(&tabled).collect();
    assert!(
        unprotected.is_empty(),
        "this guard maps files that section 7 of {PROTOCOL} no longer protects: {unprotected:?}"
    );

    let mut expected_includes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut autotest_stems: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    // Source-text observations about the hooks. Reported, never asserted: matching
    // bytes does not mean the item compiled, so treating these as a verdict is
    // exactly the fake pass the inventory checker exists to remove.
    let mut diagnostics: Vec<String> = Vec::new();
    for (path, hook) in &map {
        let full = root.join(path);
        assert!(
            full.is_file(),
            "protected oracle file {path} is gone, so nothing it asserts runs"
        );
        match hook {
            Hook::Module { file, decl } => {
                let source = std::fs::read_to_string(root.join(file))
                    .unwrap_or_else(|_| panic!("{file} is readable"));
                if !source.contains(decl) {
                    diagnostics.push(format!(
                        "{file} no longer holds `{}` verbatim, the recorded hook for {path}",
                        decl.replace('\n', " ")
                    ));
                }
            }
            Hook::Include { loader, exclusive } => {
                let (loader_dir, _) = loader
                    .rsplit_once('/')
                    .expect("a loader path has a directory");
                let relative = path
                    .strip_prefix(loader_dir)
                    .and_then(|rest| rest.strip_prefix('/'))
                    .unwrap_or_else(|| panic!("{path} must sit under {loader_dir}"));
                let line = format!("include!(\"{relative}\");");
                let source = std::fs::read_to_string(root.join(loader))
                    .unwrap_or_else(|_| panic!("{loader} is readable"));
                if !source.contains(&line) {
                    diagnostics.push(format!(
                        "{loader} no longer holds `{line}`, the recorded hook for {path}"
                    ));
                }
                if *exclusive {
                    *expected_includes.entry(loader).or_default() += 1;
                }
            }
            Hook::Autotest { manifest } => {
                let stem = path
                    .rsplit_once('/')
                    .and_then(|(dir, file)| {
                        dir.ends_with("/tests")
                            .then(|| file.trim_end_matches(".rs"))
                    })
                    .unwrap_or_else(|| {
                        panic!("{path} must sit in a crate's tests/ directory to be discovered")
                    });
                autotest_stems.entry(manifest).or_default().push(stem);
            }
        }
    }

    // Both lanes must run the integration targets by name, in one real invocation
    // rather than in an `echo` of one. `cargo test --workspace` cannot stand in:
    // with `autotests = false` it reports a smaller suite and still exits 0.
    assert!(
        !autotest_stems.is_empty(),
        "the map records no automatically discovered integration test, so the lane check below \
         would pass over nothing"
    );
    for lane in CI_LANES {
        let script = std::fs::read_to_string(root.join(lane))
            .unwrap_or_else(|_| panic!("{lane} is readable"));
        for (manifest, stems) in &autotest_stems {
            let package = match *manifest {
                DECOMPILER_MANIFEST => "flutterdec-decompiler",
                CORE_MANIFEST => "flutterdec-core",
                IR_MANIFEST => "flutterdec-ir",
                _ => panic!("{manifest} has no named integration-test lane"),
            };
            let prefix = format!("nix develop -c cargo test -p {package}");
            let invocations: Vec<&str> = script
                .lines()
                .map(|line| line.trim().trim_start_matches("run: "))
                .filter(|line| line.starts_with(&prefix))
                .collect();
            let covering = invocations.iter().find(|line| {
                stems
                    .iter()
                    .all(|stem| line.contains(&format!("--test {stem}")))
            });
            assert!(
                covering.is_some(),
                "{lane} must invoke every discovered integration target in one command line, \
                 `{prefix}{}`, so deleting one of them or setting `autotests = false` in \
                 {manifest} is a hard error instead of a quietly smaller suite. Invocation \
                 lines found: {invocations:?}",
                stems
                    .iter()
                    .map(|stem| format!(" --test {stem}"))
                    .collect::<String>()
            );
        }
    }

    // The compiled-inventory checker decides whether a protected file is really
    // compiled, so a lane has to run it. Matched as a whole command line, so an
    // `echo` of it does not count.
    assert!(
        root.join(INVENTORY_CHECKER).is_file(),
        "{INVENTORY_CHECKER} is missing, so nothing asks the compiler whether the protected \
         oracles are compiled"
    );
    let inventory_lane = format!("nix develop -c python3 {INVENTORY_CHECKER}");
    for lane in CI_LANES {
        let script = std::fs::read_to_string(root.join(lane))
            .unwrap_or_else(|_| panic!("{lane} is readable"));
        assert!(
            script
                .lines()
                .any(|line| line.trim().trim_start_matches("run: ") == inventory_lane),
            "{lane} must run `{inventory_lane}` as a lane of its own. The hook checks in this \
             test are diagnostics; that checker is the only thing that proves a protected oracle \
             reached a compiled test target"
        );
    }

    for (loader, expected) in expected_includes {
        let source = std::fs::read_to_string(root.join(loader))
            .unwrap_or_else(|_| panic!("{loader} is readable"));
        assert_eq!(
            source.matches("include!").count(),
            expected,
            "{loader} is exactly its {expected} protected includes, nothing else, so it cannot \
             grow an oracle that section 7 does not record:\n{source}"
        );
    }

    // A manifest can silence a whole test target without touching one line of the
    // loaders above: `test = false` drops the library's unit tests, `harness =
    // false` replaces the harness that reports them, and `autotests = false`
    // stops Cargo from discovering anything under `tests/`.
    for manifest in [DECOMPILER_MANIFEST, CORE_MANIFEST, IR_MANIFEST] {
        let source = std::fs::read_to_string(root.join(manifest))
            .unwrap_or_else(|_| panic!("{manifest} is readable"));
        for line in source.lines() {
            let setting: String = line
                .split('#')
                .next()
                .expect("splitting always yields a first part")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            for disabled in ["test=false", "harness=false", "autotests=false"] {
                assert_ne!(
                    setting, disabled,
                    "{manifest} sets `{line}`, which silences protected oracles while every \
                     digest in section 7 still matches"
                );
            }
        }
    }

    // Printed, not asserted. A hook whose text moved is worth knowing about, but
    // `scripts/check-oracle-inventory.py` is what decides whether the oracle it
    // loads still compiles.
    for note in &diagnostics {
        println!("loader-hook diagnostic: {note}");
    }
}

#[test]
fn the_pre_call_audit_traces_each_candidate_and_its_checker_catches_a_wrong_path() {
    let dir = std::env::temp_dir().join("flutterdec-prov-audit-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let audit = dir.join("audit.jsonl");
    std::env::set_var("FLUTTERDEC_PROV_AUDIT", &audit);
    std::env::set_var("FLUTTERDEC_PROV_SAMPLE", "fixture");

    let ir = fixture();
    let stubs: HashMap<u64, RuntimeStubEffect> = HashMap::new();
    let source = emit_program_with_runtime_stubs(
        std::slice::from_ref(&ir),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &stubs,
    )
    .remove(0)
    .source;

    let text = std::fs::read_to_string(&audit).expect("the audit is written in a release build");
    let rows: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let annotations: Vec<&&str> = rows
        .iter()
        .filter(|row| row.contains("\"record\":\"annotation\""))
        .collect();
    let snapshots: Vec<&&str> = rows
        .iter()
        .filter(|row| row.contains("\"record\":\"snapshot\""))
        .collect();

    assert_eq!(
        annotations.len(),
        2,
        "one record per emitted annotation, no more and no fewer:\n{source}\n{text}"
    );
    assert_eq!(snapshots.len(), 2, "one snapshot per cited call:\n{text}");

    // Tagged keys, the call's own address, and the value each call actually
    // took. Asserting the site keys apart is what distinguishes "the audit
    // names a call" from "the audit names *this* call".
    assert!(
        annotations[0].contains("\"site_key\":[\"call\",4104]"),
        "first annotation must key on 0x1008:\n{}",
        annotations[0]
    );
    assert!(
        annotations[1].contains("\"site_key\":[\"call\",4120]"),
        "second annotation must key on 0x1018:\n{}",
        annotations[1]
    );
    assert!(annotations[0].contains("\"value\":\"slot0.f8\""));
    assert!(annotations[1].contains("\"value\":\"slot1.f16.f8\""));
    assert!(annotations[0].contains("\"loss_site\":\"call\""));
    assert!(annotations[0].contains("\"schema_version\":"));

    // The coordinate is checked against the emitted text, not taken on trust.
    for row in &annotations {
        let line: usize = field(row, "output_line").parse().expect("line number");
        let column: usize = field(row, "output_col").parse().expect("column number");
        let text = source.lines().nth(line - 1).expect("line exists");
        assert!(
            text[column - 1..].starts_with(PRE_CALL_ANNOTATION.open()),
            "the record's coordinate must land on its own annotation: {text:?} at {column}"
        );
    }

    // The emitted pseudocode and IR the checker resolves against, in the layout
    // a corpus run writes them. Passing both is what exercises the checker's
    // site-resolution and output-anchor checks rather than leaving them
    // unrun - and the anchor check reads the annotation text, so the checker's
    // copy of the literal has to agree with the emitter's or this fails.
    let pseudocode_dir = dir.join("pseudocode");
    let ir_dir = dir.join("ir");
    std::fs::create_dir_all(&pseudocode_dir).expect("pseudocode directory");
    std::fs::create_dir_all(&ir_dir).expect("ir directory");
    std::fs::write(
        pseudocode_dir.join(format!("{:05}_auditedClobber.dartpseudo", ir.function_id)),
        format!("{source}\n"),
    )
    .expect("emitted pseudocode");
    std::fs::write(
        ir_dir.join(format!("{:05}_auditedClobber.json", ir.function_id)),
        serde_json::to_vec(&ir).expect("serialisable IR"),
    )
    .expect("emitted IR");

    let unmodified = checker();
    let clean = Command::new("python3")
        .arg(&unmodified)
        .arg(&audit)
        .arg("--ir-dir")
        .arg(&ir_dir)
        .arg("--pseudocode-dir")
        .arg(&pseudocode_dir)
        .output()
        .expect("python3 available");
    assert!(
        clean.status.success(),
        "the honest audit must pass the checker:\n{}",
        String::from_utf8_lossy(&clean.stdout)
    );

    // A real violation, planted: the first annotation keeps its own site, its
    // own register and its own snapshot id, and takes its value from the other
    // call's snapshot. Everything about the record stays internally plausible,
    // and only the attribution is wrong - which is the failure this audit
    // exists to catch and the one a self-consistent emitter would produce.
    let planted_path = dir.join("planted.jsonl");
    let planted = text.replacen("\"value\":\"slot0.f8\"", "\"value\":\"slot1.f16.f8\"", 1);
    assert_ne!(planted, text, "the plant must change the audit");
    std::fs::write(&planted_path, &planted).expect("planted audit");

    let caught = Command::new("python3")
        .arg(&unmodified)
        .arg(&planted_path)
        .output()
        .expect("python3 available");
    let report = String::from_utf8_lossy(&caught.stdout).to_string();
    assert!(
        !caught.status.success(),
        "the unmodified checker must reject a candidate taken from the wrong path:\n{report}"
    );
    assert!(
        report.contains("violations snapshot  1"),
        "the violation must be counted once, against the offending candidate:\n{report}"
    );
    assert!(
        report.contains("violations total     1"),
        "and it must not be double counted by another check:\n{report}"
    );

    // A second plant, at the other end of the same binding: a genuine value
    // attributed to the snapshot it did not come from.
    let swapped_path = dir.join("swapped.jsonl");
    let first_snapshot = field(snapshots[0], "snapshot_id").to_string();
    let second_snapshot = field(snapshots[1], "snapshot_id").to_string();
    let swapped = text.replacen(
        &format!("\"snapshot_id\":\"{first_snapshot}\"}}]"),
        &format!("\"snapshot_id\":\"{second_snapshot}\"}}]"),
        1,
    );
    assert_ne!(swapped, text, "the second plant must change the audit");
    std::fs::write(&swapped_path, &swapped).expect("swapped audit");
    let caught = Command::new("python3")
        .arg(&unmodified)
        .arg(&swapped_path)
        .output()
        .expect("python3 available");
    assert!(
        !caught.status.success(),
        "a candidate citing another call's snapshot must fail:\n{}",
        String::from_utf8_lossy(&caught.stdout)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
