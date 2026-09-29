//! Structures a function's bytecode: from jumps and the flow stack to ifs and loops.
//!
//! The bytecode is first run symbolically. A state is an offset and the flow stack, and following
//! every jump, push and pop from the entry gives a graph whose nodes are the statements, waits and
//! conditions the code can reach, each with the stack it is reached with. So a Sequence's pins
//! read as straight code, a Pop reads as whatever it pops to, and the same offset reached with
//! two different stacks is two nodes, since it goes on to different places.
//!
//! A loop can only be entered at its start, but the graph may hold a cycle that code enters at
//! more than one node. Such a cycle is given one entry by copying: the code from each other entry
//! is copied up to the point where it reaches the first entry again, and the jumps from outside
//! the cycle go to the copy instead. The copy runs the same statements in the same order, and then
//! enters the cycle at its first entry. That is bounded by the same limit as the output.
//!
//! The graph is then written out as structured code. A cycle is a loop, whose exits break to the
//! first node every exit goes through. A condition is an if, whose arms end at the first node
//! every path from both arms goes through, and that node follows the if. What one arm alone
//! reaches is written inside that arm, even when another path elsewhere reaches it too, so a node
//! with several ways in may be written more than once. That is bounded: past a generous limit the
//! code is refused rather than copied without end.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};

use super::code::{Code, Flow, Insn};

type NodeId = usize;

/// The node that ends the function or the handler.
const END: NodeId = 0;
/// Deeper than Sequences nest: a flow stack that grows past it is a cycle pushing on every pass.
const MAX_STACK: usize = 64;
/// How many times the instructions' count the output may reach before its joins are refused.
const MAX_GROWTH: usize = 20;

/// Structures the code that runs from `entry`. With `entry` None, the code is a function's, run
/// from its first instruction with an empty flow stack. With `Some(offset)`, it is an event
/// graph's, entered at that offset with the flow stack its preamble (the instructions before its
/// `ComputedJump`) leaves.
pub fn structure<S: Clone, C: Clone>(code: &Code<S, C>, entry: Option<usize>) -> Result<Vec<Flow<S, C>>> {
    let (nodes, start) = Builder::build(code, entry)?;
    if start == END {
        return Ok(Vec::new());
    }
    let limit = MAX_GROWTH * code.insns.len().max(1);
    let (nodes, start) = single_entries(nodes, start, limit)?;
    let mut structurer = Structurer::new(nodes, start, limit)?;
    let ctx = Ctx { stops: HashMap::new(), cycle: None, depth: 0, tail: Tail::End, end_breaks: false };
    structurer.emit(start, &ctx, false)
}

/// A node of the graph: what runs at a state, and the states it goes on to.
#[derive(Clone)]
enum Node<S, C> {
    End,
    Stmt { at: usize, stmt: S, next: NodeId },
    Wait { at: usize, call: S, next: NodeId },
    Cond { at: usize, cond: C, then: NodeId, otherwise: NodeId },
}

impl<S, C> Node<S, C> {
    fn succs(&self) -> Vec<NodeId> {
        match self {
            Node::End => Vec::new(),
            Node::Stmt { next, .. } | Node::Wait { next, .. } => vec![*next],
            Node::Cond { then, otherwise, .. } => vec![*then, *otherwise],
        }
    }

    fn succs_mut(&mut self) -> Vec<&mut NodeId> {
        match self {
            Node::End => Vec::new(),
            Node::Stmt { next, .. } | Node::Wait { next, .. } => vec![next],
            Node::Cond { then, otherwise, .. } => vec![then, otherwise],
        }
    }

    /// Makes the node go on to `to` wherever it went on to `from`.
    fn redirect(&mut self, from: NodeId, to: NodeId) {
        for succ in self.succs_mut() {
            if *succ == from {
                *succ = to;
            }
        }
    }

    /// The offset of the instruction the node came from.
    fn at(&self) -> usize {
        match self {
            Node::End => usize::MAX,
            Node::Stmt { at, .. } | Node::Wait { at, .. } | Node::Cond { at, .. } => *at,
        }
    }
}

/// Runs the bytecode symbolically from the entry, making a node for each state it reaches.
struct Builder<'a, S, C> {
    code: &'a Code<S, C>,
    /// Each instruction's index by its offset.
    index: HashMap<usize, usize>,
    /// Whether the code is an event graph's.
    graph: bool,
    /// The flow stack an event graph's preamble leaves; empty for a function.
    preamble: Vec<usize>,
    states: HashMap<(usize, Vec<usize>), NodeId>,
    nodes: Vec<Option<Node<S, C>>>,
    /// Nodes made but not yet filled in, with their states.
    pending: Vec<(NodeId, usize, Vec<usize>)>,
}

impl<'a, S: Clone, C: Clone> Builder<'a, S, C> {
    /// The graph of the code from `entry`, and the node it starts at.
    fn build(code: &'a Code<S, C>, entry: Option<usize>) -> Result<(Vec<Node<S, C>>, NodeId)> {
        let index = code.insns.iter().enumerate().map(|(i, (at, _))| (*at, i)).collect::<HashMap<_, _>>();
        let mut preamble = Vec::new();
        let start = match entry {
            None => {
                if let Some((at, _)) = code.insns.iter().find(|(_, insn)| matches!(insn, Insn::ComputedJump)) {
                    unexpected!(at: *at, "an event graph's entry jump in a function");
                }
                match code.insns.first() {
                    Some((at, _)) => *at,
                    None => return Ok((vec![Node::End], END)),
                }
            }
            Some(at) => {
                let Some(jump) = code.insns.iter().position(|(_, insn)| matches!(insn, Insn::ComputedJump)) else {
                    unexpected!("an event graph with no entry jump");
                };
                for (offset, insn) in &code.insns[..jump] {
                    match insn {
                        Insn::Push(target) => preamble.push(*target),
                        _ => unexpected!(at: *offset, "an event graph whose preamble has more than pushes before its entry jump"),
                    }
                }
                if !index.contains_key(&at) {
                    unexpected!(at: at, "an entry where no instruction starts");
                }
                at
            }
        };
        let mut builder = Builder {
            code,
            index,
            graph: entry.is_some(),
            preamble: preamble.clone(),
            states: HashMap::new(),
            nodes: vec![Some(Node::End)],
            pending: Vec::new(),
        };
        let start = builder.resolve(start, preamble)?;
        while let Some((id, at, stack)) = builder.pending.pop() {
            let node = builder.fill(at, stack)?;
            builder.nodes[id] = Some(node);
        }
        let nodes = builder.nodes.into_iter().map(|node| node.expect("every pending node is filled")).collect();
        Ok((nodes, start))
    }

