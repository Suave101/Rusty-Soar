//! Episodic memory's side of the decision cycle, ported from upstream `episodic_memory.cpp`:
//! storing working memory as episodes, reading commands from each state's `^epmem.command` link,
//! and recalling episodes into `^epmem.result`.
//!
//! Upstream notes working-memory changes as WMEs enter and leave the Rete; here
//! [`SoarAgent::epmem_wme_added`] and [`SoarAgent::epmem_wme_removed`] do. An identifier keeps
//! its node for as long as WMEs hold it, and when an equivalent structure comes back, it gets
//! the old rows from a pool, so the same structure over time has the same rows.

use super::smem::{Aug, RECALL};
use super::{PreferenceRecord, SoarAgent};
use crate::epmem::query::{Cue, Literal, NODE_BAD};
use crate::epmem::{EpmemForce, EpmemTrigger, IdentifierRow, ConstantRow, Range, HASH_ACCEPTABLE, NODE_ROOT, OPEN};
use crate::pool::Layout;
use crate::preference::PreferenceType;
use crate::symbol::{SymbolData, SymbolId};
use crate::wm::{SupportType, WmeKey};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

/// A state's `^epmem` link identifiers.
#[derive(Debug, Clone, Copy)]
pub(super) struct EpmemLinks {
    /// `(S ^epmem E)`: the link identifier.
    pub(super) header: SymbolId,
    /// `(E ^command C)`: the command identifier.
    pub(super) command: SymbolId,
    /// `(E ^result R)`: the result identifier.
    pub(super) result: SymbolId,
}

/// A state's episodic-memory data (upstream `epmem_data`).
#[derive(Debug, Clone, Default)]
pub(super) struct EpmemGoal {
    /// The link WMEs; `None` for a state made without them.
    pub(super) links: Option<EpmemLinks>,
    /// The `(E ^present-id n)` WME.
    pub(super) time_wme: Option<WmeKey>,
    /// Newest output-link WME timetag seen.
    last_ol_time: u64,
    /// Newest command WME timetag seen.
    last_cmd_time: u64,
    /// Command WME count seen.
    last_cmd_count: u64,
    /// The episode last recalled, 0 for none.
    last_memory: u64,
    /// Status WMEs made for the current command (upstream `epmem_wmes`), removed when it changes.
    status: Vec<(SymbolId, SymbolId, SymbolId)>,
}

/// WMEs to install, as upstream's `symbol_triple_list`s.
#[derive(Debug, Default)]
struct Recall {
    /// The command's WMEs that the retrieval depends on (upstream `cue_wmes`).
    cue: Vec<Aug>,
    meta: Vec<(SymbolId, SymbolId, SymbolId)>,
    retrieval: Vec<(SymbolId, SymbolId, SymbolId)>,
}

/// A command, parsed (upstream `_epmem_respond_to_cmd_parse`).
#[derive(Debug, Default)]
struct Command {
    retrieve: Option<u64>,
    next: Option<SymbolId>,
    previous: Option<SymbolId>,
    query: Option<SymbolId>,
    neg_query: Option<SymbolId>,
    prohibit: Vec<u64>,
    before: u64,
    after: u64,
    path: u8,
}

/// Upstream `epmem_id_reservation`: an identifier row set aside for a WME whose value is known,
/// so that a WME with an unknown value can't take it first.
struct Reservation {
    row: Option<u64>,
    pool: usize,
}

/// One walk over working memory (upstream's state across `_epmem_store_level` calls).
#[derive(Default)]
struct Walk {
    parents: VecDeque<(SymbolId, u64)>,
    visited: Vec<SymbolId>,
    reservations: BTreeMap<WmeKey, Reservation>,
    new_identifiers: Vec<SymbolId>,
    /// Constant rows that start now.
    nodes: VecDeque<u64>,
    /// Identifier rows that start now, with the value's long-term identifier.
    edges: VecDeque<(u64, u64)>,
}

impl<S: Layout> SoarAgent<S> {
    /// Upstream `epmem_enabled`.
    pub(super) fn epmem_enabled(&self) -> bool {
        self.epmem.settings.learning
    }

