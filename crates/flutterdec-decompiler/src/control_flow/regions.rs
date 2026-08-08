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
    /// Blocks the innermost loop of a header contains.
    loop_body: HashMap<usize, HashSet<usize>>,
    /// Where a `break` out of a header's loop lands.
    loop_follow: HashMap<usize, Option<usize>>,
}

impl Regions {
    pub(super) fn build(ir: &FunctionIr) -> Option<Self> {
        let n = ir.blocks.len();
        if n == 0 {
            return None;
        }
        let mut succs = vec![Vec::new(); n];
        for b in &ir.blocks {
            if b.id >= n {
                return None;
            }
            succs[b.id] = b.succs.iter().copied().filter(|s| *s < n).collect();
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

        let dom = dominators(&succs, &preds, &reachable);
        // Irreducible control flow has a retreating edge whose target does not
        // dominate its source. Structuring it needs node splitting, so those
        // functions stay on the DFS emitter.
        if is_irreducible(&succs, &dom, &reachable) {
            return None;
        }

        let ipdom = immediate_post_dominators(&succs, &reachable);
        let (loop_body, loop_follow) = natural_loops(&succs, &preds, &dom, &ipdom, &reachable);

        Some(Self {
            succs,
            preds,
            reachable,
            ipdom,
            loop_body,
            loop_follow,
        })
    }

    pub(super) fn is_reachable(&self, id: usize) -> bool {
        self.reachable.get(id).copied().unwrap_or(false)
    }

    pub(super) fn is_join(&self, id: usize) -> bool {
        self.preds.get(id).map(|p| p.len()).unwrap_or(0) > 1
    }

    pub(super) fn successors(&self, id: usize) -> &[usize] {
        self.succs.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(super) fn follow_of(&self, id: usize) -> Option<usize> {
        self.ipdom.get(id).copied().flatten()
    }

    pub(super) fn is_loop_header(&self, id: usize) -> bool {
        self.loop_body.contains_key(&id)
    }

    pub(super) fn in_loop(&self, header: usize, id: usize) -> bool {
        self.loop_body
            .get(&header)
            .map(|body| body.contains(&id))
            .unwrap_or(false)
    }

    pub(super) fn loop_follow_of(&self, header: usize) -> Option<usize> {
        self.loop_follow.get(&header).copied().flatten()
    }

    pub(super) fn reachable_count(&self) -> usize {
        self.reachable.iter().filter(|r| **r).count()
    }
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

fn immediate_post_dominators(succs: &[Vec<usize>], reachable: &[bool]) -> Vec<Option<usize>> {
    let n = succs.len();
    let all: HashSet<usize> = (0..n).filter(|i| reachable[*i]).collect();
    let exits: Vec<usize> = (0..n)
        .filter(|i| reachable[*i] && succs[*i].is_empty())
        .collect();
    let mut pdom: Vec<HashSet<usize>> = (0..n)
        .map(|i| {
            if reachable[i] {
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
            if !reachable[u] || succs[u].is_empty() {
                continue;
            }
            let mut new: Option<HashSet<usize>> = None;
            for &s in &succs[u] {
                if !reachable[s] {
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


    // The immediate post-dominator is the nearest strict post-dominator. Nearer
    // nodes are post-dominated by more blocks, so the largest post-dominator set
    // wins: for A -> {B, C} -> D -> exit, both D and exit strictly post-dominate
    // A, and D is the follow node.
    (0..n)
        .map(|u| {
            if !reachable[u] {
                return None;
            }
            pdom[u]
                .iter()
                .copied()
                .filter(|p| *p != u)
                .max_by_key(|p| pdom[*p].len())
        })
        .collect()
}

type LoopInfo = (HashMap<usize, HashSet<usize>>, HashMap<usize, Option<usize>>);

fn natural_loops(
    succs: &[Vec<usize>],
    preds: &[Vec<usize>],
    dom: &[HashSet<usize>],
    ipdom: &[Option<usize>],
    reachable: &[bool],
) -> LoopInfo {
    let n = succs.len();
    let mut bodies: HashMap<usize, HashSet<usize>> = HashMap::new();
    for u in 0..n {
        if !reachable[u] {
            continue;
        }
        for &v in &succs[u] {
            // Back edge: the target dominates its source.
            if !dom[u].contains(&v) {
                continue;
            }
            let body = bodies.entry(v).or_insert_with(|| HashSet::from([v]));
            let mut stack = vec![u];
            while let Some(x) = stack.pop() {
                if !body.insert(x) {
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

    let mut follow = HashMap::new();
    for (h, body) in &bodies {
        // Where control lands on leaving the loop.
        let mut targets: Vec<usize> = body
            .iter()
            .flat_map(|&b| succs[b].iter().copied())
            .filter(|t| !body.contains(t))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        let chosen = match targets.as_slice() {
            [only] => Some(*only),
            // 805 of the loop nests in the sampled binary have more than one
            // exit block. The header's post-dominator is the one target every
            // exit eventually reaches, so it is the `break` destination; the
            // others are rendered in place inside the loop.
            [] => None,
            _ => ipdom[*h].filter(|f| !body.contains(f)),
        };
        follow.insert(*h, chosen);
    }

    (bodies, follow)
}