    fn insn(&self, at: usize) -> Result<&'a Insn<S, C>> {
        match self.index.get(&at) {
            Some(&i) => Ok(&self.code.insns[i].1),
            None => unexpected!(at: at, "a jump to where no instruction starts"),
        }
    }

    /// The offset of the instruction after the one at `at`.
    fn after(&self, at: usize) -> Result<usize> {
        match self.code.insns.get(self.index[&at] + 1) {
            Some((next, _)) => Ok(*next),
            None => unexpected!(at: at, "bytecode that ends without an End"),
        }
    }

    /// The node that runs from a state, following jumps, pushes and pops until an instruction that
    /// does something.
    fn resolve(&mut self, mut at: usize, mut stack: Vec<usize>) -> Result<NodeId> {
        for _ in 0..=self.code.insns.len() {
            match self.insn(at)? {
                Insn::Jump(target) => at = *target,
                Insn::Push(target) => {
                    if stack.len() >= MAX_STACK {
                        unexpected!(at: at, "a flow stack that grows without end: code after this push jumps back before it, so every pass pushes again");
                    }
                    stack.push(*target);
                    at = self.after(at)?;
                }
                Insn::Pop => match stack.pop() {
                    Some(target) => at = target,
                    None => return Ok(END),
                },
                Insn::Return => return Ok(END),
                Insn::ComputedJump => unexpected!(at: at, "code that reaches the event graph's entry jump"),
                Insn::End => unexpected!(at: at, "code that runs off the end of the bytecode"),
                Insn::Stmt(_) | Insn::Latent { .. } | Insn::JumpIfNot(..) | Insn::PopIfNot(_) => {
                    let key = (at, stack);
                    if let Some(&id) = self.states.get(&key) {
                        return Ok(id);
                    }
                    let id = self.nodes.len();
                    self.nodes.push(None);
                    self.pending.push((id, key.0, key.1.clone()));
                    self.states.insert(key, id);
                    return Ok(id);
                }
            }
        }
        unexpected!(at: at, "jumps that never reach a statement")
    }

    /// The node for a state at an instruction that does something.
    fn fill(&mut self, at: usize, stack: Vec<usize>) -> Result<Node<S, C>> {
        Ok(match self.insn(at)? {
            Insn::Stmt(stmt) => {
                let next = self.resolve(self.after(at)?, stack)?;
                Node::Stmt { at, stmt: stmt.clone(), next }
            }
            Insn::Latent { call, resume } => {
                if !self.graph {
                    unexpected!(at: at, "a wait in a function, which only an event graph can do");
                }
                if stack != self.preamble {
                    cannot!(at: at, "a wait inside a Sequence's first pin or a loop's body, since the code after it would run before the wait is over");
                }
                if !self.thread_ends(self.after(at)?, stack)? {
                    cannot!(at: at, "code that follows a wait in the same thread");
                }
                let next = self.resolve(*resume, self.preamble.clone())?;
                Node::Wait { at, call: call.clone(), next }
            }
            Insn::JumpIfNot(cond, target) => {
                let then = self.resolve(self.after(at)?, stack.clone())?;
                let otherwise = self.resolve(*target, stack)?;
                Node::Cond { at, cond: cond.clone(), then, otherwise }
            }
            Insn::PopIfNot(cond) => {
                let mut popped = stack.clone();
                let then = self.resolve(self.after(at)?, stack)?;
                let otherwise = match popped.pop() {
                    Some(target) => self.resolve(target, popped)?,
                    None => END,
                };
                Node::Cond { at, cond: cond.clone(), then, otherwise }
            }
            Insn::Jump(_) | Insn::Push(_) | Insn::Pop | Insn::Return | Insn::ComputedJump | Insn::End => {
                unreachable!("resolve stops only at instructions that do something")
            }
        })
    }

    /// Whether the code from a state only ends the thread, through jumps and pops, without doing
    /// anything.
    fn thread_ends(&self, mut at: usize, mut stack: Vec<usize>) -> Result<bool> {
        for _ in 0..=self.code.insns.len() {
            match self.insn(at)? {
                Insn::Jump(target) => at = *target,
                Insn::Pop => match stack.pop() {
                    Some(target) => at = target,
                    None => return Ok(true),
                },
                Insn::Return => return Ok(true),
                _ => return Ok(false),
            }
        }
        Ok(false)
    }
}

/// The graph with every cycle entered at one node only, its header, so that each is a loop. Where
/// a cycle is entered at more than one node, the entry a depth-first walk from the start reaches
/// first is its header. Another entry is given a copy of the code that runs from it within the
/// cycle up to the header, and the jumps into it from outside the cycle go to the copy instead.
/// The copy is outside the cycle and goes into it only at the header, so the cycle has one entry
/// fewer. Cycles inside a cycle, not through its header, are made single entry the same way, where
/// the header counts as outside them. When anything was copied, the graph comes back with only the
/// nodes the start reaches, in the same order, and the start's new id.
fn single_entries<S: Clone, C: Clone>(
    mut nodes: Vec<Node<S, C>>,
    start: NodeId,
    limit: usize,
) -> Result<(Vec<Node<S, C>>, NodeId)> {
    let mut copied = false;
    loop {
        let order = preorder(&nodes, start);
        let live = (0..nodes.len()).filter(|&node| order[node] != usize::MAX).collect::<Vec<_>>();
        let mut preds = vec![Vec::new(); nodes.len()];
        for &node in &live {
            for succ in nodes[node].succs() {
                preds[succ].push(node);
            }
        }
        let Some(Entries { header, entry, cycle }) = find_entries(&nodes, &preds, &order, start, &live) else {
            break;
        };
        // The code that runs from the entry within the cycle until it reaches the header.
        let mut body = vec![entry];
        let mut seen = HashSet::from([entry]);
        let mut work = vec![entry];
        while let Some(node) = work.pop() {
            for succ in nodes[node].succs() {
                if succ != header && cycle.contains(&succ) && seen.insert(succ) {
                    body.push(succ);
                    work.push(succ);
                }
            }
        }
        if nodes.len() + body.len() > limit {
            cannot!(
                at: nodes[header].at(),
                "a loop entered at more than one place, since copying its code to give it one entry would make it over {MAX_GROWTH} times its size"
            );
        }
        let copies = body.iter().enumerate().map(|(i, &node)| (node, nodes.len() + i)).collect::<HashMap<_, _>>();
        for &node in &body {
            let mut copy = nodes[node].clone();
            for (&from, &to) in &copies {
                copy.redirect(from, to);
            }
            nodes.push(copy);
        }
        for &pred in &preds[entry] {
            if !cycle.contains(&pred) {
                nodes[pred].redirect(entry, copies[&entry]);
            }
        }
        copied = true;
    }
    if !copied {
        return Ok((nodes, start));
    }
    // Drops the nodes nothing reaches any more, keeping the end first and the rest in order.
    let order = preorder(&nodes, start);
    let mut ids = vec![usize::MAX; nodes.len()];
    let mut next = 0;
    for (node, id) in ids.iter_mut().enumerate() {
        if node == END || order[node] != usize::MAX {
            *id = next;
            next += 1;
        }
    }
    let nodes = nodes
        .into_iter()
        .enumerate()
        .filter(|&(node, _)| ids[node] != usize::MAX)
        .map(|(_, mut node)| {
            for succ in node.succs_mut() {
                *succ = ids[*succ];
            }
            node
        })
        .collect();
    Ok((nodes, ids[start]))
}

/// Where a cycle entered at more than one node is: the entry that stays, another that is to be
/// given a copy, and the cycle's nodes.
struct Entries {
    header: NodeId,
    entry: NodeId,
    cycle: HashSet<NodeId>,
}

/// The first cycle among `within` entered at more than one node, looking inside each cycle that is
/// not. A node is an entry when a node outside the cycle goes on to it, or it is the start.
fn find_entries<S, C>(
    nodes: &[Node<S, C>],
    preds: &[Vec<NodeId>],
    order: &[usize],
    start: NodeId,
    within: &[NodeId],
) -> Option<Entries> {
    for component in components(nodes, within) {
        if component.len() == 1 && !nodes[component[0]].succs().contains(&component[0]) {
            continue;
        }
        let cycle = component.iter().copied().collect::<HashSet<_>>();
        let mut entries = component
            .iter()
            .copied()
            .filter(|&node| node == start || preds[node].iter().any(|pred| !cycle.contains(pred)))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|&node| order[node]);
        let header = entries[0];
        if let Some(&entry) = entries.get(1) {
            return Some(Entries { header, entry, cycle });
        }
        let inside = component.into_iter().filter(|&node| node != header).collect::<Vec<_>>();
        if let Some(found) = find_entries(nodes, preds, order, start, &inside) {
            return Some(found);
        }
    }
    None
}