    /// The `add_wme_to_rete` part of upstream's episodic memory.
    pub(super) fn epmem_wme_added(&mut self, key: WmeKey, id: SymbolId, value: SymbolId) {
        if let Some(node) = self.epmem.nodes.get(&value).copied() {
            self.epmem.ref_counts.entry(node).or_default().insert(key);
        }
        if self.epmem.nodes.contains_key(&id) {
            self.epmem.wme_adds.insert(id);
        }
    }

    /// Upstream `_epmem_remove_wme`.
    fn epmem_remove_wme(&mut self, key: WmeKey, value: SymbolId) {
        let lti = self.symbols.lti(value);
        let row = self.epmem.wme_rows.remove(&key);
        if self.symbols.is_identifier(value) {
            if let Some(row) = row {
                self.epmem.edge_removals.insert((row, lti), true);
                if let Some((pool, child)) = self.epmem.replacement.remove(&row) {
                    self.epmem.pools[pool].push_front((child, row));
                }
            }
            if let Some(node) = self.epmem.nodes.get(&value).copied() {
                if let Some(refs) = self.epmem.ref_counts.get_mut(&node) {
                    // The top state's node is never given up.
                    if refs.remove(&key) && refs.is_empty() && node != NODE_ROOT {
                        self.epmem.id_removes.push_front(value);
                    }
                }
            }
        } else if let Some(row) = row {
            self.epmem.node_removals.insert(row, true);
        }
    }

    /// The `remove_wme_from_rete` part of upstream's episodic memory, with `_epmem_process_ids`.
    pub(super) fn epmem_wme_removed(&mut self, key: WmeKey, value: SymbolId) {
        self.epmem_remove_wme(key, value);
        while let Some(id) = self.epmem.id_removes.pop_front() {
            if self.epmem.nodes.remove(&id).is_some() {
                for w in self.augs_with_acceptable(id) {
                    self.epmem_remove_wme(w.key, w.value);
                }
            }
        }
    }

    /// Upstream `epmem_go`.
    pub(super) fn epmem_go(&mut self) {
        if !self.epmem.connected {
            let Some(top) = self.decider.goals.first() else { return };
            let top = top.id;
            self.epmem.connect(top);
        }
        self.epmem_consider_new_episode();
        self.epmem_respond_to_cmd();
    }

    /// Upstream `epmem_consider_new_episode`.
    fn epmem_consider_new_episode(&mut self) {
        let force = core::mem::take(&mut self.epmem.settings.force);
        let new_memory = match force {
            EpmemForce::Remember => true,
            EpmemForce::Ignore => false,
            EpmemForce::Off => match self.epmem.settings.trigger {
                EpmemTrigger::Dc => true,
                EpmemTrigger::None => false,
                EpmemTrigger::Output => {
                    let mut new_memory = false;
                    if let Some(link) = self.output_link() {
                        let newest = self.wm.iter().filter(|w| w.id == link).map(|w| w.timetag).max().unwrap_or(0);
                        let top = &mut self.decider.goals[0].epmem;
                        if newest > top.last_ol_time {
                            new_memory = true;
                            top.last_ol_time = newest;
                        }
                    }
                    new_memory
                }
            },
        };
        if new_memory {
            self.epmem_new_episode();
        }
    }

    /// Whether an attribute is in the `exclusions` setting.
    fn epmem_excluded(&self, attr: SymbolId) -> bool {
        match self.symbols.resolve(attr) {
            Some(SymbolData::String(s)) => self.epmem.settings.exclusions.contains(s.as_str()),
            _ => false,
        }
    }

    /// Upstream's hash of a WME's attribute: 0 for an identifier, which has none.
    fn epmem_attr_hash(&mut self, attr: SymbolId) -> u64 {
        if self.symbols.is_identifier(attr) {
            0
        } else {
            self.epmem.hash(attr)
        }
    }

    /// The attribute hash of an identifier-valued WME.
    fn epmem_edge_hash(&mut self, w: &Aug) -> u64 {
        if w.acceptable {
            HASH_ACCEPTABLE
        } else {
            self.epmem_attr_hash(w.attr)
        }
    }

