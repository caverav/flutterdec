// Control-flow structuring that emits every reachable block exactly once.
//
// The DFS emitter in `emit.rs` inlines both successors of every branch, which
// expands a DAG into a tree: 33% of basic blocks in a real Flutter AOT binary
// are join points, so each is re-emitted once per incoming path until a visit
// budget cuts it off and the remainder is dropped. Measured on a 5,800-function
// `libapp.so`, half of all emitted lines were exact duplicates and 55% of
// machine calls never appeared at all.
//
// This pass drives emission from the dominator and post-dominator trees
// instead. Every conditional in that binary falls into one of three shapes the
// follow-node rule covers, with no fourth case: the immediate post-dominator is
// a join block (56.2%), there is no post-dominator because the arms never
// rejoin (30.9%), or the post-dominator is one of the two successors (12.9%).

/// Reachable-only CFG plus the dominance facts structuring needs.
pub(super) struct Regions {
    /// Successors, restricted to blocks reachable from the entry.
    succs: Vec<Vec<usize>>,
    preds: Vec<Vec<usize>>,
    reachable: Vec<bool>,
    /// Immediate post-dominator, the follow node of a conditional.
    ipdom: Vec<Option<usize>>,
    /// One entry per natural-loop header, in header order.
    loops: BTreeMap<usize, NaturalLoop>,
}

/// The relations one natural loop carries, all of them block ids in its own
/// body's terms.
///
/// Ordered sets and an ordered map rather than the hashed ones this started as:
/// `follow` is chosen by walking the body's leaving edges, and `structured.rs`
/// asks `in_loop` and `loop_follow_of` for decisions that reach the artifact, so
/// the traversal order is part of the result and not an implementation detail.
/// Ordering the containers is how that order stops being the hash seed's choice.
struct NaturalLoop {
    /// Blocks the loop contains, its header included.
    body: BTreeSet<usize>,
    /// Back-edge sources: the blocks whose edge re-enters the header.
    latches: BTreeSet<usize>,
    /// Body blocks with an edge that leaves the body.
    exits: BTreeSet<usize>,
    /// Where a `break` out of this loop lands.
    follow: Option<usize>,
}

impl NaturalLoop {
    fn new(header: usize) -> Self {
        Self {
            body: BTreeSet::from([header]),
            latches: BTreeSet::new(),
            exits: BTreeSet::new(),
            follow: None,
        }
    }

    /// Every block this loop names is inside its own body, and its follow node is
    /// outside it. Checked rather than assumed because the four relations are
    /// derived in two passes over different edge sets: the body from a backwards
    /// walk over predecessors, the latches from the back edges themselves, and
    /// the exits and the follow from a forwards walk over successors. A body that
    /// disagreed with any of them would send `structured.rs` looking for a
    /// `break` target it can reach by falling through instead.
    fn is_self_consistent(&self, header: usize) -> bool {
        self.body.contains(&header)
            && self.latches.iter().all(|latch| self.body.contains(latch))
            && self.exits.iter().all(|exit| self.body.contains(exit))
            && self.follow.is_none_or(|follow| !self.body.contains(&follow))
    }
}

impl Regions {
    pub(super) fn build(ir: &FunctionIr) -> Option<Self> {
        // The same ruler the public emission entry points apply, not a local copy
        // of part of it: everything below is an id-indexed vector that
        // `structured.rs` reads back by block id, so a graph whose ids are not
        // dense or whose edges name absent blocks would have its relations read
        // off the wrong rows. Declining is the right answer here rather than a
        // diagnostic, because a decline is already how this pass reports a graph
        // it cannot structure.
        validate_block_identity(ir).ok()?;
        let n = ir.blocks.len();
        if n == 0 {
            return None;
        }
        let (succs, preds, reachable) = reachable_edges(ir);

        let dom = dominators(&succs, &preds, &reachable);
        // Irreducible control flow has a retreating edge whose target does not
        // dominate its source. Structuring it needs node splitting, so those
        // functions stay on the DFS emitter.
        if is_irreducible(&succs, &dom, &reachable) {
            return None;
        }

        let pdom = post_dominators(&succs, &preds, &reachable);
        let ipdom = immediate_post_dominators(&pdom);
        let loops = natural_loops(&succs, &preds, &dom, &ipdom, &reachable);
        debug_assert!(
            loops
                .iter()
                .all(|(header, region)| region.is_self_consistent(*header)),
            "a loop relation named a block outside its own body"
        );

        Some(Self {
            succs,
            preds,
            reachable,
            ipdom,
            loops,
        })
    }