/// Each node's place in a depth-first walk from the start, which takes a node's successors in
/// order as the structurer's walk does. A node the start does not reach has `usize::MAX`.
fn preorder<S, C>(nodes: &[Node<S, C>], start: NodeId) -> Vec<usize> {
    let mut order = vec![usize::MAX; nodes.len()];
    order[start] = 0;
    let mut count = 1;
    let mut stack = vec![(start, 0usize)];
    while let Some(&mut (node, ref mut next)) = stack.last_mut() {
        let succs = nodes[node].succs();
        if *next < succs.len() {
            let succ = succs[*next];
            *next += 1;
            if order[succ] == usize::MAX {
                order[succ] = count;
                count += 1;
                stack.push((succ, 0));
            }
        } else {
            stack.pop();
        }
    }
    order
}

/// The strongly connected components of the graph among `within`: the largest sets of nodes that
/// each reach every other without leaving `within`. A node on no cycle is a component of its own.
fn components<S, C>(nodes: &[Node<S, C>], within: &[NodeId]) -> Vec<Vec<NodeId>> {
    // Tarjan's algorithm: a node whose walk reaches nothing earlier still on the stack closes a
    // component, which is everything above it on the stack.
    let mut inside = vec![false; nodes.len()];
    for &node in within {
        inside[node] = true;
    }
    let mut index = vec![usize::MAX; nodes.len()];
    let mut low = vec![usize::MAX; nodes.len()];
    let mut on_stack = vec![false; nodes.len()];
    let mut stack = Vec::new();
    let mut count = 0;
    let mut out = Vec::new();
    for &root in within {
        if index[root] != usize::MAX {
            continue;
        }
        index[root] = count;
        low[root] = count;
        count += 1;
        stack.push(root);
        on_stack[root] = true;
        let mut walk = vec![(root, 0usize)];
        while let Some(&mut (node, ref mut next)) = walk.last_mut() {
            let succs = nodes[node].succs();
            if *next < succs.len() {
                let succ = succs[*next];
                *next += 1;
                if !inside[succ] {
                    continue;
                }
                if index[succ] == usize::MAX {
                    index[succ] = count;
                    low[succ] = count;
                    count += 1;
                    stack.push(succ);
                    on_stack[succ] = true;
                    walk.push((succ, 0));
                } else if on_stack[succ] {
                    low[node] = low[node].min(index[succ]);
                }
            } else {
                walk.pop();
                if let Some(&(parent, _)) = walk.last() {
                    low[parent] = low[parent].min(low[node]);
                }
                if low[node] == index[node] {
                    let mut component = Vec::new();
                    while let Some(member) = stack.pop() {
                        on_stack[member] = false;
                        component.push(member);
                        if member == node {
                            break;
                        }
                    }
                    component.sort_unstable();
                    out.push(component);
                }
            }
        }
    }
    out
}

/// A cycle in the graph: a natural loop, entered only at its header.
struct Cycle {
    header: NodeId,
    nodes: HashSet<NodeId>,
    /// The nodes outside the cycle that its nodes go on to, other than the end.
    exits: Vec<NodeId>,
}

/// What reaching a node means where code is being written.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// The node follows the if whose arm is being written, so the arm ends.
    Join,
    /// The node follows a loop, which is left. Loops are counted from the outermost, 0.
    Break(usize),
    /// The node is a loop's header, whose next pass starts.
    Continue(usize),
}

/// What falling off the end of the code being written does.
#[derive(Clone, Copy, PartialEq)]
enum Tail {
    /// Ends the function or the handler.
    End,
    /// Starts the loop's next pass.
    Continue,
    /// Goes on to the node that follows the if.
    Join,
}

#[derive(Clone)]
struct Ctx {
    stops: HashMap<NodeId, Stop>,
    /// The cycle whose body is being written.
    cycle: Option<usize>,
    /// How many loops the code is inside.
    depth: usize,
    tail: Tail,
    /// Whether the end is written as a break: the code is a loop's body, and nothing follows the
    /// loop but the end.
    end_breaks: bool,
}

struct Structurer<S, C> {
    nodes: Vec<Node<S, C>>,
    cycles: Vec<Cycle>,
    /// Each cycle by its header.
    headers: HashMap<NodeId, usize>,
    /// The offset the code starts at, which its errors name.
    entry: usize,
    /// How many items have been written, against the limit.
    written: usize,
    limit: usize,
}

impl<S: Clone, C: Clone> Structurer<S, C> {
    fn new(nodes: Vec<Node<S, C>>, start: NodeId, limit: usize) -> Result<Self> {
        let entry = nodes[start].at();
        let mut structurer = Structurer { nodes, cycles: Vec::new(), headers: HashMap::new(), entry, written: 0, limit };
        structurer.find_cycles(start)?;
        Ok(structurer)
    }

    fn at(&self, node: NodeId) -> usize {
        self.nodes[node].at()
    }

    /// The nodes reachable from `from`, not passing through `avoid`, and not going on past a sink.
    /// The starting nodes and the sinks reached are included.
    fn reach(&self, from: &[NodeId], avoid: Option<NodeId>, sinks: &HashSet<NodeId>) -> HashSet<NodeId> {
        let mut seen = HashSet::new();
        let mut work = Vec::new();
        for &node in from {
            if Some(node) != avoid && seen.insert(node) {
                work.push(node);
            }
        }
        while let Some(node) = work.pop() {
            if node == END || sinks.contains(&node) {
                continue;
            }
            for next in self.nodes[node].succs() {
                if Some(next) != avoid && seen.insert(next) {
                    work.push(next);
                }
            }
        }
        seen
    }