    /// Upstream `_epmem_store_level`: encodes the WMEs of one identifier.
    fn epmem_store_level(&mut self, walk: &mut Walk, wmes: &[Aug], parent: u64, time: u64) {
        // Reserve rows for WMEs whose values are known, which keeps a WME with an unknown value
        // from taking them.
        for w in wmes {
            if self.epmem.wme_rows.contains_key(&w.key) {
                continue;
            }
            let Some(&value_node) = self.epmem.nodes.get(&w.value) else { continue };
            if !self.symbols.is_identifier(w.value) || self.epmem_excluded(w.attr) {
                continue;
            }
            let hash = self.epmem_edge_hash(w);
            let pool = self.epmem.pool_or_add(parent, hash);
            let found = self.epmem.pools[pool].iter().position(|(child, _)| *child == value_node);
            let row = found.and_then(|i| self.epmem.pools[pool].remove(i)).map(|(_, row)| row);
            walk.reservations.insert(w.key, Reservation { row, pool });
        }

        for w in wmes {
            if self.epmem.wme_rows.contains_key(&w.key) || self.epmem_excluded(w.attr) {
                continue;
            }
            if self.symbols.is_identifier(w.value) {
                self.epmem_store_identifier(walk, w, parent, time);
            } else {
                self.epmem_store_constant(walk, w, parent, time);
            }
        }
    }

    fn epmem_store_identifier(&mut self, walk: &mut Walk, w: &Aug, parent: u64, time: u64) {
        let lti = self.symbols.lti(w.value);
        let mut row = None;
        let pool;
        if let Some(&value_node) = self.epmem.nodes.get(&w.value) {
            if let Some(reservation) = walk.reservations.remove(&w.key) {
                pool = reservation.pool;
                if let Some(reserved) = reservation.row {
                    row = Some(reserved);
                    self.epmem.replacement.insert(reserved, (pool, value_node));
                }
            } else {
                // A value shared at this level needs an exact match.
                let hash = self.epmem_edge_hash(w);
                pool = self.epmem.pool_or_add(parent, hash);
                if let Some(i) = self.epmem.pools[pool].iter().position(|(child, _)| *child == value_node) {
                    let (_, found) = self.epmem.pools[pool].remove(i).expect("just found");
                    row = Some(found);
                    self.epmem.replacement.insert(found, (pool, value_node));
                }
            }
        } else {
            // An unknown value can take any row whose node nothing holds now.
            if !walk.new_identifiers.contains(&w.value) {
                walk.new_identifiers.push(w.value);
            }
            let hash = self.epmem_edge_hash(w);
            pool = self.epmem.pool_or_add(parent, hash);
            let refs = &self.epmem.ref_counts;
            let free = self.epmem.pools[pool].iter().position(|(child, _)| refs.get(child).is_none_or(|r| r.is_empty()));
            if let Some(i) = free {
                let (child, found) = self.epmem.pools[pool].remove(i).expect("just found");
                row = Some(found);
                self.epmem.nodes.insert(w.value, child);
                self.epmem.replacement.insert(found, (pool, child));
            }
        }

        match row {
            None => {
                let value_node = match self.epmem.nodes.get(&w.value) {
                    Some(node) => *node,
                    None => {
                        let node = self.epmem.next_id;
                        self.epmem.next_id += 1;
                        self.epmem.nodes.insert(w.value, node);
                        self.epmem.ref_counts.insert(node, Default::default());
                        node
                    }
                };
                let hash = self.epmem_edge_hash(w);
                self.epmem.identifiers.push(IdentifierRow { parent, attr: hash, child: value_node, min: time, closed: false });
                self.epmem.identifier_ranges.push(Vec::new());
                let new_row = self.epmem.identifiers.len() as u64;
                self.epmem.wme_rows.insert(w.key, new_row);
                self.epmem.replacement.insert(new_row, (pool, value_node));
                walk.edges.push_back((new_row, lti));
            }
            Some(row) => {
                self.epmem.wme_rows.insert(w.key, row);
                self.epmem.edge_removals.insert((row, lti), false);
                let record = &mut self.epmem.identifiers[row as usize - 1];
                if record.closed {
                    record.closed = false;
                    walk.edges.push_back((row, lti));
                }
            }
        }

        let value_node = self.epmem.nodes[&w.value];
        if walk.new_identifiers.contains(&w.value) {
            self.epmem.ref_counts.entry(value_node).or_default().insert(w.key);
        }
        if !walk.visited.contains(&w.value) {
            walk.parents.push_back((w.value, value_node));
        }
    }