    pub(super) fn is_reachable(&self, id: usize) -> bool {
        self.reachable.get(id).copied().unwrap_or(false)
    }

    pub(super) fn is_join(&self, id: usize) -> bool {
        self.preds.get(id).map(|p| p.len()).unwrap_or(0) > 1
    }

    /// The block's reachable predecessors, ascending. Built once from the same
    /// successor edges `is_join` counts, so a caller enumerating a join's
    /// incoming paths cannot disagree with the join test itself. Ascending is
    /// relied on: it is the canonical order candidate provenance is recorded in.
    pub(super) fn predecessors(&self, id: usize) -> &[usize] {
        self.preds.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(super) fn successors(&self, id: usize) -> &[usize] {
        self.succs.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(super) fn follow_of(&self, id: usize) -> Option<usize> {
        self.ipdom.get(id).copied().flatten()
    }

    pub(super) fn is_loop_header(&self, id: usize) -> bool {
        self.loops.contains_key(&id)
    }

    pub(super) fn in_loop(&self, header: usize, id: usize) -> bool {
        self.loops
            .get(&header)
            .map(|region| region.body.contains(&id))
            .unwrap_or(false)
    }

    pub(super) fn loop_follow_of(&self, header: usize) -> Option<usize> {
        self.loops.get(&header).and_then(|region| region.follow)
    }

    pub(super) fn reachable_count(&self) -> usize {
        self.reachable.iter().filter(|r| **r).count()
    }
}

/// The edge lists every relation below is derived from: successors by block id,
/// predecessors re-derived from them, and reachability from the entry.
///
/// Split out of `Regions::build` so the relation functions can be driven from one
/// place with exactly the inputs the emitter's own run gives them. A graph the
/// analysis declines still has these three, which is the only way to say anything
/// about an irreducible graph's relations at all.
fn reachable_edges(ir: &FunctionIr) -> (Vec<Vec<usize>>, Vec<Vec<usize>>, Vec<bool>) {
    let n = ir.blocks.len();
    let mut succs = vec![Vec::new(); n];
    for b in &ir.blocks {
        succs[b.id] = b.succs.clone();
    }

    let mut reachable = vec![false; n];
    reachable[0] = true;
    let mut stack = vec![0usize];
    while let Some(u) = stack.pop() {
        for &v in &succs[u] {
            if !reachable[v] {
                reachable[v] = true;
                stack.push(v);
            }
        }
    }
    for (u, keep) in reachable.iter().enumerate() {
        if !keep {
            succs[u].clear();
        }
    }

    let mut preds = vec![Vec::new(); n];
    for u in 0..n {
        if !reachable[u] {
            continue;
        }
        for &v in &succs[u] {
            preds[v].push(u);
        }
    }

    (succs, preds, reachable)
}

fn dominators(succs: &[Vec<usize>], preds: &[Vec<usize>], reachable: &[bool]) -> Vec<HashSet<usize>> {
    let n = succs.len();
    let all: HashSet<usize> = (0..n).filter(|i| reachable[*i]).collect();
    let mut dom: Vec<HashSet<usize>> = (0..n)
        .map(|i| {
            if reachable[i] {
                all.clone()
            } else {
                HashSet::new()
            }
        })
        .collect();
    dom[0] = HashSet::from([0]);
    let mut changed = true;
    while changed {
        changed = false;
        for u in 1..n {
            if !reachable[u] {
                continue;
            }
            let mut new: Option<HashSet<usize>> = None;
            for &p in &preds[u] {
                new = Some(match new {
                    None => dom[p].clone(),
                    Some(acc) => acc.intersection(&dom[p]).copied().collect(),
                });
            }
            let mut new = new.unwrap_or_default();
            new.insert(u);
            if new != dom[u] {
                dom[u] = new;
                changed = true;
            }
        }
    }
    dom
}

fn is_irreducible(succs: &[Vec<usize>], dom: &[HashSet<usize>], reachable: &[bool]) -> bool {
    // Depth-first search; an edge back to a node still on the stack is
    // retreating, and a retreating edge to a non-dominator is irreducible.
    let n = succs.len();
    let mut seen = vec![false; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    seen[0] = true;
    on_stack[0] = true;
    while let Some((u, idx)) = stack.pop() {
        if idx < succs[u].len() {
            stack.push((u, idx + 1));
            let v = succs[u][idx];
            if !reachable[v] {
                continue;
            }
            if on_stack[v] {
                if !dom[u].contains(&v) {
                    return true;
                }
            } else if !seen[v] {
                seen[v] = true;
                on_stack[v] = true;
                stack.push((v, 0));
            }
        } else {
            on_stack[u] = false;
        }
    }
    false
}

/// Full post-dominator sets: `pdom[u]` holds every block on every path from `u`
/// to an exit, `u` itself included.
///
/// A block with no path to any exit gets the empty set. "Every path to an exit
/// passes through" is not a statement about such a block: it has no such path, so
/// the intersection below has nothing to shrink the universe with and every block
/// of an endless cycle comes out post-dominating every other one, its own
/// dominators included. The relation `immediate_post_dominators` reads off that is
/// not a tree either - two blocks of the cycle each come out as the other's
/// nearest post-dominator - and it reaches `structured.rs` as the follow node of a
/// conditional, which is where a branch's arms are told to converge. The empty set
/// is reported as no follow node instead, and the loop's own exit relation, which
/// is derived from the leaving edges rather than from post-dominance, is what
/// still answers where such a loop can be left.
fn post_dominators(succs: &[Vec<usize>], preds: &[Vec<usize>], reachable: &[bool]) -> Vec<HashSet<usize>> {
    let n = succs.len();
    let exits: Vec<usize> = (0..n)
        .filter(|i| reachable[*i] && succs[*i].is_empty())
        .collect();

    // Backwards from the exits: `preds` is already restricted to reachable
    // blocks, so this cannot pick up a block the entry never reaches.
    let mut reaches_exit = vec![false; n];
    let mut stack = exits.clone();
    for &e in &exits {
        reaches_exit[e] = true;
    }
    while let Some(u) = stack.pop() {
        for &p in &preds[u] {
            if !reaches_exit[p] {
                reaches_exit[p] = true;
                stack.push(p);
            }
        }
    }

    let all: HashSet<usize> = (0..n).filter(|i| reaches_exit[*i]).collect();
    let mut pdom: Vec<HashSet<usize>> = (0..n)
        .map(|i| {
            if reaches_exit[i] {
                all.clone()
            } else {
                HashSet::new()
            }
        })
        .collect();
    for &e in &exits {
        pdom[e] = HashSet::from([e]);
    }
    let mut changed = true;
    while changed {
        changed = false;
        for u in (0..n).rev() {
            if !reaches_exit[u] || succs[u].is_empty() {
                continue;
            }
            let mut new: Option<HashSet<usize>> = None;
            for &s in &succs[u] {
                // A successor with no path to an exit contributes no path to an
                // exit, so it constrains nothing here.
                if !reaches_exit[s] {
                    continue;
                }
                new = Some(match new {
                    None => pdom[s].clone(),
                    Some(acc) => acc.intersection(&pdom[s]).copied().collect(),
                });
            }
            let mut new = new.unwrap_or_default();
            new.insert(u);
            if new != pdom[u] {
                pdom[u] = new;
                changed = true;
            }
        }
    }

    pdom
}

/// The nearest strict post-dominator of each block, which is the follow node of a
/// conditional there.
///
/// Nearer nodes are post-dominated by more blocks, so the largest post-dominator
/// set wins: for A -> {B, C} -> D -> exit, both D and exit strictly post-dominate
/// A, and D is the follow node.
fn immediate_post_dominators(pdom: &[HashSet<usize>]) -> Vec<Option<usize>> {
    (0..pdom.len())
        .map(|u| {
            // Empty for a block with no path to an exit, which yields no follow
            // node at all.
            //
            // The strict post-dominators of one block form a chain, so their set
            // sizes are distinct and the block index never decides. It is in the
            // key regardless: `pdom[u]` is a `HashSet` whose iteration order is
            // seeded per process and `max_by_key` keeps the last maximum, so a
            // comparator that could tie would hand the follow node to that seed
            // and the emitted structure would not be reproducible.
            pdom[u]
                .iter()
                .copied()
                .filter(|p| *p != u)
                .max_by_key(|p| (pdom[*p].len(), std::cmp::Reverse(*p)))
        })
        .collect()
}

/// Every natural loop, keyed by header: the blocks it holds, the back edges that
/// close it, the blocks control can leave it from, and where a `break` lands.
fn natural_loops(
    succs: &[Vec<usize>],
    preds: &[Vec<usize>],
    dom: &[HashSet<usize>],
    ipdom: &[Option<usize>],
    reachable: &[bool],
) -> BTreeMap<usize, NaturalLoop> {
    let n = succs.len();
    let mut loops: BTreeMap<usize, NaturalLoop> = BTreeMap::new();
    for u in 0..n {
        if !reachable[u] {
            continue;
        }
        for &v in &succs[u] {
            // Back edge: the target dominates its source.
            if !dom[u].contains(&v) {
                continue;
            }
            let region = loops.entry(v).or_insert_with(|| NaturalLoop::new(v));
            region.latches.insert(u);
            let mut stack = vec![u];
            while let Some(x) = stack.pop() {
                if !region.body.insert(x) {
                    continue;
                }
                for &p in &preds[x] {
                    if p != v {
                        stack.push(p);
                    }
                }
            }
        }
    }

    for (header, region) in loops.iter_mut() {
        // Where control lands on leaving the loop, and which body blocks it can
        // leave from.
        let mut targets: BTreeSet<usize> = BTreeSet::new();
        for &b in &region.body {
            for &t in &succs[b] {
                if !region.body.contains(&t) {
                    region.exits.insert(b);
                    targets.insert(t);
                }
            }
        }
        region.follow = match targets.len() {
            0 => None,
            1 => targets.first().copied(),
            // 805 of the loop nests in the sampled binary have more than one
            // exit block. The header's post-dominator is the one target every
            // exit eventually reaches, so it is the `break` destination; the
            // others are rendered in place inside the loop.
            _ => ipdom[*header].filter(|f| !region.body.contains(f)),
        };
    }

    loops
}

/// The analysis boundary in isolation.
///
/// `Regions::build` is where a graph first becomes id-indexed relation vectors,
/// and `structured.rs` reads every one of them back by block id. The public
/// emitter refuses a graph that fails the shared ruler before reaching here, so
/// these cases can only be produced by calling the analysis directly -- which is
/// exactly what a later in-crate caller would do, and what this pins.
#[cfg(test)]
mod identity_boundary_tests {
    use super::*;
    use flutterdec_ir::LlirInstr;

    fn blk(id: usize, start_va: u64, succs: Vec<usize>) -> BasicBlock {
        BasicBlock {
            id,
            start_va,
            instrs: vec![LlirInstr {
                va: start_va,
                op: IROp::Other,
                src: "mov x0, x1".to_string(),
                target: String::new(),
            }],
            succs,
            preds: Vec::new(),
        }
    }

    fn diamond() -> FunctionIr {
        FunctionIr {
            function_id: 1,
            name: "diamond".to_string(),
            entry_va: 0x1000,
            blocks: vec![
                blk(0, 0x1000, vec![1, 2]),
                blk(1, 0x1004, vec![3]),
                blk(2, 0x1008, vec![3]),
                blk(3, 0x100c, Vec::new()),
            ],
        }
    }

    /// The control row: this shape is reducible and must still be analysed, so a
    /// decline below cannot be blamed on the fixture.
    #[test]
    fn a_well_formed_reducible_graph_is_still_analysed() {
        let regions = Regions::build(&diamond()).expect("a diamond is reducible");
        assert!(regions.is_join(3), "block 3 is the join");
        assert_eq!(regions.reachable_count(), 4);
    }

    #[test]
    fn every_planted_identity_failure_declines_before_any_relation_is_built() {
        let mut duplicate_id = diamond();
        duplicate_id.blocks[2].id = 1;

        let mut sparse_id = diamond();
        sparse_id.blocks[3].id = 9;

        let mut entry_not_first = diamond();
        entry_not_first.blocks.swap(0, 1);

        let mut duplicate_start = diamond();
        duplicate_start.blocks[2].start_va = 0x1004;

        let mut missing_succ = diamond();
        missing_succ.blocks[1].succs = vec![7];

        let mut missing_pred = diamond();
        missing_pred.blocks[1].preds = vec![7];

        let mut no_entry = diamond();
        for (offset, b) in no_entry.blocks.iter_mut().enumerate() {
            b.id = offset + 1;
            b.succs = Vec::new();
        }

        for (label, ir) in [
            ("duplicate id", duplicate_id),
            ("non-dense id", sparse_id),
            ("entry not first", entry_not_first),
            ("duplicate start address", duplicate_start),
            ("successor names no block", missing_succ),
            ("predecessor names no block", missing_pred),
            ("no entry block 0", no_entry),
        ] {
            assert!(
                Regions::build(&ir).is_none(),
                "{label}: relation analysis must decline, not read another block's rows"
            );
        }
    }
}