    /// Finds the graph's cycles from its back edges. One entered anywhere but at its header, which
    /// no loop can say, is refused, though copying has already given every cycle a single entry.
    fn find_cycles(&mut self, start: NodeId) -> Result<()> {
        // A depth-first walk, where an edge to a node still on the walk's stack is a back edge.
        let mut colour = vec![0u8; self.nodes.len()];
        let mut stack = vec![(start, 0usize)];
        colour[start] = 1;
        let mut back_edges = Vec::new();
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            let succs = self.nodes[node].succs();
            if *next < succs.len() {
                let succ = succs[*next];
                *next += 1;
                match colour[succ] {
                    0 => {
                        colour[succ] = 1;
                        stack.push((succ, 0));
                    }
                    1 => back_edges.push((node, succ)),
                    _ => {}
                }
            } else {
                colour[node] = 2;
                stack.pop();
            }
        }
        let mut preds = vec![Vec::new(); self.nodes.len()];
        for (node, kind) in self.nodes.iter().enumerate() {
            for succ in kind.succs() {
                preds[succ].push(node);
            }
        }
        let no_sinks = HashSet::new();
        let mut bodies: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
        for (from, header) in back_edges {
            if self.reach(&[start], Some(header), &no_sinks).contains(&from) {
                bail!("at offset {}: a loop entered at more than one place after every loop was given one entry, which is a fault in the lifter", self.at(header));
            }
            // The natural loop: the header, and every node that reaches the back edge without it.
            let body = bodies.entry(header).or_default();
            body.insert(header);
            let mut work = vec![from];
            while let Some(node) = work.pop() {
                if body.insert(node) {
                    work.extend(preds[node].iter().copied());
                }
            }
        }
        let mut headers = bodies.keys().copied().collect::<Vec<_>>();
        headers.sort_unstable();
        for header in headers {
            let nodes = bodies.remove(&header).expect("listed from the keys");
            let mut members = nodes.iter().copied().collect::<Vec<_>>();
            members.sort_unstable();
            let mut exits = Vec::new();
            for member in members {
                for succ in self.nodes[member].succs() {
                    if succ != END && !nodes.contains(&succ) && !exits.contains(&succ) {
                        exits.push(succ);
                    }
                }
            }
            self.headers.insert(header, self.cycles.len());
            self.cycles.push(Cycle { header, nodes, exits });
        }
        Ok(())
    }

    /// Whether a node is strictly inside a cycle, not as its header, that `outside` is not in. A
    /// path from `outside` into the cycle goes through its header first, so the header is the
    /// node to name, never one inside.
    fn buried(&self, node: NodeId, outside: NodeId) -> bool {
        self.cycles.iter().any(|cycle| cycle.nodes.contains(&node) && node != cycle.header && !cycle.nodes.contains(&outside))
    }

    /// Of some nodes that every path goes through, the first: one no other reaches. When they lie
    /// on a cycle, the lowest.
    fn first(&self, cuts: &[NodeId], sinks: &HashSet<NodeId>) -> Option<NodeId> {
        let reaches = cuts.iter().map(|&cut| self.reach(&[cut], None, sinks)).collect::<Vec<_>>();
        let firsts = cuts
            .iter()
            .enumerate()
            .filter(|&(i, cut)| !reaches.iter().enumerate().any(|(j, reach)| i != j && reach.contains(cut)))
            .map(|(_, &cut)| cut);
        firsts.min().or_else(|| cuts.iter().copied().min())
    }

    /// The node that follows an if: the first node that both arms reach and every path from either
    /// to a stop goes through, if there is one. A path that ends the function without reaching
    /// it is written with its own return.
    fn join(&self, from: NodeId, then: NodeId, otherwise: NodeId, ctx: &Ctx) -> Option<NodeId> {
        let sinks = ctx.stops.keys().copied().collect::<HashSet<_>>();
        let from_then = self.reach(&[then], None, &sinks);
        let from_otherwise = self.reach(&[otherwise], None, &sinks);
        let mut shared = from_then
            .intersection(&from_otherwise)
            .copied()
            .filter(|&node| node != END && !sinks.contains(&node) && !self.buried(node, from))
            .collect::<Vec<_>>();
        shared.sort_unstable();
        let cuts = shared
            .into_iter()
            .filter(|&node| self.reach(&[then, otherwise], Some(node), &sinks).is_disjoint(&sinks))
            .collect::<Vec<_>>();
        self.first(&cuts, &sinks)
    }

    /// The node that follows a loop: the one its exits break to. Every exit's path to a stop must
    /// go through it, and of those nodes the one most exits reach is chosen, the first when they
    /// tie. An exit that only ends the function is written with its own return, and with no such
    /// node every exit is written inside the loop.
    fn follow(&self, cycle: usize, ctx: &Ctx) -> Option<NodeId> {
        let sinks = ctx.stops.keys().copied().collect::<HashSet<_>>();
        let cycle = &self.cycles[cycle];
        let reaches = cycle.exits.iter().map(|&exit| self.reach(&[exit], None, &sinks)).collect::<Vec<_>>();
        let mut shared = reaches
            .iter()
            .flatten()
            .copied()
            .filter(|&node| {
                node != END
                    && !cycle.nodes.contains(&node)
                    && !self.cycles.iter().any(|other| {
                        other.nodes.contains(&node) && node != other.header && !other.nodes.is_superset(&cycle.nodes)
                    })
            })
            .collect::<Vec<_>>();
        shared.sort_unstable();
        shared.dedup();
        let cuts = shared
            .into_iter()
            .filter(|&node| self.reach(&cycle.exits, Some(node), &sinks).is_disjoint(&sinks))
            .collect::<Vec<_>>();
        let count = |node: NodeId| reaches.iter().filter(|reach| reach.contains(&node)).count();
        let most = cuts.iter().map(|&cut| count(cut)).max()?;
        let best = cuts.into_iter().filter(|&cut| count(cut) == most).collect::<Vec<_>>();
        self.first(&best, &sinks)
    }

    fn write(&mut self, out: &mut Vec<Flow<S, C>>, item: Flow<S, C>) -> Result<()> {
        self.written += 1;
        if self.written > self.limit {
            cannot!(
                at: self.entry,
                "code that joins in ways loom can only say by repeating code, since the repeats would be over {MAX_GROWTH} times its size"
            );
        }
        out.push(item);
        Ok(())
    }

    /// Writes the code from `start` until it reaches a stop or the end. With `entering`, `start`
    /// is the header of the loop whose body this is, and is written rather than taken as a stop.
    fn emit(&mut self, start: NodeId, ctx: &Ctx, entering: bool) -> Result<Vec<Flow<S, C>>> {
        let mut out = Vec::new();
        let mut cur = start;
        let mut entering = entering;
        loop {
            if !entering {
                if let Some(&stop) = ctx.stops.get(&cur) {
                    // A loop is named by how many loops out from the innermost it is.
                    let out_to = |level: usize| ctx.depth - 1 - level;
                    match stop {
                        Stop::Join => {}
                        Stop::Break(level) => self.write(&mut out, Flow::Break(out_to(level)))?,
                        Stop::Continue(level) => {
                            if out_to(level) != 0 || ctx.tail != Tail::Continue {
                                self.write(&mut out, Flow::Continue(out_to(level)))?;
                            }
                        }
                    }
                    return Ok(out);
                }
                if cur == END {
                    match ctx.tail {
                        Tail::End => {}
                        _ if ctx.end_breaks => self.write(&mut out, Flow::Break(0))?,
                        _ => self.write(&mut out, Flow::Return)?,
                    }
                    return Ok(out);
                }
                if let Some(&cycle) = self.headers.get(&cur) {
                    if let Some(inner) = ctx.cycle.filter(|&inner| self.cycles[cycle].nodes.contains(&self.cycles[inner].header)) {
                        cannot!(
                            at: self.at(cur),
                            "a loop started again from inside the loop at offset {}",
                            self.at(self.cycles[inner].header)
                        );
                    }
                    let follow = self.follow(cycle, ctx);
                    // The loops around this one can still be left or gone on with, by name. An
                    // if's join outside the loop is only reached by leaving the loop.
                    let mut stops = ctx.stops.iter().filter(|(_, stop)| **stop != Stop::Join).map(|(&node, &stop)| (node, stop)).collect::<HashMap<_, _>>();
                    stops.insert(cur, Stop::Continue(ctx.depth));
                    if let Some(follow) = follow {
                        stops.insert(follow, Stop::Break(ctx.depth));
                    }
                    let inner = Ctx {
                        stops,
                        cycle: Some(cycle),
                        depth: ctx.depth + 1,
                        tail: Tail::Continue,
                        end_breaks: follow.is_none() && ctx.tail == Tail::End,
                    };
                    let body = self.emit(cur, &inner, true)?;
                    self.write(&mut out, Flow::Loop { body })?;
                    match follow {
                        Some(follow) => cur = follow,
                        None => return Ok(out),
                    }
                    continue;
                }
            }
            entering = false;
            match &self.nodes[cur] {
                Node::End => unreachable!("the end is handled above"),
                Node::Stmt { at, stmt, next } => {
                    let (item, next) = (Flow::Stmt { at: *at, stmt: stmt.clone() }, *next);
                    self.write(&mut out, item)?;
                    cur = next;
                }
                Node::Wait { at, call, next } => {
                    let (item, next) = (Flow::Wait { at: *at, call: call.clone() }, *next);
                    self.write(&mut out, item)?;
                    cur = next;
                }
                Node::Cond { cond, then, otherwise, .. } => {
                    let (cond, then, otherwise) = (cond.clone(), *then, *otherwise);
                    let join = self.join(cur, then, otherwise, ctx);
                    let mut arm = ctx.clone();
                    if let Some(join) = join {
                        arm.stops.insert(join, Stop::Join);
                        arm.tail = Tail::Join;
                    }
                    let then = self.emit(then, &arm, false)?;
                    let otherwise = self.emit(otherwise, &arm, false)?;
                    self.write(&mut out, Flow::If { cond, then, otherwise })?;
                    match join {
                        Some(join) => cur = join,
                        None => {
                            hoist(&mut out);
                            return Ok(out);
                        }
                    }
                }
            }
        }
    }
}

