//! Cue-based retrieval, ported from upstream's `epmem_process_query`: the interval walk.
//!
//! The cue becomes a graph of *literals*, one per cue WME. Going backwards through time, the walk
//! follows only the moments some cue WME's range starts or ends, keeps which literals are
//! satisfied and the score that gives, and remembers the best episode. It stops at the first
//! episode that satisfies every positive leaf and passes the graph match.
//!
//! Upstream's SQL cursors are [`Cursor`]s here: the rows, sorted the same way, and a position.
//! Upstream's dummy root queries are `SELECT ?`, which it runs once and resets, so each gives
//! one more row later on; [`Query::new`] builds those extra rows in. Its sets of literals and
//! edges are ordered by address; here by creation.

use super::{EpisodicMemory, GmOrdering, Range, OPEN};
use crate::symbol::SymbolId;
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap};
use alloc::vec::Vec;

/// Upstream `EPMEM_NODEID_BAD`.
pub(crate) const NODE_BAD: i64 = -1;

const NODE: usize = 0;
const EDGE: usize = 1;

/// Upstream `epmem_triple`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Triple {
    parent: i64,
    attr: i64,
    child: i64,
}

/// Upstream `epmem_literal`.
#[derive(Debug, Clone)]
pub(crate) struct Literal {
    /// The cue WME's identifier; `None` for the root.
    pub(crate) id_sym: Option<SymbolId>,
    pub(crate) value_sym: SymbolId,
    pub(crate) negative: bool,
    pub(crate) value_is_id: bool,
    pub(crate) is_leaf: bool,
    pub(crate) attr: i64,
    /// The value's hash, or [`NODE_BAD`] for an identifier, which any node can match.
    pub(crate) child: i64,
    pub(crate) weight: f64,
    pub(crate) parents: BTreeSet<usize>,
    pub(crate) children: BTreeSet<usize>,
    matches: BTreeSet<(i64, i64)>,
    values: BTreeMap<i64, i64>,
}

impl Literal {
    /// A literal with no relations or matches.
    pub(crate) fn new(id_sym: Option<SymbolId>, value_sym: SymbolId, negative: bool, value_is_id: bool, attr: i64, child: i64) -> Self {
        Self {
            id_sym,
            value_sym,
            negative,
            value_is_id,
            is_leaf: false,
            attr,
            child,
            weight: 0.0,
            parents: BTreeSet::new(),
            children: BTreeSet::new(),
            matches: BTreeSet::new(),
            values: BTreeMap::new(),
        }
    }
}

/// The cue, ready to be searched for.
#[derive(Debug, Default)]
pub(crate) struct Cue {
    /// The literals; the first is the root.
    pub(crate) literals: Vec<Literal>,
    pub(crate) leaves: BTreeSet<usize>,
    pub(crate) gm_ordering: Vec<usize>,
    /// Upstream `symbol_num_incoming`.
    pub(crate) num_incoming: BTreeMap<SymbolId, i64>,
}

/// What a search found.
#[derive(Debug, Default)]
pub(crate) struct Found {
    pub(crate) episode: u64,
    pub(crate) score: f64,
    pub(crate) perfect_score: f64,
    pub(crate) cardinality: i64,
    pub(crate) graph_matched: bool,
    /// Literal to (parent, child) node, for literals the graph match bound.
    pub(crate) bindings: BTreeMap<usize, (i64, i64)>,
}

/// An SQL cursor: rows of (row id, value, time).
#[derive(Debug, Clone, Default)]
struct Cursor {
    rows: Vec<(i64, i64, i64)>,
    position: usize,
}

impl Cursor {
    fn current(&self) -> (i64, i64, i64) {
        self.rows[self.position]
    }

    /// Moves to the next row, returning whether there is one.
    fn step(&mut self) -> bool {
        self.position += 1;
        self.position < self.rows.len()
    }
}