    fn epmem_store_constant(&mut self, walk: &mut Walk, w: &Aug, parent: u64, time: u64) {
        let attr = self.epmem_attr_hash(w.attr);
        let value_hash = self.epmem.hash(w.value);
        match self.epmem.constant_ids.get(&(parent, attr, w.value)).copied() {
            None => {
                self.epmem.constants.push(ConstantRow { parent, attr, value: w.value, value_hash, min: time, closed: false });
                self.epmem.constant_ranges.push(Vec::new());
                let row = self.epmem.constants.len() as u64;
                self.epmem.constant_ids.insert((parent, attr, w.value), row);
                self.epmem.wme_rows.insert(w.key, row);
                walk.nodes.push_back(row);
            }
            Some(row) => {
                self.epmem.wme_rows.insert(w.key, row);
                self.epmem.node_removals.insert(row, false);
                let record = &mut self.epmem.constants[row as usize - 1];
                if record.closed {
                    record.closed = false;
                    walk.nodes.push_back(row);
                }
            }
        }
    }

    /// Upstream `epmem_new_episode`.
    fn epmem_new_episode(&mut self) {
        let time = self.epmem.time;
        let mut walk = Walk::default();
        for id in self.epmem.wme_adds.clone() {
            // A parent removed after its child was added is no longer known.
            let Some(&node) = self.epmem.nodes.get(&id) else { continue };
            walk.parents.push_back((id, node));
            while let Some((parent, parent_node)) = walk.parents.pop_front() {
                if walk.visited.contains(&parent) {
                    continue;
                }
                walk.visited.push(parent);
                let wmes = self.augs_with_acceptable(parent);
                self.epmem_store_level(&mut walk, &wmes, parent_node, time);
            }
        }

        while let Some(row) = walk.nodes.pop_front() {
            self.epmem.constant_ranges[row as usize - 1].push(Range { start: time, end: OPEN, lti: 0 });
            self.epmem.constants[row as usize - 1].min = time;
        }
        while let Some((row, lti)) = walk.edges.pop_front() {
            self.epmem.identifier_ranges[row as usize - 1].push(Range { start: time, end: OPEN, lti });
            self.epmem.identifiers[row as usize - 1].min = time;
        }

        for (row, remove) in core::mem::take(&mut self.epmem.node_removals) {
            if remove {
                let index = row as usize - 1;
                if let Some(range) = self.epmem.constant_ranges[index].last_mut() {
                    range.end = time - 1;
                }
                self.epmem.constants[index].closed = true;
            }
        }
        for ((row, lti), remove) in core::mem::take(&mut self.epmem.edge_removals) {
            if remove {
                let index = row as usize - 1;
                if let Some(range) = self.epmem.identifier_ranges[index].last_mut() {
                    range.end = time - 1;
                    range.lti = lti;
                }
                self.epmem.identifiers[index].closed = true;
            }
        }

        self.epmem.episodes.push(time);
        self.epmem.time = time + 1;

        let present = self.symbols.intern_int((time + 1) as i64);
        let attr = self.symbols.predefined.present_id;
        for goal in (0..self.decider.goals.len()).rev() {
            let Some(links) = self.decider.goals[goal].epmem.links else { continue };
            if let Some(old) = self.decider.goals[goal].epmem.time_wme.take() {
                self.decider.goals[goal].impasse_wmes.retain(|k| *k != old);
                self.remove_wme_quietly(old);
            }
            let key = self.insert_wme_with_flag(links.header, attr, present, false, SupportType::ISupport);
            self.decider.goals[goal].impasse_wmes.push(key);
            self.decider.goals[goal].epmem.time_wme = Some(key);
        }
        self.epmem.wme_adds.clear();
    }

