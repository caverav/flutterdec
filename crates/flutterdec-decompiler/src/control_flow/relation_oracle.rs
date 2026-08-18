// Literal control-flow graphs, the relations they must produce, and the checks
// that keep those relations honest across processes and across emitters.
//
// The fixtures are written as successor lists and nothing else: a case is a few
// numbers a reader can draw on paper, and the expected relations next to it are
// derived by hand from the definitions, never from a run of this analysis. The
// analysis functions are read directly rather than through `Regions`, because
// `Regions` keeps only what structuring consumes - a declined graph keeps
// nothing at all - and the full dominator and post-dominator sets are what a
// wrong relation shows up in first.

/// Independent expected relations for every fixture shape.
#[cfg(test)]
mod relation_oracle {
    use super::*;
    use flutterdec_ir::LlirInstr;

    /// A fixture graph: successor lists by block id, and nothing derived.
    ///
    /// Block `i` starts at `0x1000 + 0x10 * i`, opens with a call to its own
    /// marker symbol so the emitted artifact names the blocks it emitted, and
    /// ends in the terminator its successor list implies: no successors is a
    /// return, one is a jump, two is `cbz x0` whose taken edge is the second
    /// entry and whose fallthrough is the first, which is the order the ARM64
    /// lifter records a conditional in.
    struct Graph {
        name: &'static str,
        succs: &'static [&'static [usize]],
    }

    /// Expected relations, hand-derived from the graph next to it.
    ///
    /// Sets are ascending block ids. An empty dominator or post-dominator set
    /// means the relation says nothing about that block: it is unreachable from
    /// the entry, or no exit is reachable from it.
    struct Expected {
        reachable: &'static [usize],
        dom: &'static [&'static [usize]],
        pdom: &'static [&'static [usize]],
        /// Immediate follows, one per block, in block order.
        ipdom: &'static [Option<usize>],
        loops: &'static [ExpectedLoop],
        reducible: bool,
    }

    struct ExpectedLoop {
        header: usize,
        body: &'static [usize],
        latches: &'static [usize],
        /// Body blocks control can leave the loop from.
        exits: &'static [usize],
        /// Where a `break` out of this loop lands.
        follow: Option<usize>,
    }

    fn va(id: usize) -> u64 {
        0x1000 + 0x10 * id as u64
    }

    /// The symbol each block's opening call names, so one emitted line is
    /// attributable to exactly one block id.
    fn marker_va(id: usize) -> u64 {
        0x9000 + 0x10 * id as u64
    }

    fn instr(va: u64, op: IROp, src: String, target: String) -> LlirInstr {
        LlirInstr {
            va,
            op,
            src,
            target,
        }
    }

    /// The fixture's blocks, built from its successor lists alone.
    fn lift(graph: &Graph) -> FunctionIr {
        let blocks = graph
            .succs
            .iter()
            .enumerate()
            .map(|(id, succs)| {
                let start = va(id);
                let mut instrs = vec![instr(
                    start,
                    IROp::Call,
                    format!("bl #{:#x}", marker_va(id)),
                    format!("#{:#x}", marker_va(id)),
                )];
                let end = start + 4;
                match *succs {
                    [] => instrs.push(instr(end, IROp::Return, "ret".to_string(), String::new())),
                    [only] => instrs.push(instr(
                        end,
                        IROp::Jump,
                        format!("b #{:#x}", va(*only)),
                        format!("#{:#x}", va(*only)),
                    )),
                    [_fallthrough, taken] => instrs.push(instr(
                        end,
                        IROp::Branch,
                        format!("cbz x0, #{:#x}", va(*taken)),
                        format!("#{:#x}", va(*taken)),
                    )),
                    _ => panic!("{}: a fixture block has more than two successors", graph.name),
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
        FunctionIr {
            function_id: 7000,
            name: graph.name.replace('-', "_"),
            entry_va: va(0),
            blocks,
        }
    }

    /// Straight line, one exit.
    const LINEAR: Graph = Graph {
        name: "linear",
        succs: &[&[1], &[2], &[]],
    };
    const LINEAR_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2],
        dom: &[&[0], &[0, 1], &[0, 1, 2]],
        pdom: &[&[0, 1, 2], &[1, 2], &[2]],
        ipdom: &[Some(1), Some(2), None],
        loops: &[],
        reducible: true,
    };

    /// Two arms that rejoin: the join is the follow node of the branch.
    const DIAMOND: Graph = Graph {
        name: "diamond",
        succs: &[&[1, 2], &[3], &[3], &[]],
    };
    const DIAMOND_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3],
        dom: &[&[0], &[0, 1], &[0, 2], &[0, 3]],
        pdom: &[&[0, 3], &[1, 3], &[2, 3], &[3]],
        ipdom: &[Some(3), Some(3), Some(3), None],
        loops: &[],
        reducible: true,
    };

    /// Three predecessors on one block, reached from arms of different depths.
    const FAN_IN: Graph = Graph {
        name: "fan-in",
        succs: &[&[1, 2], &[4, 3], &[4], &[4], &[]],
    };
    const FAN_IN_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3, 4],
        dom: &[&[0], &[0, 1], &[0, 2], &[0, 1, 3], &[0, 4]],
        pdom: &[&[0, 4], &[1, 4], &[2, 4], &[3, 4], &[4]],
        ipdom: &[Some(4), Some(4), Some(4), Some(4), None],
        loops: &[],
        reducible: true,
    };

    /// One arm returns immediately, so the branch has no follow node at all.
    const EARLY_RETURN: Graph = Graph {
        name: "early-return",
        succs: &[&[2, 1], &[], &[3], &[]],
    };
    const EARLY_RETURN_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3],
        dom: &[&[0], &[0, 1], &[0, 2], &[0, 2, 3]],
        pdom: &[&[0], &[1], &[2, 3], &[3]],
        ipdom: &[None, None, Some(3), None],
        loops: &[],
        reducible: true,
    };

    /// Block 2 has no predecessor, so no relation holds of it.
    const UNREACHABLE: Graph = Graph {
        name: "unreachable",
        succs: &[&[1], &[], &[]],
    };
    const UNREACHABLE_EXPECTED: Expected = Expected {
        reachable: &[0, 1],
        dom: &[&[0], &[0, 1], &[]],
        pdom: &[&[0, 1], &[1], &[]],
        ipdom: &[Some(1), None, None],
        loops: &[],
        reducible: true,
    };

    /// An inner loop 2 <-> 3 inside an outer loop headed at 1, one exit each.
    const NESTED_LOOP: Graph = Graph {
        name: "nested-loop",
        succs: &[&[1], &[2], &[4, 3], &[2], &[5, 1], &[]],
    };
    const NESTED_LOOP_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3, 4, 5],
        dom: &[
            &[0],
            &[0, 1],
            &[0, 1, 2],
            &[0, 1, 2, 3],
            &[0, 1, 2, 4],
            &[0, 1, 2, 4, 5],
        ],
        pdom: &[
            &[0, 1, 2, 4, 5],
            &[1, 2, 4, 5],
            &[2, 4, 5],
            &[2, 3, 4, 5],
            &[4, 5],
            &[5],
        ],
        ipdom: &[Some(1), Some(2), Some(4), Some(2), Some(5), None],
        loops: &[
            ExpectedLoop {
                header: 1,
                body: &[1, 2, 3, 4],
                latches: &[4],
                exits: &[4],
                follow: Some(5),
            },
            ExpectedLoop {
                header: 2,
                body: &[2, 3],
                latches: &[3],
                exits: &[2],
                follow: Some(4),
            },
        ],
        reducible: true,
    };

    /// One loop leaving from two body blocks to two different targets, which is
    /// the case the single leaving edge cannot answer and the header's own
    /// post-dominator has to.
    const MULTI_EXIT: Graph = Graph {
        name: "multi-exit",
        succs: &[&[1], &[2, 4], &[3, 5], &[1], &[6], &[6], &[]],
    };
    const MULTI_EXIT_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3, 4, 5, 6],
        dom: &[
            &[0],
            &[0, 1],
            &[0, 1, 2],
            &[0, 1, 2, 3],
            &[0, 1, 4],
            &[0, 1, 2, 5],
            &[0, 1, 6],
        ],
        pdom: &[
            &[0, 1, 6],
            &[1, 6],
            &[2, 6],
            &[1, 3, 6],
            &[4, 6],
            &[5, 6],
            &[6],
        ],
        ipdom: &[Some(1), Some(6), Some(6), Some(1), Some(6), Some(6), None],
        loops: &[ExpectedLoop {
            header: 1,
            body: &[1, 2, 3],
            latches: &[3],
            exits: &[1, 2],
            follow: Some(6),
        }],
        reducible: true,
    };

    /// A cycle with no return anywhere, the shape post-dominance has no exit to
    /// anchor on.
    const NO_EXIT: Graph = Graph {
        name: "no-exit",
        succs: &[&[1], &[2], &[1]],
    };
    const NO_EXIT_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2],
        dom: &[&[0], &[0, 1], &[0, 1, 2]],
        // No exit is reachable from any block, so "every path to an exit passes
        // through" holds of nothing and the relation is empty rather than
        // universal. Where a `break` could land is still answered, by the loop's
        // own leaving edges: there are none, so there is nowhere to break to.
        pdom: &[&[], &[], &[]],
        ipdom: &[None, None, None],
        loops: &[ExpectedLoop {
            header: 1,
            body: &[1, 2],
            latches: &[2],
            exits: &[],
            follow: None,
        }],
        reducible: true,
    };

    /// One arm returns and the other enters a loop that never can, so the two
    /// halves of the same graph have to be answered differently: block 1 really
    /// does post-dominate the entry, and nothing post-dominates a block inside the
    /// endless loop.
    ///
    /// Both of the loop's arms are latches, which is what makes the empty-set
    /// answer matter: with the whole reachable set standing in for the
    /// post-dominators of a trapped block, the two arms tie on set size and the
    /// tie-break hands the conditional at block 2 one of its own arms as the
    /// follow node the other arm is supposed to converge on.
    const TRAPPED_LOOP: Graph = Graph {
        name: "trapped-loop",
        succs: &[&[1, 2], &[], &[3, 4], &[2], &[2]],
    };
    const TRAPPED_LOOP_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3, 4],
        dom: &[&[0], &[0, 1], &[0, 2], &[0, 2, 3], &[0, 2, 4]],
        pdom: &[&[0, 1], &[1], &[], &[], &[]],
        ipdom: &[Some(1), None, None, None, None],
        loops: &[ExpectedLoop {
            header: 2,
            body: &[2, 3, 4],
            latches: &[3, 4],
            exits: &[],
            follow: None,
        }],
        reducible: true,
    };

    /// Two entries into the 1 <-> 2 cycle, so neither cycle block dominates the
    /// other and structuring must decline.
    const IRREDUCIBLE: Graph = Graph {
        name: "irreducible",
        succs: &[&[1, 2], &[2, 3], &[1, 3], &[]],
    };
    const IRREDUCIBLE_EXPECTED: Expected = Expected {
        reachable: &[0, 1, 2, 3],
        dom: &[&[0], &[0, 1], &[0, 2], &[0, 3]],
        pdom: &[&[0, 3], &[1, 3], &[2, 3], &[3]],
        ipdom: &[Some(3), Some(3), Some(3), None],
        // A retreating edge whose target does not dominate its source is not a
        // back edge, so no natural loop is found even though the graph cycles.
        loops: &[],
        reducible: false,
    };

    /// Every case, so a shape cannot be dropped from the table unnoticed.
    const CASES: &[(&Graph, &Expected)] = &[
        (&LINEAR, &LINEAR_EXPECTED),
        (&DIAMOND, &DIAMOND_EXPECTED),
        (&FAN_IN, &FAN_IN_EXPECTED),
        (&EARLY_RETURN, &EARLY_RETURN_EXPECTED),
        (&UNREACHABLE, &UNREACHABLE_EXPECTED),
        (&NESTED_LOOP, &NESTED_LOOP_EXPECTED),
        (&MULTI_EXIT, &MULTI_EXIT_EXPECTED),
        (&NO_EXIT, &NO_EXIT_EXPECTED),
        (&IRREDUCIBLE, &IRREDUCIBLE_EXPECTED),
        (&TRAPPED_LOOP, &TRAPPED_LOOP_EXPECTED),
    ];

    /// The symbol each block's opening call names.
    fn symbols(graph: &Graph) -> HashMap<u64, String> {
        (0..graph.succs.len())
            .map(|id| (marker_va(id), format!("mark{id}")))
            .collect()
    }

    /// Every relation the analysis derives for one graph, as ordered text.
    ///
    /// The analysis functions are called with the same inputs `Regions::build`
    /// gives them, in the same order, so this is the relation set structuring
    /// runs on and not a second derivation of it.
    fn relations(graph: &Graph) -> Vec<String> {
        let ir = lift(graph);
        let (succs, preds, reachable) = reachable_edges(&ir);
        let dom = dominators(&succs, &preds, &reachable);
        let irreducible = is_irreducible(&succs, &dom, &reachable);
        let pdom = post_dominators(&succs, &preds, &reachable);
        let ipdom = immediate_post_dominators(&pdom);
        let loops = natural_loops(&succs, &preds, &dom, &ipdom, &reachable);

        let ids = |ids: &[usize]| {
            ids.iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        let ordered = |members: &BTreeSet<usize>| {
            members
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };

        let case = graph.name;
        let live: Vec<usize> = (0..reachable.len()).filter(|id| reachable[*id]).collect();
        let mut lines = vec![
            format!("{case}|reducible|{}", !irreducible),
            format!("{case}|reachable|{}", ids(&live)),
        ];
        for id in 0..ir.blocks.len() {
            lines.push(format!("{case}|dom|{id}|{}", ids(&ascending(&dom[id]))));
            lines.push(format!("{case}|pdom|{id}|{}", ids(&ascending(&pdom[id]))));
            lines.push(format!("{case}|ipdom|{id}|{:?}", ipdom[id]));
            lines.push(format!("{case}|succs|{id}|{}", ids(&succs[id])));
            lines.push(format!("{case}|preds|{id}|{}", ids(&preds[id])));
        }
        for (header, region) in &loops {
            lines.push(format!("{case}|loop-body|{header}|{}", ordered(&region.body)));
            lines.push(format!(
                "{case}|loop-latches|{header}|{}",
                ordered(&region.latches)
            ));
            lines.push(format!(
                "{case}|loop-exits|{header}|{}",
                ordered(&region.exits)
            ));
            lines.push(format!("{case}|loop-follow|{header}|{:?}", region.follow));
        }
        lines
    }

    /// The relations structuring reads back through `Regions`, and the artifact the
    /// public emitter builds out of them. A declined graph has no `Regions`, which
    /// is itself part of the record.
    fn consumed_relations_and_artifact(graph: &Graph) -> Vec<String> {
        let ir = lift(graph);
        let case = graph.name;
        let mut lines = Vec::new();
        match Regions::build(&ir) {
            None => lines.push(format!("{case}|regions|declined")),
            Some(regions) => {
                lines.push(format!(
                    "{case}|regions|built|{}",
                    regions.reachable_count()
                ));
                for id in 0..ir.blocks.len() {
                    lines.push(format!(
                        "{case}|consumed|{id}|join={} header={} follow={:?} loop-follow={:?} preds={:?} succs={:?}",
                        regions.is_join(id),
                        regions.is_loop_header(id),
                        regions.follow_of(id),
                        regions.loop_follow_of(id),
                        regions.predecessors(id),
                        regions.successors(id),
                    ));
                }
            }
        }
        let artifact = crate::emit_pseudocode(&ir, &symbols(graph));
        for (index, line) in artifact.source.lines().enumerate() {
            lines.push(format!("{case}|source|{index}|{line}"));
        }
        lines.push(format!(
            "{case}|counters|unresolved_cf={} repeated_blocks={} placeholder_ifs={}",
            artifact.unresolved_cf, artifact.repeated_blocks, artifact.placeholder_ifs
        ));
        lines
    }

    /// Every case's relations and artifact, in one byte-comparable record.
    fn normalized_dump() -> String {
        let mut lines = Vec::new();
        for (graph, _) in CASES {
            lines.extend(relations(graph));
            lines.extend(consumed_relations_and_artifact(graph));
        }
        lines.join("\n")
    }

    fn ascending(ids: &HashSet<usize>) -> Vec<usize> {
        let mut ids: Vec<usize> = ids.iter().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// One case against its hand-written expectations, every relation named
    /// separately so a failure says which one disagreed.
    fn assert_case(graph: &Graph, expected: &Expected) {
        let ir = lift(graph);
        let blocks = ir.blocks.len();
        assert_eq!(
            blocks,
            expected.dom.len(),
            "{}: the expected table must cover every block",
            graph.name
        );
        assert_eq!(blocks, expected.pdom.len(), "{}", graph.name);
        assert_eq!(blocks, expected.ipdom.len(), "{}", graph.name);

        let (succs, preds, reachable) = reachable_edges(&ir);
        let dom = dominators(&succs, &preds, &reachable);
        let pdom = post_dominators(&succs, &preds, &reachable);
        let ipdom = immediate_post_dominators(&pdom);
        let loops = natural_loops(&succs, &preds, &dom, &ipdom, &reachable);

        let live: Vec<usize> = (0..blocks).filter(|id| reachable[*id]).collect();
        assert_eq!(
            live, expected.reachable,
            "{}: reachable set",
            graph.name
        );
        assert_eq!(
            is_irreducible(&succs, &dom, &reachable),
            !expected.reducible,
            "{}: reducibility verdict",
            graph.name
        );
        for id in 0..blocks {
            assert_eq!(
                ascending(&dom[id]),
                expected.dom[id],
                "{}: dominators of block {id}",
                graph.name
            );
            assert_eq!(
                ascending(&pdom[id]),
                expected.pdom[id],
                "{}: post-dominators of block {id}",
                graph.name
            );
            assert_eq!(
                ipdom[id], expected.ipdom[id],
                "{}: immediate follow of block {id}",
                graph.name
            );
        }

        let headers: Vec<usize> = loops.keys().copied().collect();
        let expected_headers: Vec<usize> = expected.loops.iter().map(|l| l.header).collect();
        assert_eq!(
            headers, expected_headers,
            "{}: loop headers",
            graph.name
        );
        for want in expected.loops {
            let region = &loops[&want.header];
            let members = |ids: &BTreeSet<usize>| ids.iter().copied().collect::<Vec<usize>>();
            assert_eq!(
                members(&region.body),
                want.body,
                "{}: body of the loop headed at {}",
                graph.name,
                want.header
            );
            assert_eq!(
                members(&region.latches),
                want.latches,
                "{}: latches of the loop headed at {}",
                graph.name,
                want.header
            );
            assert_eq!(
                members(&region.exits),
                want.exits,
                "{}: exit blocks of the loop headed at {}",
                graph.name,
                want.header
            );
            assert_eq!(
                region.follow, want.follow,
                "{}: follow of the loop headed at {}",
                graph.name,
                want.header
            );
        }
    }

    /// Set on a child to make this test dump the record instead of spawning.
    const DUMP_REQUEST: &str = "FLUTTERDEC_CFG_RELATION_DUMP";
    /// The child is filtered to this test by name, so the name is part of the
    /// contract: a rename that misses it leaves the child running no test at all,
    /// which the empty-record assertion below refuses rather than passes.
    const DUMP_TEST: &str =
        "control_flow::relation_oracle::normalized_relations_are_identical_in_twenty_processes";
    const RECORD_PREFIX: &str = "record|";
    const CANARY_PREFIX: &str = "canary|";
    const PROCESSES: usize = 20;

    /// A value decided by `HashSet` iteration order and nothing else.
    ///
    /// The whole point of paying for 20 processes is that each one seeds its
    /// hashers differently. If they did not, every relation below would agree for
    /// a reason that has nothing to do with the analysis being ordered, and the
    /// check would pass vacuously forever. This is what tells the two apart.
    fn hash_order_canary() -> String {
        let members: HashSet<usize> = (0..32).collect();
        members
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Identical `FunctionIr` in, identical normalized relations out, in 20
    /// separate processes with 20 separate hash seeds.
    ///
    /// A process boundary is the only way to test this. Within one process the
    /// hashers are seeded once, so repeating the analysis in a loop re-uses the
    /// same iteration order and would agree however order-dependent the analysis
    /// was. Each child re-executes this same test binary filtered to this same
    /// test, with the request variable set so it dumps rather than spawns.
    #[test]
    fn normalized_relations_are_identical_in_twenty_processes() {
        if std::env::var_os(DUMP_REQUEST).is_some() {
            // The harness leaves `test <name> ... ` unterminated, so the first
            // line printed here would otherwise carry that prefix and not match.
            println!();
            println!("{CANARY_PREFIX}{}", hash_order_canary());
            for line in normalized_dump().lines() {
                println!("{RECORD_PREFIX}{line}");
            }
            return;
        }

        let exe = std::env::current_exe().expect("the running test binary");
        let mut records: Vec<Vec<String>> = Vec::with_capacity(PROCESSES);
        let mut canaries: BTreeSet<String> = BTreeSet::new();
        for index in 0..PROCESSES {
            let child = std::process::Command::new(&exe)
                .args(["--exact", "--nocapture", "--test-threads=1", DUMP_TEST])
                .env(DUMP_REQUEST, "1")
                .output()
                .expect("re-execute the test binary");
            assert!(
                child.status.success(),
                "process {index} failed: {}",
                String::from_utf8_lossy(&child.stderr)
            );
            let stdout = String::from_utf8(child.stdout).expect("the record is utf-8");
            let record: Vec<String> = stdout
                .lines()
                .filter_map(|line| line.strip_prefix(RECORD_PREFIX))
                .map(str::to_string)
                .collect();
            assert!(
                !record.is_empty(),
                "process {index} produced no record, so {DUMP_TEST} named no test"
            );
            canaries.extend(
                stdout
                    .lines()
                    .filter_map(|line| line.strip_prefix(CANARY_PREFIX))
                    .map(str::to_string),
            );
            records.push(record);
        }

        assert!(
            canaries.len() > 1,
            "all {PROCESSES} processes iterated a `HashSet` in the same order, so this \
             comparison proves nothing about the analysis"
        );

        let first = &records[0];
        for (index, record) in records.iter().enumerate().skip(1) {
            if record == first {
                continue;
            }
            let differing = first
                .iter()
                .zip(record)
                .find(|(want, got)| want != got)
                .map(|(want, got)| format!("process 0 said `{want}`, process {index} said `{got}`"))
                .unwrap_or_else(|| {
                    format!(
                        "process 0 recorded {} lines, process {index} recorded {}",
                        first.len(),
                        record.len()
                    )
                });
            panic!("cross-process relation difference: {differing}");
        }
        assert_eq!(
            normalized_dump(),
            first.join("\n"),
            "this process disagreed with the {PROCESSES} it spawned"
        );
    }

    #[test]
    fn the_case_table_covers_every_required_shape() {
        let names: Vec<&str> = CASES.iter().map(|(graph, _)| graph.name).collect();
        assert_eq!(
            names,
            [
                "linear",
                "diamond",
                "fan-in",
                "early-return",
                "unreachable",
                "nested-loop",
                "multi-exit",
                "no-exit",
                "irreducible",
                "trapped-loop",
            ],
            "a case may not be dropped from the table without the list saying so"
        );
    }

    #[test]
    fn linear_relations_match_the_expected_graph() {
        assert_case(&LINEAR, &LINEAR_EXPECTED);
    }

    #[test]
    fn diamond_relations_match_the_expected_graph() {
        assert_case(&DIAMOND, &DIAMOND_EXPECTED);
    }

    #[test]
    fn fan_in_relations_match_the_expected_graph() {
        assert_case(&FAN_IN, &FAN_IN_EXPECTED);
    }

    #[test]
    fn early_return_relations_match_the_expected_graph() {
        assert_case(&EARLY_RETURN, &EARLY_RETURN_EXPECTED);
    }

    #[test]
    fn unreachable_relations_match_the_expected_graph() {
        assert_case(&UNREACHABLE, &UNREACHABLE_EXPECTED);
    }

    #[test]
    fn nested_loop_relations_match_the_expected_graph() {
        assert_case(&NESTED_LOOP, &NESTED_LOOP_EXPECTED);
    }

    #[test]
    fn multi_exit_relations_match_the_expected_graph() {
        assert_case(&MULTI_EXIT, &MULTI_EXIT_EXPECTED);
    }

    #[test]
    fn no_exit_relations_match_the_expected_graph() {
        assert_case(&NO_EXIT, &NO_EXIT_EXPECTED);
    }

    #[test]
    fn trapped_loop_relations_match_the_expected_graph() {
        assert_case(&TRAPPED_LOOP, &TRAPPED_LOOP_EXPECTED);
    }

    #[test]
    fn irreducible_relations_match_the_expected_graph() {
        assert_case(&IRREDUCIBLE, &IRREDUCIBLE_EXPECTED);
    }
}