/// Upstream `epmem_pedge`: every WME row a literal could match under one parent.
#[derive(Debug)]
struct Pedge {
    triple: Triple,
    value_is_id: bool,
    literals: BTreeSet<usize>,
    cursor: Option<Cursor>,
    time: i64,
}

/// Upstream `epmem_uedge`: one WME row.
#[derive(Debug)]
struct Uedge {
    triple: Triple,
    activation_count: i64,
    pedges: BTreeSet<usize>,
    activated: bool,
}

/// Upstream `epmem_interval`: the times one uedge's ranges start, or end.
#[derive(Debug)]
struct Interval {
    uedge: usize,
    is_end_point: bool,
    times: Vec<i64>,
    position: usize,
}

struct Query<'a> {
    store: &'a EpisodicMemory,
    literals: Vec<Literal>,
    num_incoming: BTreeMap<SymbolId, i64>,
    symbol_node_count: BTreeMap<(SymbolId, i64), i64>,
    pedges: Vec<Pedge>,
    uedges: Vec<Uedge>,
    intervals: Vec<Interval>,
    pedge_cache: [BTreeMap<Triple, usize>; 2],
    uedge_cache: [BTreeMap<Triple, usize>; 2],
    /// (time, pedge)
    pedge_pq: BinaryHeap<(i64, usize)>,
    /// (time, is a start, interval). Upstream's comparator ranks ends below starts.
    interval_pq: BinaryHeap<(i64, bool, usize)>,
    after: i64,
    current_score: f64,
    current_cardinality: i64,
}

fn time_of(end: u64) -> i64 {
    if end == OPEN {
        i64::MAX
    } else {
        end as i64
    }
}

impl<'a> Query<'a> {
    /// Upstream's dummy edge and interval for the root literal. Both root queries give two rows
    /// (see the module docs): the edge's second row has time 0, the interval repeats `before`.
    fn new(store: &'a EpisodicMemory, cue: Cue, before: i64, after: i64) -> Self {
        let triple = Triple { parent: NODE_BAD, attr: NODE_BAD, child: 0 };
        let root_pedge = Pedge {
            triple,
            value_is_id: true,
            literals: BTreeSet::from([0]),
            cursor: Some(Cursor { rows: alloc::vec![(0, 0, i64::MAX), (0, 0, 0)], position: 0 }),
            time: i64::MAX,
        };
        let root_uedge = Uedge { triple, activation_count: 0, pedges: BTreeSet::new(), activated: false };
        let root_interval = Interval { uedge: 0, is_end_point: true, times: alloc::vec![before, before], position: 0 };
        let mut query = Self {
            store,
            literals: cue.literals,
            num_incoming: cue.num_incoming,
            symbol_node_count: BTreeMap::new(),
            pedges: alloc::vec![root_pedge],
            uedges: alloc::vec![root_uedge],
            intervals: alloc::vec![root_interval],
            pedge_cache: [BTreeMap::new(), BTreeMap::new()],
            uedge_cache: [BTreeMap::new(), BTreeMap::new()],
            pedge_pq: BinaryHeap::from([(i64::MAX, 0)]),
            interval_pq: BinaryHeap::from([(before, false, 0)]),
            after,
            current_score: 0.0,
            current_cardinality: 0,
        };
        query.pedge_cache[EDGE].insert(triple, 0);
        query.uedge_cache[EDGE].insert(triple, 0);
        query
    }

    /// Upstream's partial-edge queries.
    fn edge_rows(&self, literal: &Literal, parent: i64) -> Cursor {
        let attr = literal.attr as u64;
        let mut rows: Vec<(i64, i64, i64)> = Vec::new();
        if !literal.value_is_id {
            for (i, c) in self.store.constants.iter().enumerate() {
                if c.parent as i64 == parent && c.attr == attr && (literal.child == NODE_BAD || c.value_hash as i64 == literal.child) {
                    rows.push((i as i64 + 1, c.value_hash as i64, i64::MAX));
                }
            }
        } else {
            for (i, e) in self.store.identifiers.iter().enumerate() {
                let last = self.store.identifier_ranges[i].last().map_or(i64::MAX, |r| time_of(r.end));
                if e.parent as i64 == parent && e.attr == attr && (literal.child == NODE_BAD || e.child as i64 == literal.child) && self.after < last {
                    rows.push((i as i64 + 1, e.child as i64, last));
                }
            }
            rows.sort_by_key(|r| core::cmp::Reverse(r.2));
        }
        Cursor { rows, position: 0 }
    }