    /// Upstream `_epmem_respond_to_cmd_parse`; `None` for a bad command.
    fn epmem_parse_command(&self, commands: &[Aug]) -> Option<Command> {
        let p = self.symbols.predefined;
        let mut c = Command::default();
        let integer = |w: &Aug| self.symbols.integer(w.value);
        let id = |w: &Aug| self.symbols.is_identifier(w.value);
        for w in commands {
            let query_path = c.path == 0 || c.path == 3;
            if w.attr == p.retrieve {
                c.retrieve = Some(integer(w).filter(|t| c.path == 0 && *t > 0)? as u64);
                c.path = 1;
            } else if w.attr == p.next || w.attr == p.previous {
                if !id(w) || c.path != 0 {
                    return None;
                }
                if w.attr == p.next {
                    c.next = Some(w.value);
                } else {
                    c.previous = Some(w.value);
                }
                c.path = 2;
            } else if w.attr == p.query {
                if !id(w) || !query_path || c.query.is_some() {
                    return None;
                }
                c.query = Some(w.value);
                c.path = 3;
            } else if w.attr == p.neg_query {
                if !id(w) || !query_path || c.neg_query.is_some() {
                    return None;
                }
                c.neg_query = Some(w.value);
                c.path = 3;
            } else if w.attr == p.before {
                let t = integer(w).filter(|_| query_path)? as u64;
                if c.before == 0 || t < c.before {
                    c.before = t;
                }
                c.path = 3;
            } else if w.attr == p.after {
                let t = integer(w).filter(|_| query_path)? as u64;
                c.after = c.after.max(t);
                c.path = 3;
            } else if w.attr == p.prohibit {
                c.prohibit.push(integer(w).filter(|_| query_path)? as u64);
                c.path = 3;
            } else {
                return None;
            }
        }
        (c.path != 0 && (c.path != 3 || c.query.is_some())).then_some(c)
    }

    /// Upstream `epmem_respond_to_cmd`.
    fn epmem_respond_to_cmd(&mut self) {
        let p = self.symbols.predefined;
        let mut do_wm_phase = false;
        for goal in (0..self.decider.goals.len()).rev() {
            let Some(links) = self.decider.goals[goal].epmem.links else { continue };

            // Breadth-first over the whole command.
            let mut new_cue = false;
            let mut wme_count = 0u64;
            let mut commands: Option<Vec<Aug>> = None;
            let mut whole: Vec<Aug> = Vec::new();
            let mut visited: Vec<SymbolId> = Vec::new();
            let mut queue = VecDeque::from([links.command]);
            while let Some(parent) = queue.pop_front() {
                let wmes = if visited.contains(&parent) { Vec::new() } else { self.augs_with_acceptable(parent) };
                visited.push(parent);
                for w in &wmes {
                    wme_count += 1;
                    let state = &mut self.decider.goals[goal].epmem;
                    if w.timetag > state.last_cmd_time {
                        new_cue = true;
                        state.last_cmd_time = w.timetag;
                    }
                    if self.symbols.is_identifier(w.value) {
                        queue.push_back(w.value);
                    }
                }
                whole.extend(wmes.iter().copied());
                commands.get_or_insert(wmes);
            }
            let state = &mut self.decider.goals[goal].epmem;
            if state.last_cmd_count != wme_count {
                new_cue = true;
                state.last_cmd_count = wme_count;
            }
            if new_cue {
                self.epmem_clear_result(goal);
                do_wm_phase = true;
            }
            if !(new_cue && wme_count > 0) {
                continue;
            }

            let commands = commands.unwrap_or_default();
            let mut recall = Recall { cue: commands.clone(), ..Recall::default() };
            match self.epmem_parse_command(&commands) {
                Some(c) if c.path == 1 => {
                    self.epmem_install_memory(goal, c.retrieve.unwrap_or(0), &mut recall);
                    self.epmem.stats.ncbr += 1;
                }
                Some(c) if c.path == 2 => {
                    let last = self.decider.goals[goal].epmem.last_memory;
                    let memory = if last == 0 {
                        None
                    } else if c.next.is_some() {
                        self.epmem.next_episode(last)
                    } else {
                        self.epmem.previous_episode(last)
                    };
                    self.epmem_install_memory(goal, memory.unwrap_or(0), &mut recall);
                    if c.next.is_some() {
                        self.epmem.stats.nexts += 1;
                    } else {
                        self.epmem.stats.prevs += 1;
                    }
                    let outcome = if self.decider.goals[goal].epmem.last_memory == 0 { p.failure } else { p.success };
                    let target = c.next.or(c.previous).expect("on this path");
                    recall.meta.push((links.result, outcome, target));
                }
                Some(c) => {
                    recall.cue = whole;
                    self.epmem_process_query(goal, &c, &mut recall);
                    self.epmem.stats.cbr += 1;
                }
                None => recall.meta.push((links.result, p.status, p.bad_cmd)),
            }
            if !recall.meta.is_empty() || !recall.retrieval.is_empty() {
                self.epmem_install(goal, recall);
                do_wm_phase = true;
            }
        }
        if do_wm_phase {
            self.do_working_memory_phase();
        }
    }

