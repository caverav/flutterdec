// Literal control-flow graphs, the relations they must produce, and the two
// checks that keep those relations honest across processes and across emitters.
//
// The fixtures are written as successor lists and nothing else: a case is nine
// numbers a reader can draw on paper, and the expected relations next to it are
// derived by hand from the definitions, never from a run of this analysis. The
// analysis functions are read directly rather than through `Regions`, because
// `Regions` keeps only what structuring consumes - a declined graph keeps
// nothing at all - and the full dominator and post-dominator sets are what a
// wrong relation shows up in first.

/// Independent expected relations for the nine graph shapes.
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
        (&IRREDUCIBLE, &IRREDUCIBLE_EXPECTED),
    ];

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
        let pdom = post_dominators(&succs, &reachable);
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
                "irreducible",
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
    fn irreducible_relations_match_the_expected_graph() {
        assert_case(&IRREDUCIBLE, &IRREDUCIBLE_EXPECTED);
    }
}