    /// Upstream `epmem_register_pedges`.
    fn register_pedges(&mut self, parent: i64, literal: usize) -> bool {
        let lit = &self.literals[literal];
        let triple = Triple { parent, attr: lit.attr, child: lit.child };
        let is_edge = usize::from(lit.value_is_id);
        let Some(&pedge) = self.pedge_cache[is_edge].get(&triple) else {
            let cursor = self.edge_rows(lit, parent);
            if cursor.rows.is_empty() {
                return false;
            }
            let time = cursor.current().2;
            let value_is_id = lit.value_is_id;
            self.pedges.push(Pedge { triple, value_is_id, literals: BTreeSet::from([literal]), cursor: Some(cursor), time });
            let pedge = self.pedges.len() - 1;
            self.pedge_pq.push((time, pedge));
            self.pedge_cache[is_edge].insert(triple, pedge);
            return true;
        };
        if self.pedges[pedge].literals.insert(literal) && !lit.is_leaf && lit.child == NODE_BAD {
            // An edge with no set value: the literal applies to all the edges seen so far.
            let mut created = false;
            let seen: Vec<(Triple, usize)> = self.uedge_cache[is_edge]
                .range(triple..)
                .take_while(|(t, _)| t.parent == triple.parent && t.attr == triple.attr)
                .map(|(t, u)| (*t, *u))
                .collect();
            let children: Vec<usize> = self.literals[literal].children.iter().copied().collect();
            for (child_triple, _) in seen {
                if child_triple.child != NODE_BAD && is_edge == EDGE {
                    for &child in &children {
                        created |= self.register_pedges(child_triple.child, child);
                    }
                }
            }
            return created;
        }
        true
    }

    /// Upstream `epmem_satisfy_literal`.
    fn satisfy(&mut self, literal: usize, parent: i64, child: i64) -> bool {
        let lit = &self.literals[literal];
        let parents_satisfied = match lit.id_sym {
            None => true,
            Some(id) => {
                // A node satisfies all incoming literals exactly when each counted it.
                let incoming = self.num_incoming.get(&id).copied().unwrap_or(0);
                self.symbol_node_count.get(&(id, parent)) == Some(&incoming)
            }
        };
        if !parents_satisfied {
            return false;
        }
        let lit = &mut self.literals[literal];
        lit.matches.insert((parent, child));
        if let Some(count) = lit.values.get_mut(&child) {
            *count += 1;
            return false;
        }
        lit.values.insert(child, 1);
        if lit.is_leaf {
            if lit.matches.len() == 1 {
                self.current_score += lit.weight;
                self.current_cardinality += if lit.negative { -1 } else { 1 };
                return true;
            }
            return false;
        }
        *self.symbol_node_count.entry((lit.value_sym, child)).or_insert(0) += 1;
        let mut changed_score = false;
        let children: Vec<usize> = lit.children.iter().copied().collect();
        for child_lit in children {
            let lit = &self.literals[child_lit];
            let cache = usize::from(lit.value_is_id);
            let triple = Triple { parent: child, attr: lit.attr, child: lit.child };
            if lit.child == NODE_BAD {
                let edges: Vec<(Triple, usize)> =
                    self.uedge_cache[cache].range(triple..).take_while(|(t, _)| t.parent == child && t.attr == triple.attr).map(|(t, u)| (*t, *u)).collect();
                for (t, uedge) in edges {
                    if self.uedges[uedge].activated && self.uedges[uedge].activation_count == 1 {
                        changed_score |= self.satisfy(child_lit, t.parent, t.child);
                    }
                }
            } else if let Some(&uedge) = self.uedge_cache[cache].get(&triple) {
                if self.uedges[uedge].activated && self.uedges[uedge].activation_count == 1 {
                    changed_score |= self.satisfy(child_lit, triple.parent, triple.child);
                }
            }
        }
        changed_score
    }