    /// Upstream `epmem_build_dnf`: the literal of a cue WME, or `None` if its value is part of a
    /// loop. Cues are kept acyclic.
    fn epmem_build_dnf(&mut self, cue: &mut Cue, cache: &mut BTreeMap<WmeKey, usize>, visiting: &mut Vec<SymbolId>, w: &Aug, negative: bool) -> Option<usize> {
        if visiting.contains(&w.value) {
            return None;
        }
        if let Some(literal) = cache.get(&w.key) {
            return Some(*literal);
        }
        let attr = self.epmem_attr_hash(w.attr) as i64;
        let is_id = self.symbols.is_identifier(w.value);
        let child = if is_id { NODE_BAD } else { self.epmem.hash(w.value) as i64 };
        let literal = cue.literals.len();
        cue.literals.push(Literal::new(Some(w.id), w.value, negative, is_id, attr, child));
        let augs = if is_id { self.augs_with_acceptable(w.value) } else { Vec::new() };
        if augs.is_empty() {
            cue.literals[literal].is_leaf = true;
            cue.leaves.insert(literal);
        } else {
            let mut cycle = false;
            visiting.push(w.value);
            for aug in &augs {
                match self.epmem_build_dnf(cue, cache, visiting, aug, negative) {
                    Some(child) => {
                        cue.literals[child].parents.insert(literal);
                        cue.literals[literal].children.insert(child);
                    }
                    None => cycle = true,
                }
            }
            visiting.pop();
            if cycle && cue.literals[literal].children.is_empty() {
                return None;
            }
            *cue.num_incoming.entry(w.value).or_insert(0) += 1;
        }
        if !negative {
            cue.gm_ordering.insert(0, literal);
        }
        // Upstream: below a balance of 1, a literal weighs its cue WME's activation.
        let weight = if self.epmem.settings.balance >= 1.0 - 1.0e-8 {
            1.0
        } else {
            self.wme_activation(w.key, true).unwrap_or(crate::wma::ACTIVATION_NONE)
        };
        cue.literals[literal].weight = if negative { -weight } else { weight };
        cache.insert(w.key, literal);
        Some(literal)
    }