/// Whether code never falls off its end: it breaks, continues or returns, or is an if whose arms
/// both do.
fn terminates<S, C>(body: &[Flow<S, C>]) -> bool {
    match body.last() {
        Some(Flow::Break(_) | Flow::Continue(_) | Flow::Return) => true,
        Some(Flow::If { then, otherwise, .. }) => terminates(then) && terminates(otherwise),
        _ => false,
    }
}

/// When code ends with an if whose one arm never falls off its end, moves the other arm out
/// after the if, which falls off to the same place. `if c { long } else { break }` reads better
/// as `if !c { break }; long`.
fn hoist<S, C>(out: &mut Vec<Flow<S, C>>) {
    let Some(Flow::If { then, otherwise, .. }) = out.last_mut() else { return };
    let moved = match (terminates(then), terminates(otherwise)) {
        (true, false) if !otherwise.is_empty() => std::mem::take(otherwise),
        (false, true) if !then.is_empty() => std::mem::take(then),
        _ => return,
    };
    out.extend(moved);
}

#[cfg(test)]
mod tests {
    use super::super::code::{Code, Flow, Insn};
    use super::structure;

    type S = &'static str;
    type C = &'static str;
    type F = Flow<S, C>;

    fn code(insns: Vec<(usize, Insn<S, C>)>) -> Code<S, C> {
        Code { insns }
    }

    fn st(at: usize, stmt: S) -> F {
        Flow::Stmt { at, stmt }
    }

    fn wait(at: usize, call: S) -> F {
        Flow::Wait { at, call }
    }

    fn iff(cond: C, then: Vec<F>, otherwise: Vec<F>) -> F {
        Flow::If { cond, then, otherwise }
    }

    fn lp(body: Vec<F>) -> F {
        Flow::Loop { body }
    }