    /// Upstream `epmem_unsatisfy_literal`.
    fn unsatisfy(&mut self, literal: usize, parent: i64, child: i64) -> bool {
        let lit = &mut self.literals[literal];
        if !lit.matches.remove(&(parent, child)) {
            return false;
        }
        let count = lit.values.get_mut(&child).expect("a match has its value counted");
        *count -= 1;
        if *count != 0 {
            return false;
        }
        lit.values.remove(&child);
        if lit.is_leaf {
            if lit.matches.is_empty() {
                self.current_score -= lit.weight;
                self.current_cardinality -= if lit.negative { -1 } else { 1 };
                return true;
            }
            return false;
        }
        let matched = (lit.value_sym, child);
        let still_matched = !lit.matches.is_empty();
        let children: Vec<usize> = lit.children.iter().copied().collect();
        let count = self.symbol_node_count.get_mut(&matched).expect("counted when satisfied");
        *count -= 1;
        if *count == 0 {
            self.symbol_node_count.remove(&matched);
        }
        // Still satisfied: only the children matching under this node lose out. Otherwise, all.
        let mut changed_score = false;
        for child_lit in children {
            if still_matched {
                let first = self.literals[child_lit].matches.range((child, NODE_BAD)..).next().copied();
                if let Some((p, c)) = first.filter(|(p, _)| *p == child) {
                    changed_score |= self.unsatisfy(child_lit, p, c);
                }
            } else {
                let matches: Vec<(i64, i64)> = self.literals[child_lit].matches.iter().copied().collect();
                for (p, c) in matches {
                    changed_score |= self.unsatisfy(child_lit, p, c);
                    if self.literals[child_lit].matches.is_empty() {
                        break;
                    }
                }
            }
        }
        changed_score
    }

    /// Upstream's interval queries for a WME row, as (is end point, times); the empty ones left
    /// out. Times are in each query's order.
    fn interval_queries(ranges: &[Range], current: i64) -> Vec<(bool, Vec<i64>)> {
        let kind = |r: &&Range| if r.end == OPEN { 1 } else if r.start == r.end { 2 } else { 0 };
        let mut queries = Vec::new();
        for interval_type in 0..3 {
            let of_type: Vec<&Range> = ranges.iter().filter(|r| kind(r) == interval_type && r.start as i64 <= current).collect();
            let mut starts: Vec<i64> = of_type.iter().map(|r| r.start as i64 - 1).collect();
            starts.sort_unstable_by(|a, b| b.cmp(a));
            let mut ends: Vec<i64> = of_type.iter().map(|r| if r.end == OPEN { current } else { r.end as i64 }).collect();
            ends.sort_unstable_by(|a, b| b.cmp(a));
            if !starts.is_empty() {
                queries.push((false, starts));
                queries.push((true, ends));
            }
        }
        queries
    }