    /// Upstream `epmem_process_query`.
    fn epmem_process_query(&mut self, goal: usize, c: &Command, recall: &mut Recall) {
        let p = self.symbols.predefined;
        let result = self.decider.goals[goal].epmem.links.expect("checked by the caller").result;
        if c.before != 0 && c.after != 0 && c.before <= c.after {
            recall.meta.push((result, p.status, p.bad_cmd));
            return;
        }
        let pos_query = c.query.expect("on this path");

        let mut cue = Cue::default();
        cue.literals.push(Literal::new(None, pos_query, false, true, NODE_BAD, NODE_ROOT as i64));
        cue.num_incoming.insert(pos_query, 1);
        let mut cache = BTreeMap::new();
        let mut visiting: Vec<SymbolId> = alloc::vec![pos_query];
        visiting.extend(c.neg_query);
        for (root, negative) in [(Some(pos_query), false), (c.neg_query, true)] {
            let Some(root) = root else { continue };
            for w in self.augs_with_acceptable(root) {
                if let Some(child) = self.epmem_build_dnf(&mut cue, &mut cache, &mut visiting, &w, negative) {
                    // All first-level literals have the same identifier, whichever query they
                    // came from.
                    cue.literals[child].id_sym = Some(pos_query);
                    cue.literals[child].parents.insert(0);
                    cue.literals[0].children.insert(child);
                }
            }
        }

        let cue_size = cue.leaves.len() as i64;
        let values: Vec<(bool, SymbolId)> = cue.literals.iter().map(|l| (l.value_is_id, l.value_sym)).collect();
        let graph_match = self.epmem.settings.graph_match;
        let found = self.epmem.search(cue, c.prohibit.clone(), c.before, c.after, graph_match);
        let Some(found) = found else {
            recall.meta.push((result, p.failure, pos_query));
            if let Some(neg_query) = c.neg_query {
                recall.meta.push((result, p.failure, neg_query));
            }
            return;
        };

        let cue_size = self.symbols.intern_int(cue_size);
        recall.meta.push((result, p.cue_size, cue_size));
        let cardinality = self.symbols.intern_int(found.cardinality);
        recall.meta.push((result, p.match_cardinality, cardinality));
        let score = self.symbols.intern_float(found.score);
        recall.meta.push((result, p.match_score, score));
        let normalized = self.symbols.intern_float(found.score / found.perfect_score);
        recall.meta.push((result, p.normalized_match_score, normalized));
        recall.meta.push((result, p.success, pos_query));
        if let Some(neg_query) = c.neg_query {
            recall.meta.push((result, p.success, neg_query));
        }

        // Mapping node for each cue identifier, by the episode's node it was bound to.
        let mut mapped: BTreeMap<i64, SymbolId> = BTreeMap::new();
        if graph_match {
            let matched = self.symbols.intern_int(i64::from(found.graph_matched));
            recall.meta.push((result, p.graph_match, matched));
            if found.graph_matched {
                let level = self.decider.links.level(result).unwrap_or(goal + 1);
                let mapping = self.new_identifier('M', level);
                recall.meta.push((result, p.mapping, mapping));
                for (literal, (_, child)) in &found.bindings {
                    let (is_id, cue_sym) = values[*literal];
                    if is_id {
                        let node = self.new_identifier('N', level);
                        recall.meta.push((mapping, p.node, node));
                        recall.meta.push((node, p.cue, cue_sym));
                        mapped.insert(*child, node);
                    }
                }
            }
        }

        let ids = self.epmem_install_memory(goal, found.episode, recall);
        if found.graph_matched {
            for (child, node) in mapped {
                if let Some(id) = ids.get(&(child as u64)) {
                    recall.meta.push((node, p.retrieved, *id));
                }
            }
        }
    }

    /// Upstream `_epmem_install_id_wme`: the WME for an identifier edge, making the identifier
    /// the first time the node comes up.
    fn epmem_install_id_wme(&mut self, parent: SymbolId, attr: SymbolId, (child, lti): (u64, u64), ids: &mut BTreeMap<u64, SymbolId>, level: usize, recall: &mut Recall) {
        let value = match ids.get(&child) {
            Some(id) => *id,
            None => {
                let default = if lti != 0 { 'L' } else { 'E' };
                let letter = match self.symbols.resolve(attr) {
                    Some(SymbolData::String(s)) => s.chars().next().unwrap_or(default),
                    _ => default,
                };
                let id = self.new_identifier(letter, level);
                if lti != 0 && self.smem.contains(crate::smem::LtiId(lti)) {
                    self.symbols.set_lti(id, lti);
                }
                ids.insert(child, id);
                id
            }
        };
        recall.retrieval.push((parent, attr, value));
    }

    /// The attribute symbol of a row.
    fn epmem_attr(&mut self, hash: u64) -> Option<SymbolId> {
        if hash == HASH_ACCEPTABLE {
            return Some(self.symbols.intern_str("operator*"));
        }
        self.epmem.reverse_hash(hash)
    }