    /// Structured code as one line, for a failing test to show.
    fn show(body: &[F]) -> String {
        body.iter()
            .map(|item| match item {
                Flow::Stmt { stmt, .. } => stmt.to_string(),
                Flow::Wait { call, .. } => format!("wait {call}"),
                Flow::If { cond, then, otherwise } => format!("if {cond} {{{}}} else {{{}}}", show(then), show(otherwise)),
                Flow::Loop { body } => format!("loop {{{}}}", show(body)),
                Flow::Break(0) => "break".into(),
                Flow::Break(n) => format!("break {n}"),
                Flow::Continue(0) => "continue".into(),
                Flow::Continue(n) => format!("continue {n}"),
                Flow::Return => "return".into(),
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[track_caller]
    fn check(code: &Code<S, C>, entry: Option<usize>, expected: Vec<F>) {
        let got = structure(code, entry).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got, expected, "\n     got: {}\nexpected: {}", show(&got), show(&expected));
    }

    #[track_caller]
    fn fails(code: &Code<S, C>, entry: Option<usize>, message: &str) {
        match structure(code, entry) {
            Ok(got) => panic!("structured, as {}, but should have failed with {message:?}", show(&got)),
            Err(e) => {
                assert!(e.to_string().contains(message), "the error {e:?} should mention {message:?}");
                assert!(crate::lift::Refused::of(&e).is_some(), "{e:?} is a fault in the lifter, not a refusal");
            }
        }
    }

    #[test]
    fn straight_line() {
        let code = code(vec![(0, Insn::Stmt("a")), (10, Insn::Stmt("b")), (20, Insn::Return), (22, Insn::End)]);
        check(&code, None, vec![st(0, "a"), st(10, "b")]);
    }

    #[test]
    fn empty_function() {
        let code = code(vec![(0, Insn::Return), (2, Insn::End)]);
        check(&code, None, vec![]);
    }

    #[test]
    fn branch_both_wired_joins() {
        let code = code(vec![
            (0, Insn::Stmt("a")),
            (10, Insn::JumpIfNot("c", 40)),
            (20, Insn::Stmt("b")),
            (30, Insn::Jump(50)),
            (40, Insn::Stmt("d")),
            (50, Insn::Stmt("e")),
            (60, Insn::Return),
            (62, Insn::End),
        ]);
        check(&code, None, vec![st(0, "a"), iff("c", vec![st(20, "b")], vec![st(40, "d")]), st(50, "e")]);
    }

    #[test]
    fn branch_pop_if_not() {
        let code = code(vec![
            (0, Insn::Push(50)),
            (5, Insn::Stmt("a")),
            (10, Insn::PopIfNot("c")),
            (20, Insn::Stmt("b")),
            (30, Insn::Stmt("d")),
            (40, Insn::Pop),
            (50, Insn::Return),
            (52, Insn::End),
        ]);
        check(&code, None, vec![st(5, "a"), iff("c", vec![st(20, "b"), st(30, "d")], vec![])]);
    }

    #[test]
    fn sequence_of_two() {
        let code = code(vec![
            (0, Insn::Push(60)),
            (5, Insn::Push(40)),
            (10, Insn::Stmt("a")),
            (20, Insn::Stmt("b")),
            (30, Insn::Pop),
            (40, Insn::Stmt("d")),
            (50, Insn::Pop),
            (60, Insn::Return),
            (62, Insn::End),
        ]);
        check(&code, None, vec![st(10, "a"), st(20, "b"), st(40, "d")]);
    }

    #[test]
    fn sequence_of_three_in_a_branch() {
        let code = code(vec![
            (0, Insn::Push(120)),
            (5, Insn::JumpIfNot("c", 100)),
            (10, Insn::Push(40)),
            (20, Insn::Stmt("a")),
            (30, Insn::Pop),
            (40, Insn::Push(70)),
            (50, Insn::Stmt("b")),
            (60, Insn::Pop),
            (70, Insn::Stmt("d")),
            (80, Insn::Stmt("e")),
            (90, Insn::Pop),
            (100, Insn::Stmt("f")),
            (110, Insn::Pop),
            (120, Insn::Return),
            (122, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![iff("c", vec![st(20, "a"), st(50, "b"), st(70, "d"), st(80, "e")], vec![st(100, "f")])],
        );
    }

    #[test]
    fn pop_as_return_before_a_join() {
        // The false pin runs b and then joins the true pin's a; the true pin's thread ends at once.
        let code = code(vec![
            (0, Insn::Push(60)),
            (5, Insn::JumpIfNot("c", 30)),
            (10, Insn::Stmt("a")),
            (20, Insn::Pop),
            (30, Insn::Stmt("b")),
            (40, Insn::Jump(10)),
            (60, Insn::Return),
            (62, Insn::End),
        ]);
        check(&code, None, vec![iff("c", vec![], vec![st(30, "b")]), st(10, "a")]);
    }

    #[test]
    fn jump_to_return() {
        let code = code(vec![
            (0, Insn::JumpIfNot("c", 30)),
            (10, Insn::Stmt("a")),
            (20, Insn::Jump(40)),
            (30, Insn::Stmt("b")),
            (40, Insn::Return),
            (42, Insn::End),
        ]);
        check(&code, None, vec![iff("c", vec![st(10, "a")], vec![st(30, "b")])]);
    }

    #[test]
    fn arms_that_both_end_need_no_return() {
        let code = code(vec![
            (0, Insn::Push(60)),
            (5, Insn::JumpIfNot("c", 30)),
            (10, Insn::Stmt("a")),
            (20, Insn::Pop),
            (30, Insn::Stmt("b")),
            (40, Insn::Stmt("d")),
            (50, Insn::Pop),
            (60, Insn::Return),
            (62, Insn::End),
        ]);
        check(&code, None, vec![iff("c", vec![st(10, "a")], vec![st(30, "b"), st(40, "d")])]);
    }

    #[test]
    fn early_return_is_hoisted() {
        let code = code(vec![
            (0, Insn::Stmt("a")),
            (10, Insn::JumpIfNot("c", 30)),
            (20, Insn::Jump(60)),
            (30, Insn::Stmt("b")),
            (40, Insn::JumpIfNot("d", 60)),
            (50, Insn::Stmt("e")),
            (60, Insn::Return),
            (62, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![st(0, "a"), iff("c", vec![], vec![st(30, "b"), iff("d", vec![st(50, "e")], vec![])])],
        );
    }

    #[test]
    fn out_of_order_join_is_not_a_loop() {
        // An event graph with no preamble push, whose threads end by jumping to Return.
        let code = code(vec![
            (0, Insn::ComputedJump),
            (10, Insn::Stmt("log")),
            (44, Insn::Jump(125)),
            (49, Insn::Stmt("a")),
            (85, Insn::Stmt("b")),
            (120, Insn::Jump(10)),
            (125, Insn::Return),
            (127, Insn::End),
        ]);
        check(&code, Some(49), vec![st(49, "a"), st(85, "b"), st(10, "log")]);
        check(&code, Some(10), vec![st(10, "log")]);
    }

    #[test]
    fn shared_code_after_both_arms_is_hoisted() {
        // Both arms of the branch go on to d, laid out before the branch.
        let code = code(vec![
            (0, Insn::Push(90)),
            (5, Insn::Jump(30)),
            (10, Insn::Stmt("d")),
            (20, Insn::Pop),
            (30, Insn::JumpIfNot("c", 60)),
            (40, Insn::Stmt("a")),
            (50, Insn::Jump(10)),
            (60, Insn::Stmt("b")),
            (70, Insn::Stmt("e")),
            (80, Insn::Jump(10)),
            (90, Insn::Return),
            (92, Insn::End),
        ]);
        check(&code, None, vec![iff("c", vec![st(40, "a")], vec![st(60, "b"), st(70, "e")]), st(10, "d")]);
    }

    #[test]
    fn join_reached_with_different_stacks_is_written_twice() {
        // A Sequence's first pin and its second both go on to z, but the first pin's z goes on to
        // the second pin, so z is two states.
        let code = code(vec![
            (0, Insn::Push(80)),
            (5, Insn::Push(40)),
            (10, Insn::Stmt("a")),
            (20, Insn::Jump(60)),
            (40, Insn::Stmt("b")),
            (50, Insn::Jump(60)),
            (60, Insn::Stmt("z")),
            (70, Insn::Pop),
            (80, Insn::Return),
            (82, Insn::End),
        ]);
        check(&code, None, vec![st(10, "a"), st(60, "z"), st(40, "b"), st(60, "z")]);
    }

    /// The ForEach loop in a function from the compiler's layout: the header computes the length
    /// and tests the counter, the body pushes the increment, and the completed pin goes to the
    /// return.
    fn for_each() -> Code<S, C> {
        code(vec![
            (0, Insn::Push(928)),
            (154, Insn::Stmt("counter := 0")),
            (200, Insn::Stmt("len := length")),
            (301, Insn::Stmt("cond := counter < len")),
            (339, Insn::JumpIfNot("cond", 813)),
            (353, Insn::Stmt("index := counter")),
            (380, Insn::Push(854)),
            (385, Insn::Stmt("body")),
            (812, Insn::Pop),
            (813, Insn::Stmt("out := done")),
            (849, Insn::Jump(928)),
            (854, Insn::Stmt("tmp := counter + 1")),
            (896, Insn::Stmt("counter := tmp")),
            (923, Insn::Jump(200)),
            (928, Insn::Return),
            (930, Insn::End),
        ])
    }

    #[test]
    fn for_each_loop_in_a_function() {
        check(
            &for_each(),
            None,
            vec![
                st(154, "counter := 0"),
                lp(vec![
                    st(200, "len := length"),
                    st(301, "cond := counter < len"),
                    iff("cond", vec![], vec![Flow::Break(0)]),
                    st(353, "index := counter"),
                    st(385, "body"),
                    st(854, "tmp := counter + 1"),
                    st(896, "counter := tmp"),
                ]),
                st(813, "out := done"),
            ],
        );
    }

    #[test]
    fn loop_body_with_a_branch_and_a_return() {
        // The body's branch returns from the function on one pin and skips the rest on the other.
        let code = code(vec![
            (0, Insn::Push(200)),
            (5, Insn::Stmt("counter := 0")),
            (10, Insn::Stmt("cond := counter < len")),
            (20, Insn::JumpIfNot("cond", 150)),
            (30, Insn::Push(120)),
            (40, Insn::Stmt("found := item == x")),
            (50, Insn::JumpIfNot("found", 90)),
            (60, Insn::Stmt("out := item")),
            (70, Insn::Jump(200)),
            (90, Insn::Stmt("log")),
            (100, Insn::Pop),
            (120, Insn::Stmt("counter := counter + 1")),
            (130, Insn::Jump(10)),
            (150, Insn::Stmt("out := none")),
            (160, Insn::Pop),
            (200, Insn::Return),
            (202, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![
                st(5, "counter := 0"),
                lp(vec![
                    st(10, "cond := counter < len"),
                    iff("cond", vec![], vec![Flow::Break(0)]),
                    st(40, "found := item == x"),
                    iff("found", vec![st(60, "out := item"), Flow::Return], vec![]),
                    st(90, "log"),
                    st(120, "counter := counter + 1"),
                ]),
                st(150, "out := none"),
            ],
        );
    }

    #[test]
    fn nested_loops() {
        // An outer ForEach whose body is an inner ForEach, each exiting with PopIfNot to what
        // the stack holds: the return for the outer, the outer's increment for the inner.
        let code = code(vec![
            (0, Insn::Push(200)),
            (5, Insn::Stmt("i := 0")),
            (10, Insn::Stmt("ci := i < n")),
            (20, Insn::PopIfNot("ci")),
            (30, Insn::Push(150)),
            (40, Insn::Stmt("j := 0")),
            (50, Insn::Stmt("cj := j < m")),
            (60, Insn::PopIfNot("cj")),
            (70, Insn::Push(120)),
            (80, Insn::Stmt("body")),
            (90, Insn::Pop),
            (120, Insn::Stmt("j := j + 1")),
            (130, Insn::Jump(50)),
            (150, Insn::Stmt("i := i + 1")),
            (160, Insn::Jump(10)),
            (200, Insn::Return),
            (202, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![
                st(5, "i := 0"),
                lp(vec![
                    st(10, "ci := i < n"),
                    iff("ci", vec![], vec![Flow::Break(0)]),
                    st(40, "j := 0"),
                    lp(vec![
                        st(50, "cj := j < m"),
                        iff("cj", vec![], vec![Flow::Break(0)]),
                        st(80, "body"),
                        st(120, "j := j + 1"),
                    ]),
                    st(150, "i := i + 1"),
                ]),
            ],
        );
    }

    #[test]
    fn loop_in_an_event_graph() {
        // A ForEach whose completed pin is not wired: PopIfNot pops the preamble's return.
        let code = code(vec![
            (0, Insn::Push(300)),
            (5, Insn::ComputedJump),
            (10, Insn::Stmt("a")),
            (20, Insn::Pop),
            (30, Insn::Stmt("counter := 0")),
            (40, Insn::Stmt("cond := counter < len")),
            (50, Insn::PopIfNot("cond")),
            (60, Insn::Push(100)),
            (70, Insn::Stmt("body")),
            (80, Insn::Pop),
            (100, Insn::Stmt("counter := counter + 1")),
            (110, Insn::Jump(40)),
            (300, Insn::Return),
            (302, Insn::End),
        ]);
        check(
            &code,
            Some(30),
            vec![
                st(30, "counter := 0"),
                lp(vec![
                    st(40, "cond := counter < len"),
                    iff("cond", vec![], vec![Flow::Break(0)]),
                    st(70, "body"),
                    st(100, "counter := counter + 1"),
                ]),
            ],
        );
        check(&code, Some(10), vec![st(10, "a")]);
    }

    #[test]
    fn loop_started_again_from_its_completed_pin() {
        // The completed pin is wired back to the loop's start, so both arms start the next pass.
        let code = code(vec![
            (0, Insn::Push(200)),
            (5, Insn::Stmt("i := 0")),
            (10, Insn::Stmt("ci := i < n")),
            (20, Insn::JumpIfNot("ci", 100)),
            (30, Insn::Push(80)),
            (40, Insn::Stmt("body")),
            (50, Insn::Pop),
            (80, Insn::Stmt("i := i + 1")),
            (90, Insn::Jump(10)),
            (100, Insn::Stmt("reset")),
            (110, Insn::Jump(10)),
            (200, Insn::Return),
            (202, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![
                st(5, "i := 0"),
                lp(vec![
                    st(10, "ci := i < n"),
                    iff("ci", vec![st(40, "body"), st(80, "i := i + 1")], vec![st(100, "reset")]),
                ]),
            ],
        );
    }

    #[test]
    fn body_thread_that_ends_pops_back_into_the_loop() {
        // The body's branch jumps to code the completed pin also runs, but with the increment
        // still pushed: when that code's thread ends, the loop goes on. It is two states.
        let code = code(vec![
            (0, Insn::Push(200)),
            (5, Insn::Stmt("i := 0")),
            (10, Insn::Stmt("ci := i < n")),
            (20, Insn::JumpIfNot("ci", 150)),
            (30, Insn::Push(120)),
            (40, Insn::Stmt("found := item == x")),
            (50, Insn::JumpIfNot("found", 100)),
            (60, Insn::Stmt("hit")),
            (70, Insn::Jump(160)),
            (100, Insn::Pop),
            (120, Insn::Stmt("i := i + 1")),
            (130, Insn::Jump(10)),
            (150, Insn::Stmt("miss")),
            (160, Insn::Stmt("after")),
            (170, Insn::Pop),
            (200, Insn::Return),
            (202, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![
                st(5, "i := 0"),
                lp(vec![
                    st(10, "ci := i < n"),
                    iff("ci", vec![], vec![Flow::Break(0)]),
                    st(40, "found := item == x"),
                    iff("found", vec![st(60, "hit"), st(160, "after")], vec![]),
                    st(120, "i := i + 1"),
                ]),
                st(150, "miss"),
                st(160, "after"),
            ],
        );
    }

    #[test]
    fn inner_loop_breaking_to_the_outer_header_is_a_continue() {
        // Loops wired by hand, with no pushes: the inner loop leaves to the outer's start.
        let code = code(vec![
            (10, Insn::Stmt("a")),
            (20, Insn::JumpIfNot("c1", 100)),
            (30, Insn::Stmt("b")),
            (40, Insn::JumpIfNot("c2", 10)),
            (50, Insn::Jump(30)),
            (100, Insn::Return),
            (102, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![lp(vec![
                st(10, "a"),
                iff("c1", vec![], vec![Flow::Break(0)]),
                lp(vec![st(30, "b"), iff("c2", vec![], vec![Flow::Break(0)])]),
            ])],
        );
    }

    #[test]
    fn wait_then_resume_elsewhere() {
        let code = code(vec![
            (0, Insn::Push(1104)),
            (5, Insn::ComputedJump),
            (15, Insn::Stmt("a")),
            (58, Insn::Stmt("b")),
            (471, Insn::Pop),
            (472, Insn::Stmt("c")),
            (495, Insn::Stmt("d")),
            (593, Insn::Latent { call: "delay", resume: 15 }),
            (647, Insn::Pop),
            (648, Insn::Stmt("e := x")),
            (696, Insn::JumpIfNot("e", 874)),
            (710, Insn::Stmt("f")),
            (873, Insn::Pop),
            (874, Insn::Stmt("g := y")),
            (912, Insn::PopIfNot("g")),
            (922, Insn::Stmt("h")),
            (1103, Insn::Pop),
            (1104, Insn::Return),
            (1106, Insn::End),
        ]);
        check(&code, Some(472), vec![st(472, "c"), st(495, "d"), wait(593, "delay"), st(15, "a"), st(58, "b")]);
        check(
            &code,
            Some(648),
            vec![
                st(648, "e := x"),
                iff("e", vec![st(710, "f")], vec![st(874, "g := y"), iff("g", vec![st(922, "h")], vec![])]),
            ],
        );
    }

    #[test]
    fn wait_in_a_loop_of_its_own() {
        // Polling: the wait's resume point is the code before it.
        let code = code(vec![
            (0, Insn::Push(100)),
            (5, Insn::ComputedJump),
            (10, Insn::Stmt("poll")),
            (20, Insn::Latent { call: "delay", resume: 10 }),
            (30, Insn::Pop),
            (100, Insn::Return),
            (102, Insn::End),
        ]);
        check(&code, Some(10), vec![lp(vec![st(10, "poll"), wait(20, "delay")])]);
    }

    #[test]
    fn wait_inside_a_sequence_is_refused() {
        let code = code(vec![
            (0, Insn::Push(100)),
            (5, Insn::ComputedJump),
            (10, Insn::Push(40)),
            (20, Insn::Latent { call: "delay", resume: 60 }),
            (30, Insn::Pop),
            (40, Insn::Stmt("a")),
            (50, Insn::Pop),
            (60, Insn::Stmt("b")),
            (70, Insn::Pop),
            (100, Insn::Return),
            (102, Insn::End),
        ]);
        fails(&code, Some(10), "at offset 20: a wait inside a Sequence's first pin");
    }

    #[test]
    fn wait_followed_by_code_is_refused() {
        let code = code(vec![
            (0, Insn::Push(100)),
            (5, Insn::ComputedJump),
            (10, Insn::Latent { call: "delay", resume: 40 }),
            (20, Insn::Stmt("a")),
            (30, Insn::Pop),
            (40, Insn::Stmt("b")),
            (50, Insn::Pop),
            (100, Insn::Return),
            (102, Insn::End),
        ]);
        fails(&code, Some(10), "loom cannot express, at offset 10: code that follows a wait");
    }

    #[test]
    fn wait_in_a_function_is_refused() {
        let code = code(vec![
            (0, Insn::Latent { call: "delay", resume: 20 }),
            (10, Insn::Return),
            (20, Insn::Return),
            (22, Insn::End),
        ]);
        fails(&code, None, "at offset 0: a wait in a function");
    }

    #[test]
    fn loop_entered_twice_is_given_one_entry() {
        // Two nodes each wired to the other, entered at either: a cycle with no header. The walk
        // reaches a first, so a is the header, and the jump into b gets a copy of b that goes on
        // to a. Either way the code runs the same statements in the same order as before: a, b,
        // a, b and so on when c holds, and b, a, b, a and so on when it does not.
        let code = code(vec![
            (0, Insn::JumpIfNot("c", 40)),
            (10, Insn::Stmt("a")),
            (20, Insn::Stmt("b")),
            (30, Insn::Jump(10)),
            (40, Insn::Jump(20)),
            (50, Insn::Return),
            (52, Insn::End),
        ]);
        check(&code, None, vec![iff("c", vec![], vec![st(20, "b")]), lp(vec![st(10, "a"), st(20, "b")])]);
    }

    #[test]
    fn loop_entered_twice_with_an_exit() {
        // The loop runs a, then b when d holds, and leaves to x when it does not. It is entered at
        // a when c holds and at b when it does not. Entered at b, the copy runs b and then the
        // loop from a, as the original did.
        let code = code(vec![
            (0, Insn::JumpIfNot("c", 30)),
            (10, Insn::Stmt("a")),
            (20, Insn::JumpIfNot("d", 60)),
            (30, Insn::Stmt("b")),
            (40, Insn::Jump(10)),
            (60, Insn::Stmt("x")),
            (70, Insn::Return),
            (72, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![
                iff("c", vec![], vec![st(30, "b")]),
                lp(vec![st(10, "a"), iff("d", vec![], vec![Flow::Break(0)]), st(30, "b")]),
                st(60, "x"),
            ],
        );
    }

    #[test]
    fn inner_loop_entered_twice() {
        // The outer loop, from o, is entered only at o. Its body enters the inner loop at a when c
        // holds and at b when it does not, and the inner loop leaves to e, which goes back to o.
        // The inner loop is only a cycle once o is set aside, and o counts as outside it.
        let code = code(vec![
            (10, Insn::Stmt("o")),
            (20, Insn::JumpIfNot("co", 200)),
            (30, Insn::JumpIfNot("c", 60)),
            (40, Insn::Stmt("a")),
            (50, Insn::JumpIfNot("d", 90)),
            (60, Insn::Stmt("b")),
            (70, Insn::Jump(40)),
            (90, Insn::Stmt("e")),
            (100, Insn::Jump(10)),
            (200, Insn::Return),
            (202, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![lp(vec![
                st(10, "o"),
                iff("co", vec![], vec![Flow::Break(0)]),
                iff("c", vec![], vec![st(60, "b")]),
                lp(vec![st(40, "a"), iff("d", vec![], vec![Flow::Break(0)]), st(60, "b")]),
                st(90, "e"),
            ])],
        );
    }

    #[test]
    fn copies_run_to_the_header() {
        // Entered at b, the code runs b and then d before it reaches a, so both are copied.
        let code = code(vec![
            (0, Insn::JumpIfNot("c", 50)),
            (10, Insn::Stmt("a")),
            (20, Insn::Stmt("b")),
            (30, Insn::Stmt("d")),
            (40, Insn::Jump(10)),
            (50, Insn::Jump(20)),
            (60, Insn::Return),
            (62, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![iff("c", vec![], vec![st(20, "b"), st(30, "d")]), lp(vec![st(10, "a"), st(20, "b"), st(30, "d")])],
        );
    }

    #[test]
    fn loop_entered_everywhere_is_too_large_to_copy() {
        // A ring of statements, each entered from outside it. Every entry but the first is given
        // a copy of the ring from it round to the first, so the copies grow with the square of the
        // ring's length, while the code grows with its length. 150 statements pass the limit.
        let len = 150;
        let ring = 20 * len;
        let mut insns = Vec::new();
        for i in 0..len - 1 {
            insns.push((20 * i, Insn::JumpIfNot("c", 20 * (i + 1))));
            insns.push((20 * i + 10, Insn::Jump(ring + 10 * i)));
        }
        insns.push((20 * (len - 1), Insn::Jump(ring + 10 * (len - 1))));
        for i in 0..len {
            insns.push((ring + 10 * i, Insn::Stmt("r")));
        }
        insns.push((ring + 10 * len, Insn::Jump(ring)));
        insns.push((ring + 10 * len + 10, Insn::End));
        fails(
            &code(insns),
            None,
            &format!("loom cannot express, at offset {ring}: a loop entered at more than one place, since copying its code to give it one entry would make it over 20 times its size"),
        );
    }

    #[test]
    fn leaving_and_continuing_outer_loops_names_them() {
        // The innermost loop leaves both to the outer loop's start and to the middle loop's. Going
        // back to the outer loop's start is leaving the middle loop, which nothing follows.
        let code = code(vec![
            (10, Insn::Stmt("a")),
            (20, Insn::JumpIfNot("c1", 100)),
            (30, Insn::Stmt("b")),
            (40, Insn::JumpIfNot("c2", 10)),
            (50, Insn::JumpIfNot("c3", 20)),
            (55, Insn::Jump(30)),
            (100, Insn::Return),
            (102, Insn::End),
        ]);
        check(
            &code,
            None,
            vec![lp(vec![
                st(10, "a"),
                lp(vec![
                    iff("c1", vec![], vec![Flow::Return]),
                    lp(vec![st(30, "b"), iff("c2", vec![], vec![Flow::Break(1)]), iff("c3", vec![], vec![Flow::Continue(1)])]),
                ]),
            ])],
        );
    }

    #[test]
    fn jump_back_before_a_push_is_refused() {
        // The inner loop's body jumps to the outer loop's header with both increments pushed, so
        // every pass pushes again.
        let code = code(vec![
            (0, Insn::Push(200)),
            (5, Insn::Stmt("i := 0")),
            (10, Insn::Stmt("ci := i < n")),
            (20, Insn::PopIfNot("ci")),
            (30, Insn::Push(150)),
            (40, Insn::Stmt("j := 0")),
            (50, Insn::Stmt("cj := j < m")),
            (60, Insn::PopIfNot("cj")),
            (70, Insn::Push(120)),
            (80, Insn::Stmt("body")),
            (85, Insn::JumpIfNot("stop", 90)),
            (87, Insn::Jump(10)),
            (90, Insn::Pop),
            (120, Insn::Stmt("j := j + 1")),
            (130, Insn::Jump(50)),
            (150, Insn::Stmt("i := i + 1")),
            (160, Insn::Jump(10)),
            (200, Insn::Return),
            (202, Insn::End),
        ]);
        fails(&code, None, "unexpected input, at offset 70: a flow stack that grows without end");
    }

    #[test]
    fn missing_entry_jump_is_refused() {
        let code = code(vec![(0, Insn::Push(10)), (10, Insn::Return), (12, Insn::End)]);
        fails(&code, Some(10), "no entry jump");
    }

    #[test]
    fn jump_to_nowhere_is_refused() {
        let code = code(vec![(0, Insn::Jump(7)), (10, Insn::Return), (12, Insn::End)]);
        fails(&code, None, "offset 7");
    }
}