    /// Takes every edge whose time has come. Returns whether the score changed.
    fn process_pedges(&mut self, next_edge: i64, current_episode: i64) -> bool {
        let mut changed_score = false;
        while let Some(&(time, pedge)) = self.pedge_pq.peek() {
            if !(time == next_edge || time >= current_episode) {
                break;
            }
            self.pedge_pq.pop();
            let (row, child, _) = self.pedges[pedge].cursor.as_ref().expect("queued pedges have rows").current();
            let mut triple = self.pedges[pedge].triple;
            triple.child = child;
            let value_is_id = self.pedges[pedge].value_is_id;

            if value_is_id {
                let literals: Vec<usize> = self.pedges[pedge].literals.iter().copied().collect();
                for literal in literals {
                    let children: Vec<usize> = self.literals[literal].children.iter().copied().collect();
                    for child_lit in children {
                        self.register_pedges(triple.child, child_lit);
                    }
                }
            }

            let cache = usize::from(value_is_id);
            match self.uedge_cache[cache].get(&triple).copied() {
                None => {
                    let ranges: &[Range] = if value_is_id { &self.store.identifier_ranges[row as usize - 1] } else { &self.store.constant_ranges[row as usize - 1] };
                    let queries = Self::interval_queries(ranges, current_episode);
                    if !queries.is_empty() {
                        let uedge = self.uedges.len();
                        for (is_end_point, times) in queries {
                            let interval = self.intervals.len();
                            self.interval_pq.push((times[0], !is_end_point, interval));
                            self.intervals.push(Interval { uedge, is_end_point, times, position: 0 });
                        }
                        self.uedges.push(Uedge { triple, activation_count: 0, pedges: BTreeSet::from([pedge]), activated: false });
                        self.uedge_cache[cache].insert(triple, uedge);
                    }
                }
                Some(uedge) => {
                    self.uedges[uedge].pedges.insert(pedge);
                    if self.uedges[uedge].activated && self.uedges[uedge].activation_count == 1 {
                        let literals: Vec<usize> = self.pedges[pedge].literals.iter().copied().collect();
                        for literal in literals {
                            changed_score |= self.satisfy(literal, triple.parent, triple.child);
                        }
                    }
                }
            }

            let more = self.pedges[pedge].cursor.as_mut().is_some_and(Cursor::step);
            if more {
                let time = self.pedges[pedge].cursor.as_ref().expect("just stepped").current().2;
                self.pedges[pedge].time = time;
                self.pedge_pq.push((time, pedge));
            } else {
                self.pedges[pedge].cursor = None;
            }
        }
        changed_score
    }

    /// Takes every interval end point at or after `current_episode`.
    fn process_intervals(&mut self, current_episode: i64) -> bool {
        let mut changed_score = false;
        while let Some(&(time, _, interval)) = self.interval_pq.peek() {
            if time < current_episode {
                break;
            }
            self.interval_pq.pop();
            let uedge = self.intervals[interval].uedge;
            let triple = self.uedges[uedge].triple;
            let pedges: Vec<usize> = self.uedges[uedge].pedges.iter().copied().collect();
            let is_end_point = self.intervals[interval].is_end_point;
            if is_end_point {
                self.uedges[uedge].activated = true;
                self.uedges[uedge].activation_count += 1;
            } else {
                self.uedges[uedge].activated = false;
                self.uedges[uedge].activation_count -= 1;
            }
            if !is_end_point || self.uedges[uedge].activation_count == 1 {
                for pedge in pedges {
                    let literals: Vec<usize> = self.pedges[pedge].literals.iter().copied().collect();
                    for literal in literals {
                        changed_score |= if is_end_point { self.satisfy(literal, triple.parent, triple.child) } else { self.unsatisfy(literal, triple.parent, triple.child) };
                    }
                }
            }
            let i = &mut self.intervals[interval];
            i.position += 1;
            if let Some(&time) = i.times.get(i.position) {
                self.interval_pq.push((time, !i.is_end_point, interval));
            }
        }
        changed_score
    }

    /// Upstream `epmem_graph_match`.
    fn graph_match(&self, order: &[usize], bindings: &mut BTreeMap<usize, (i64, i64)>, bound: &mut [BTreeMap<i64, SymbolId>; 2]) -> bool {
        let Some((&literal, rest)) = order.split_first() else { return true };
        if bindings.contains_key(&literal) {
            return false;
        }
        let lit = &self.literals[literal];
        let kind = if lit.value_is_id { EDGE } else { NODE };
        let mut failed_parents: BTreeSet<i64> = BTreeSet::new();
        for &(parent_node, child_node) in &lit.matches {
            if failed_parents.contains(&parent_node) {
                continue;
            }
            if lit.parents.iter().any(|p| bindings.get(p).is_some_and(|b| b.1 != parent_node)) {
                failed_parents.insert(parent_node);
                continue;
            }
            if bound[kind].get(&child_node).is_some_and(|sym| *sym != lit.value_sym) {
                continue;
            }
            if lit.child != NODE_BAD && lit.child != child_node {
                continue;
            }
            if lit.children.iter().any(|c| bindings.get(c).is_some_and(|b| b.0 != child_node)) {
                continue;
            }
            bindings.insert(literal, (parent_node, child_node));
            bound[kind].insert(child_node, lit.value_sym);
            if self.graph_match(rest, bindings, bound) {
                return true;
            }
            bindings.remove(&literal);
            bound[kind].remove(&child_node);
        }
        false
    }
}