    /// Upstream `epmem_install_memory`: recalls an episode (none if `memory` is 0 or not stored)
    /// under `^result.retrieved`. Identifiers are made first and constants hung on them after.
    /// Returns the identifier made for each node.
    fn epmem_install_memory(&mut self, goal: usize, memory: u64, recall: &mut Recall) -> BTreeMap<u64, SymbolId> {
        let p = self.symbols.predefined;
        let result = self.decider.goals[goal].epmem.links.expect("checked by the caller").result;
        self.epmem.stats.ncb_wmes = 0;
        if memory == 0 || self.epmem.episodes.binary_search(&memory).is_err() {
            recall.meta.push((result, p.retrieved, p.no_memory));
            self.decider.goals[goal].epmem.last_memory = 0;
            return BTreeMap::new();
        }
        self.decider.goals[goal].epmem.last_memory = memory;
        let level = self.decider.links.level(result).unwrap_or(goal + 1);
        let header = self.new_identifier('R', level);
        recall.meta.push((result, p.retrieved, header));
        let memory_id = self.symbols.intern_int(memory as i64);
        recall.meta.push((result, p.memory_id, memory_id));
        let present = self.symbols.intern_int(self.epmem.time as i64);
        recall.meta.push((result, p.present_id, present));

        let mut ids: BTreeMap<u64, SymbolId> = BTreeMap::from([(NODE_ROOT, header)]);
        let mut count = 0;
        // A shared identifier's row can come before its parent's: such rows wait in `orphans`
        // until a pass makes no progress.
        let mut orphans: VecDeque<(u64, SymbolId, u64, u64)> = VecDeque::new();
        for (parent, attr, child, lti) in self.epmem.identifiers_at(memory) {
            let Some(attr) = self.epmem_attr(attr) else { continue };
            match ids.get(&parent).copied() {
                Some(parent) => {
                    self.epmem_install_id_wme(parent, attr, (child, lti), &mut ids, level, recall);
                    count += 1;
                }
                None => orphans.push_back((parent, attr, child, lti)),
            }
        }
        loop {
            let left = orphans.len();
            for _ in 0..left {
                let orphan = orphans.pop_front().expect("counted");
                match ids.get(&orphan.0).copied() {
                    Some(parent) => {
                        self.epmem_install_id_wme(parent, orphan.1, (orphan.2, orphan.3), &mut ids, level, recall);
                        count += 1;
                    }
                    None => orphans.push_back(orphan),
                }
            }
            if orphans.is_empty() || orphans.len() == left {
                break;
            }
        }
        for (parent, attr, value) in self.epmem.constants_at(memory) {
            let (Some(parent), Some(attr)) = (ids.get(&parent).copied(), self.epmem_attr(attr)) else { continue };
            recall.retrieval.push((parent, attr, value));
            count += 1;
        }
        self.epmem.stats.ncb_wmes = count;
        ids
    }

    /// Upstream `epmem_process_buffered_wmes`.
    fn epmem_install(&mut self, goal: usize, recall: Recall) {
        let goal_id = self.decider.goals[goal].id;
        let preference = |firing: Option<u64>| {
            move |(action, (id, attr, value)): (usize, (SymbolId, SymbolId, SymbolId))| PreferenceRecord {
                id,
                attr,
                value,
                preference_type: PreferenceType::Acceptable,
                o_supported: false,
                instantiation: None,
                production: Some(RECALL),
                match_goal: Some(goal_id),
                justification: None,
                firing,
                action: action as u32,
                seq: 0,
            }
        };
        self.decider.goals[goal].epmem.status.extend(recall.meta.iter().copied());
        let firing = self.chunk_record_recall(goal, &recall.cue, &recall.meta);
        self.assert_preferences(recall.meta.into_iter().enumerate().map(preference(firing)).collect());
        let firing = self.chunk_record_recall(goal, &recall.cue, &recall.retrieval);
        self.assert_preferences(recall.retrieval.into_iter().enumerate().map(preference(firing)).collect());
    }

    /// Upstream `epmem_clear_result`.
    fn epmem_clear_result(&mut self, goal: usize) {
        let status = core::mem::take(&mut self.decider.goals[goal].epmem.status);
        if !status.is_empty() {
            self.remove_preferences(|p| p.production == Some(RECALL) && status.contains(&(p.id, p.attr, p.value)));
        }
    }
}