impl EpisodicMemory {
    /// The interval walk of upstream `epmem_process_query`. `before` and `after` are 0 when not
    /// given; `prohibits` holds episodes not to retrieve. `None` if no episode matched at all.
    pub(crate) fn search(&self, mut cue: Cue, mut prohibits: Vec<u64>, before: u64, after: u64, graph_match: bool) -> Option<Found> {
        prohibits.sort_unstable();
        let mut prohibits: Vec<i64> = prohibits.into_iter().map(|p| p as i64).collect();
        let perfect_score: f64 = cue.leaves.iter().filter(|l| !cue.literals[**l].negative).map(|l| cue.literals[*l].weight).sum();
        let perfect_cardinality = cue.leaves.iter().filter(|l| !cue.literals[**l].negative).count() as i64;
        let before = if before == 0 { self.time as i64 - 1 } else { before as i64 - 1 };
        let after = after as i64;
        let mut gm_ordering = core::mem::take(&mut cue.gm_ordering);
        let mut query = Query::new(self, cue, before, after);

        let mut current_episode = before;
        let mut best = Found { perfect_score, ..Found::default() };
        let mut has_best = false;
        while !query.pedge_pq.is_empty() && current_episode > after {
            let next_edge = query.pedge_pq.peek().expect("not empty").0;
            let mut changed_score = query.process_pedges(next_edge, current_episode);
            let next_edge = query.pedge_pq.peek().map_or(after, |top| top.0);

            while query.interval_pq.peek().is_some_and(|top| top.0 > next_edge) && current_episode > after {
                changed_score |= query.process_intervals(current_episode);
                let next_interval = query.interval_pq.peek().map_or(after, |top| top.0);
                let next_episode = next_edge.max(next_interval);

                while prohibits.last().is_some_and(|p| *p > current_episode) {
                    prohibits.pop();
                }
                while current_episode > next_episode && prohibits.last() == Some(&current_episode) {
                    current_episode -= 1;
                    prohibits.pop();
                }

                let (score, cardinality) = (query.current_score, query.current_cardinality);
                let better = !has_best || score > best.score;
                if current_episode > next_episode && changed_score && (better || (graph_match && score == best.score && !best.graph_matched)) {
                    if better {
                        best.episode = current_episode as u64;
                        best.score = score;
                        best.cardinality = cardinality;
                        has_best = true;
                    }
                    if cardinality == perfect_cardinality {
                        let mut matched = false;
                        if graph_match {
                            match self.settings.gm_ordering {
                                GmOrdering::Undefined => gm_ordering.sort_unstable(),
                                GmOrdering::Mcv => gm_ordering.sort_by_key(|l| query.literals[*l].matches.len()),
                                GmOrdering::Dfs => {}
                            }
                            best.bindings.clear();
                            let mut bound = [BTreeMap::new(), BTreeMap::new()];
                            matched = query.graph_match(&gm_ordering, &mut best.bindings, &mut bound);
                        }
                        if !graph_match || matched {
                            best.episode = current_episode as u64;
                            best.graph_matched = true;
                            current_episode = 0;
                        }
                    }
                }
                if current_episode == 0 {
                    break;
                }
                current_episode = next_episode;
            }
        }
        has_best.then_some(best)
    }
}
