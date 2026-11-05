//! Semantic memory's side of the decision cycle, ported from upstream `semantic_memory.cpp`
//! (`respond_to_cmd`), `smem_instance.cpp` and `smem_store.cpp`: reading commands from each
//! state's `^smem.command` link, storing working-memory structure, and recalling long-term
//! structure into `^smem.result`.
//!
//! Recalled structure enters working memory through preferences of an architectural
//! instantiation (upstream "Memory System Recall"), as upstream installs it. Status WMEs on
//! `^result` are removed when the command changes; recalled identifiers' WMEs stay until
//! their identifiers are no longer linked to a goal.
//!
//! Activation is recency, frequency or base-level; spreading activation is not ported.

use super::{PreferenceRecord, SoarAgent};
use crate::pool::Layout;
use crate::preference::PreferenceType;
use crate::smem::{CueElement, CueKind, LtiId, MathQuery, Number, SmemValue};
use crate::symbol::SymbolId;
use crate::wm::WmeKey;
use alloc::vec::Vec;

/// Name of the instantiation recalled structure is attributed to (upstream
/// `fake_instantiation_symbol`).
pub(super) const RECALL: &str = "Memory System Recall";

/// A state's `^smem` link identifiers.
#[derive(Debug, Clone, Copy)]
pub(super) struct SmemLinks {
    /// `(L ^command C)`: the command identifier.
    pub(super) command: SymbolId,
    /// `(L ^result R)`: the result identifier.
    pub(super) result: SymbolId,
}

/// A state's semantic-memory data (upstream `smem_data`).
#[derive(Debug, Clone, Default)]
pub(super) struct SmemGoal {
    /// The link WMEs; `None` for a state made without them (agents set up by hand).
    pub(super) links: Option<SmemLinks>,
    /// Newest command WME timetag seen, for the full and the store-only pass.
    last_cmd_time: [u64; 2],
    /// Command WME count seen, for the full and the store-only pass.
    last_cmd_count: [u64; 2],
    /// Status WMEs made for the current command (upstream `smem_wmes`), removed when it changes.
    status: Vec<(SymbolId, SymbolId, SymbolId)>,
}

/// What a command asks for (upstream `path_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Path {
    BlankSlate,
    Bad,
    Retrieve,
    Query,
    StoreNew,
    Store,
    Prohibit,
}

/// Command flags upstream declares once per pass rather than per state, so a value one state's
/// command sets carries to the states above it.
#[derive(Debug, Clone, Copy)]
struct PassFlags {
    /// `link-to-new-ltm`
    overwrite: bool,
    /// `link-to-ltm`
    link_to_ltm: bool,
}

/// WMEs to install, as upstream's `symbol_triple_list`s.
#[derive(Debug, Default)]
struct Recall {
    /// The command's WMEs that the retrieval depends on (upstream `cue_wmes`).
    cue: Vec<Aug>,
    status: Vec<(SymbolId, SymbolId, SymbolId)>,
    retrieved: Vec<(SymbolId, SymbolId, SymbolId)>,
}

/// One WME of an identifier, as upstream's `get_direct_augs_of_id` lists them.
#[derive(Debug, Clone, Copy)]
pub(super) struct Aug {
    pub(super) key: WmeKey,
    pub(super) id: SymbolId,
    pub(super) attr: SymbolId,
    pub(super) value: SymbolId,
    pub(super) timetag: u64,
    pub(super) acceptable: bool,
}

impl<S: Layout> SoarAgent<S> {
    /// Whether semantic memory runs (upstream `SMem_Manager::enabled`: `learning` is on).
    pub(super) fn smem_enabled(&self) -> bool {
        self.smem.settings.learning
    }

    /// The long-term identifier an identifier is linked to (upstream `id->LTI_ID`).
    pub fn lti_of(&self, id: SymbolId) -> Option<LtiId> {
        Some(self.symbols.lti(id)).filter(|&l| l != 0).map(LtiId)
    }

    fn set_lti(&mut self, id: SymbolId, lti: Option<LtiId>) {
        self.symbols.set_lti(id, lti.map_or(0, |l| l.0));
    }

    /// Upstream `get_direct_augs_of_id`: an identifier's WMEs, not acceptable preferences, in
    /// upstream's order: impasse WMEs, then input WMEs, then slot WMEs. Upstream lists slots
    /// newest first and each slot's WMEs newest first; slots are taken here to be as old as
    /// their oldest WME.
    fn direct_augs(&self, id: SymbolId) -> Vec<Aug> {
        self.augs(id, false)
    }

    /// Upstream `epmem_get_augs_of_id`: as [`Self::direct_augs`], with each slot's
    /// acceptable-preference WMEs after its other WMEs.
    pub(super) fn augs_with_acceptable(&self, id: SymbolId) -> Vec<Aug> {
        self.augs(id, true)
    }

    fn augs(&self, id: SymbolId, acceptable: bool) -> Vec<Aug> {
        let impasse: Vec<WmeKey> = self.decider.goals.iter().flat_map(|g| g.impasse_wmes.iter().copied()).collect();
        let mut wmes: Vec<(u8, u64, u64, Aug)> = Vec::new();
        for w in self.wm.iter().filter(|w| w.id == id && (acceptable || !w.acceptable)) {
            let group = if impasse.contains(&w.key) {
                0
            } else if w.support == crate::wm::SupportType::Input {
                1
            } else {
                2
            };
            wmes.push((group, 0, w.timetag, Aug { key: w.key, id: w.id, attr: w.attr, value: w.val, timetag: w.timetag, acceptable: w.acceptable }));
        }
        // Slot age: the oldest timetag among the slot's WMEs.
        let ages: Vec<(SymbolId, u64)> = wmes
            .iter()
            .filter(|w| w.0 == 2)
            .fold(Vec::new(), |mut ages: Vec<(SymbolId, u64)>, w| {
                match ages.iter_mut().find(|(a, _)| *a == w.3.attr) {
                    Some((_, age)) => *age = (*age).min(w.2),
                    None => ages.push((w.3.attr, w.2)),
                }
                ages
            });
        for w in &mut wmes {
            if w.0 == 2 {
                w.1 = ages.iter().find(|(a, _)| *a == w.3.attr).map_or(0, |(_, age)| *age);
            }
        }
        // Impasse and input WMEs in the order they were added; slots and their WMEs newest first.
        wmes.sort_by(|a, b| {
            a.0.cmp(&b.0).then(if a.0 == 2 { b.1.cmp(&a.1).then(a.3.acceptable.cmp(&b.3.acceptable)).then(b.2.cmp(&a.2)) } else { a.2.cmp(&b.2) })
        });
        wmes.into_iter().map(|w| w.3).collect()
    }

    /// Upstream `SMem_Manager::go` / `respond_to_cmd`: processes each state's command, bottom
    /// state first. `store_only` is the pass after each elaboration phase, which only stores.
    pub(super) fn smem_go(&mut self, store_only: bool) {
        let p = self.symbols.predefined;
        let slot = usize::from(store_only);
        let mut do_wm_phase = false;
        let mut flags = PassFlags { overwrite: false, link_to_ltm: true };
        for goal in (0..self.decider.goals.len()).rev() {
            let Some(links) = self.decider.goals[goal].smem.links else { continue };
            let command_id = links.command;

            // Breadth-first over the command and the identifiers of its `^query` and `^store`.
            let mut new_cue = false;
            let mut wme_count = 0u64;
            let mut commands: Vec<Aug> = Vec::new();
            let mut visited: Vec<SymbolId> = Vec::new();
            let mut queue = alloc::collections::VecDeque::from([(command_id, 0u32)]);
            while let Some((parent, level)) = queue.pop_front() {
                if visited.contains(&parent) {
                    continue;
                }
                visited.push(parent);
                let augs = self.direct_augs(parent);
                for w in &augs {
                    let counted = if store_only { level != 0 || w.attr == p.store } else { level != 0 || w.attr != p.store };
                    if !counted {
                        continue;
                    }
                    wme_count += 1;
                    let state = &mut self.decider.goals[goal].smem;
                    if w.timetag > state.last_cmd_time[slot] {
                        new_cue = true;
                        state.last_cmd_time[slot] = w.timetag;
                    }
                    if self.symbols.is_identifier(w.value) && level == 0 && (w.attr == p.query || w.attr == p.store) {
                        queue.push_back((w.value, level + 1));
                    }
                }
                if level == 0 && commands.is_empty() {
                    commands = augs;
                }
            }
            let state = &mut self.decider.goals[goal].smem;
            if state.last_cmd_count[slot] != wme_count {
                new_cue = true;
                state.last_cmd_count[slot] = wme_count;
            }
            if new_cue {
                self.smem_clear_result(goal);
                do_wm_phase = true;
            }
            if !(new_cue && wme_count > 0) {
                continue;
            }

            let mut recall = Recall::default();
            self.smem_command(goal, &commands, &mut flags, &mut recall);
            for w in commands.iter().filter(|w| w.attr == p.query || w.attr == p.neg_query) {
                recall.cue.extend(self.direct_augs(w.value));
            }
            recall.cue.extend(commands);
            if !recall.status.is_empty() || !recall.retrieved.is_empty() {
                self.smem_install(goal, recall, !flags.link_to_ltm);
                do_wm_phase = true;
            }
        }
        if do_wm_phase {
            self.do_working_memory_phase();
        }
    }

    /// Reads and carries out a command, adding the WMEs to install to `recall`.
    fn smem_command(&mut self, goal: usize, commands: &[Aug], flags: &mut PassFlags, recall: &mut Recall) {
        let p = self.symbols.predefined;
        let links = self.decider.goals[goal].smem.links.expect("checked by the caller");
        let result = links.result;
        let mut path = Path::BlankSlate;
        let (mut retrieve, mut query, mut negquery, mut math) = (None, None, None, None);
        let mut store: Vec<SymbolId> = Vec::new();
        let mut prohibit: Vec<SymbolId> = Vec::new();
        let mut depth = 1u64;
        for w in commands {
            if path == Path::Bad {
                break;
            }
            let is_id = self.symbols.is_identifier(w.value);
            let open = matches!(path, Path::BlankSlate | Path::Prohibit | Path::Query);
            path = if w.attr == p.retrieve {
                if is_id && path == Path::BlankSlate {
                    retrieve = Some(w.value);
                    Path::Retrieve
                } else {
                    Path::Bad
                }
            } else if w.attr == p.query {
                if is_id && open && query.is_none() {
                    query = Some(w.value);
                    Path::Query
                } else {
                    Path::Bad
                }
            } else if w.attr == p.neg_query {
                if is_id && open && negquery.is_none() {
                    negquery = Some(w.value);
                    Path::Query
                } else {
                    Path::Bad
                }
            } else if w.attr == p.prohibit {
                if is_id && open && self.lti_of(w.value).is_some() {
                    prohibit.push(w.value);
                    if path == Path::Query { Path::Query } else { Path::Prohibit }
                } else {
                    Path::Bad
                }
            } else if w.attr == p.math_query {
                if is_id && open && math.is_none() {
                    math = Some(w.value);
                    Path::Query
                } else {
                    Path::Bad
                }
            } else if w.attr == p.store {
                if is_id && matches!(path, Path::BlankSlate | Path::Store) {
                    store.push(w.value);
                    Path::Store
                } else {
                    Path::Bad
                }
            } else if w.attr == p.store_new {
                if is_id && matches!(path, Path::BlankSlate | Path::StoreNew) {
                    store.push(w.value);
                    Path::StoreNew
                } else {
                    Path::Bad
                }
            } else if w.attr == p.link_to_new_ltm {
                if (w.value == p.yes || w.value == p.no) && matches!(path, Path::BlankSlate | Path::StoreNew) {
                    flags.overwrite = w.value == p.yes;
                    path
                } else {
                    Path::Bad
                }
            } else if w.attr == p.link_to_ltm {
                if (w.value == p.yes || w.value == p.no) && matches!(path, Path::BlankSlate | Path::Retrieve | Path::Query) {
                    flags.link_to_ltm = w.value == p.yes;
                    path
                } else {
                    Path::Bad
                }
            } else if w.attr == p.depth {
                match self.symbols.resolve(w.value) {
                    Some(crate::symbol::SymbolData::Integer(n)) => {
                        depth = if *n > 0 { *n as u64 } else { 1 };
                        path
                    }
                    _ => Path::Bad,
                }
            } else {
                Path::Bad
            };
        }
        if (path == Path::Query && query.is_none()) || path == Path::BlankSlate {
            path = Path::Bad;
        }

        // Upstream `clear_instance_mappings`.
        self.smem_instance_ltis.clear();
        self.smem_instance_stis.clear();
        match path {
            Path::Retrieve => {
                let retrieve = retrieve.expect("set on this path");
                match self.lti_of(retrieve) {
                    None => recall.status.push((result, p.failure, retrieve)),
                    Some(lti) => {
                        recall.status.push((result, p.success, retrieve));
                        self.smem_install_memory(goal, lti, Some(retrieve), true, recall, depth);
                        self.smem.stats.retrievals += 1;
                    }
                }
            }
            Path::StoreNew | Path::Store => {
                for id in store {
                    let (use_lookup, overwrite) = if path == Path::StoreNew { (!flags.overwrite, flags.overwrite) } else { (false, false) };
                    self.smem_store(id, use_lookup, overwrite);
                    recall.status.push((result, p.success, id));
                    self.smem.stats.stores += 1;
                }
            }
            Path::Query => {
                let query = query.expect("checked above");
                let mut prohibited: Vec<LtiId> = prohibit.iter().filter_map(|id| self.lti_of(*id)).collect();
                for lti in &prohibited {
                    self.smem.prohibit(*lti);
                }
                match self.smem_process_query(query, negquery, math, &mut prohibited) {
                    Some(lti) => {
                        recall.status.push((result, p.success, query));
                        if let Some(negquery) = negquery {
                            recall.status.push((result, p.success, negquery));
                        }
                        let activate = self.smem.settings.activate_on_query;
                        self.smem_install_memory(goal, lti, None, activate, recall, depth);
                    }
                    None => {
                        recall.status.push((result, p.failure, query));
                        if let Some(negquery) = negquery {
                            recall.status.push((result, p.failure, negquery));
                        }
                    }
                }
                self.smem.stats.queries += 1;
            }
            Path::Prohibit => {
                for id in prohibit {
                    if let Some(lti) = self.lti_of(id) {
                        self.smem.prohibit(lti);
                    }
                }
            }
            Path::Bad => recall.status.push((result, p.bad_cmd, links.command)),
            Path::BlankSlate => unreachable!("turned into a bad command above"),
        }
    }

    /// Upstream `process_cue_wme`: adds the cue element for one cue WME, returning whether the
    /// cue can still match anything.
    fn smem_cue_element(&self, w: &Aug, positive: bool, math: Option<MathQuery>, cue: &mut Vec<CueElement>) -> bool {
        let Some(attr) = self.smem.hash(w.attr) else { return !positive };
        let kind = if !self.symbols.is_identifier(w.value) && math.is_none() {
            match self.smem.hash(w.value) {
                Some(value) => CueKind::Constant(value),
                None => return !positive,
            }
        } else {
            match self.symbols.is_identifier(w.value).then(|| self.lti_of(w.value)).flatten() {
                Some(lti) => CueKind::Lti(lti),
                None => CueKind::Attr,
            }
        };
        match self.smem.cue_weight(attr, kind) {
            Some(weight) => {
                cue.push(CueElement { weight, attr, kind, positive, math });
                true
            }
            None => !positive,
        }
    }

    /// Upstream `processMathQuery`: adds the math query's elements. Returns whether the cue is
    /// still good and whether it needs a full search (`^max` / `^min`).
    fn smem_math_query(&self, math: SymbolId, cue: &mut Vec<CueElement>) -> (bool, bool) {
        let p = self.symbols.predefined;
        let mut good = true;
        let mut full_search = false;
        let mut seen: Vec<SymbolId> = Vec::new();
        for w in self.direct_augs(math) {
            let types = self.direct_augs(w.value);
            if types.is_empty() {
                good = false;
                break;
            }
            for t in types {
                let number = self.smem_number(t.value);
                let query = if t.attr == p.less {
                    number.map(MathQuery::Less)
                } else if t.attr == p.greater {
                    number.map(MathQuery::Greater)
                } else if t.attr == p.less_or_equal {
                    number.map(MathQuery::LessOrEqual)
                } else if t.attr == p.greater_or_equal {
                    number.map(MathQuery::GreaterOrEqual)
                } else if t.attr == p.max || t.attr == p.min {
                    if seen.contains(&t.attr) {
                        None
                    } else {
                        seen.push(t.attr);
                        full_search = true;
                        Some(if t.attr == p.max { MathQuery::max() } else { MathQuery::min() })
                    }
                } else {
                    continue;
                };
                match query {
                    Some(query) => {
                        self.smem_cue_element(&w, true, Some(query), cue);
                    }
                    None => {
                        good = false;
                        break;
                    }
                }
            }
        }
        (good, good && full_search)
    }

    fn smem_number(&self, symbol: SymbolId) -> Option<Number> {
        number_of(&self.symbols, symbol)
    }

    /// Upstream `process_query` for a `^query` command: the long-term identifier retrieved.
    fn smem_process_query(&mut self, query: SymbolId, negquery: Option<SymbolId>, math: Option<SymbolId>, prohibit: &mut Vec<LtiId>) -> Option<LtiId> {
        let mut cue: Vec<CueElement> = Vec::new();
        let positive = self.direct_augs(query);
        let mut good = !positive.is_empty();
        for w in &positive {
            if good {
                good = self.smem_cue_element(w, true, None, &mut cue);
            }
        }
        let mut full_search = false;
        if let (Some(math), true) = (math, good) {
            (good, full_search) = self.smem_math_query(math, &mut cue);
        }
        if let Some(negquery) = negquery {
            for w in &self.direct_augs(negquery) {
                if good {
                    good = self.smem_cue_element(w, false, None, &mut cue);
                }
            }
        }
        if !good || cue.is_empty() {
            return None;
        }
        let mut cue = crate::smem::order_cue(cue);
        let symbols = &self.symbols;
        self.smem.query(&mut cue, prohibit, full_search, |s| number_of(symbols, s))
    }

    /// Upstream `get_current_iSTI_for_LTI`: the identifier recalling `lti` during this command,
    /// made at `level` the first time.
    fn smem_isti_for_lti(&mut self, lti: LtiId, level: usize, letter: char) -> SymbolId {
        if let Some((_, id)) = self.smem_instance_stis.iter().find(|(l, _)| *l == lti) {
            return *id;
        }
        let id = self.new_identifier(letter, level);
        self.set_lti(id, Some(lti));
        self.smem_instance_stis.push((lti, id));
        id
    }

    /// Upstream `get_current_LTI_for_iSTI`.
    fn smem_lti_for_isti(&mut self, id: SymbolId, use_lookup: bool, overwrite: bool) -> LtiId {
        let existing = self.lti_of(id);
        let lti = if use_lookup {
            match self.smem_instance_ltis.iter().find(|(s, _)| *s == id) {
                Some((_, lti)) => *lti,
                None => {
                    let lti = self.smem.add_lti();
                    self.smem_instance_ltis.push((id, lti));
                    lti
                }
            }
        } else {
            existing.unwrap_or_else(|| self.smem.add_lti())
        };
        if overwrite || existing.is_none() {
            self.set_lti(id, Some(lti));
        }
        lti
    }

    /// Upstream `STM_to_LTM` for `store_level` storage: stores an identifier's WMEs as its
    /// long-term identifier's edges, linking identifier values to long-term identifiers.
    fn smem_store(&mut self, id: SymbolId, use_lookup: bool, overwrite: bool) {
        let children = self.direct_augs(id);
        let lti = self.smem_lti_for_isti(id, use_lookup, overwrite);
        let mut edges = Vec::with_capacity(children.len());
        for w in children {
            let value = if self.symbols.is_identifier(w.value) {
                SmemValue::Lti(self.smem_lti_for_isti(w.value, use_lookup, overwrite))
            } else {
                SmemValue::Constant(w.value)
            };
            edges.push((w.attr, value));
        }
        self.smem_store_output();
        self.smem.store(lti, &edges);
    }

    /// Upstream `LTM_to_DB` writes one `@` to the trace for each identifier it stores.
    fn smem_store_output(&mut self) {
        self.output.push('@');
    }

    /// Upstream `CLI_add` (`smem --add`): adds the clauses' structure to semantic memory,
    /// clause by clause, appending to the edges identifiers already have and without
    /// activating them.
    ///
    /// Upstream collects each clause's identifiers in a set ordered by their addresses in
    /// memory, numbers the new ones in that order and stores them in that order; the port uses
    /// the order they were made in, which a fresh allocator usually gives too. Upstream also
    /// stores an identifier's attributes in the order of their symbols' addresses; the port
    /// uses the order the symbols were made in.
    pub(crate) fn smem_add(&mut self, clauses: &[crate::soar_parser::SmemAddClause]) {
        use crate::soar_parser::{SmemAddRef, SmemAddValue, SoarSymbol};
        enum Value {
            Constant(SymbolId),
            Node(usize),
        }
        /// Upstream `ltm_object`: an identifier and, while its clause is stored, its slots.
        struct Node {
            lti: Option<LtiId>,
            slots: Option<Vec<(SymbolId, Vec<Value>)>>,
        }
        fn push(slots: &mut Vec<(SymbolId, Vec<Value>)>, attr: SymbolId, value: Value) {
            match slots.iter_mut().find(|(a, _)| *a == attr) {
                Some((_, values)) => values.push(value),
                None => slots.push((attr, alloc::vec![value])),
            }
        }
        let symbol = |agent: &mut Self, s: &SoarSymbol| match s {
            SoarSymbol::Str(s) | SoarSymbol::Variable(s) => agent.symbols.intern_str(s),
            SoarSymbol::Int(i) => agent.symbols.intern_int(*i),
            SoarSymbol::Float(x) => agent.symbols.intern_float(*x),
        };
        // Upstream `str_to_LTMs`: nodes by name.
        let mut names: Vec<(alloc::string::String, usize)> = Vec::new();
        let mut nodes: Vec<Node> = Vec::new();
        // The name and number an `@` reference gives (an alias's identifier is made now).
        let named = |agent: &mut Self, r: &SmemAddRef| -> (alloc::string::String, Option<LtiId>) {
            match r {
                SmemAddRef::Variable(v) => (v.clone(), None),
                SmemAddRef::Lti(n) => (alloc::format!("@{}", n), Some(LtiId(*n))),
                SmemAddRef::Alias(a) => {
                    let lti = agent.smem.lti_with_alias(a);
                    (alloc::format!("@{}", lti.0), Some(lti))
                }
            }
        };
        for clause in clauses {
            let mut newbies: Vec<usize> = Vec::new();
            let root = nodes.len();
            nodes.push(Node { lti: None, slots: Some(Vec::new()) });
            let (name, lti) = named(self, &clause.id);
            nodes[root].lti = lti;
            for slot in &clause.slots {
                let mut parent = root;
                let mut attr = symbol(self, &slot.path[0]);
                for next in &slot.path[1..] {
                    let node = nodes.len();
                    nodes.push(Node { lti: None, slots: Some(Vec::new()) });
                    push(nodes[parent].slots.as_mut().expect("being parsed"), attr, Value::Node(node));
                    newbies.push(node);
                    parent = node;
                    attr = symbol(self, next);
                }
                for value in &slot.values {
                    let value = match value {
                        SmemAddValue::Constant(c) => Value::Constant(symbol(self, c)),
                        SmemAddValue::Ref(r) => {
                            let (key, lti) = named(self, r);
                            match names.iter().find(|(n, _)| *n == key) {
                                Some((_, node)) => Value::Node(*node),
                                None => {
                                    let node = nodes.len();
                                    nodes.push(Node { lti, slots: None });
                                    names.push((key, node));
                                    newbies.push(node);
                                    Value::Node(node)
                                }
                            }
                        }
                    };
                    push(nodes[parent].slots.as_mut().expect("being parsed"), attr, value);
                }
            }
            match names.iter().find(|(n, _)| *n == name) {
                None => {
                    names.push((name, root));
                    newbies.push(root);
                }
                Some(&(_, existing)) => {
                    let slots = nodes[root].slots.take().expect("being parsed");
                    match nodes[existing].slots.as_mut() {
                        None => nodes[existing].slots = Some(slots),
                        Some(target) => {
                            for (attr, values) in slots {
                                for value in values {
                                    push(target, attr, value);
                                }
                            }
                        }
                    }
                    newbies.push(existing);
                }
            }
            newbies.sort_unstable();
            newbies.dedup();
            for &node in &newbies {
                nodes[node].lti = Some(match nodes[node].lti {
                    None => self.smem.add_lti(),
                    Some(lti) if !self.smem.contains(lti) => self.smem.add_specific_lti(lti),
                    Some(lti) => lti,
                });
            }
            for &node in &newbies {
                let Some(mut slots) = nodes[node].slots.take() else { continue };
                slots.sort_by_key(|(attr, _)| *attr);
                let edges: Vec<(SymbolId, SmemValue)> = slots
                    .iter()
                    .flat_map(|(attr, values)| values.iter().map(move |v| (*attr, v)))
                    .map(|(attr, v)| match v {
                        Value::Constant(c) => (attr, SmemValue::Constant(*c)),
                        Value::Node(n) => (attr, SmemValue::Lti(nodes[*n].lti.expect("numbered above"))),
                    })
                    .collect();
                self.smem_store_output();
                self.smem.store_edges(nodes[node].lti.expect("numbered above"), &edges, false, false);
            }
        }
    }

    /// Upstream `install_memory` for working-memory installs: recalls `lti` onto `sti` (or a
    /// new identifier) and, below `depth` 1, the identifiers it links to.
    fn smem_install_memory(&mut self, goal: usize, lti: LtiId, sti: Option<SymbolId>, activate: bool, recall: &mut Recall, depth: u64) {
        let p = self.symbols.predefined;
        let result = self.decider.goals[goal].smem.links.expect("checked by the caller").result;
        let level = |agent: &Self, id: SymbolId| agent.decider.links.level(id).unwrap_or(goal + 1);
        let mut visited: Vec<LtiId> = Vec::new();
        let mut queue: alloc::collections::VecDeque<(SymbolId, LtiId, u64)> = alloc::collections::VecDeque::new();
        let mut current = Some((lti, sti, depth));
        let mut first = true;
        while let Some((lti, sti, depth)) = current.take() {
            let sti = match sti {
                Some(sti) => sti,
                None => {
                    let level = level(self, result);
                    self.smem_isti_for_lti(lti, level, 'L')
                }
            };
            if activate && first {
                let edges = self.smem.edge_count(lti);
                self.smem.activate(lti, true, edges);
            }
            recall.status.push((result, if first { p.retrieved } else { p.depth_retrieved }, sti));
            let sti_level = level(self, sti);
            for (attr, value) in self.smem.edges(lti) {
                let value = match value {
                    SmemValue::Constant(symbol) => symbol,
                    SmemValue::Lti(child) => {
                        let id = self.smem_isti_for_lti(child, sti_level, 'L');
                        if depth > 1 {
                            queue.push_back((id, child, depth - 1));
                        }
                        id
                    }
                };
                recall.retrieved.push((sti, attr, value));
            }
            visited.push(lti);
            first = false;
            while let Some((id, child, depth)) = queue.pop_front() {
                if !visited.contains(&child) {
                    visited.push(child);
                    current = Some((child, Some(id), depth));
                    break;
                }
            }
        }
    }

    /// Upstream `install_recall_buffer`: asserts the status WMEs (kept to be removed when the
    /// command changes) and the recalled WMEs as preferences of the state's recall.
    fn smem_install(&mut self, goal: usize, recall: Recall, strip_lti_links: bool) {
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
        self.decider.goals[goal].smem.status.extend(recall.status.iter().copied());
        let firing = self.chunk_record_recall(goal, &recall.cue, &recall.status);
        self.assert_preferences(recall.status.into_iter().enumerate().map(preference(firing)).collect());
        if strip_lti_links {
            for (id, _, value) in &recall.retrieved {
                self.set_lti(*id, None);
                if self.symbols.is_identifier(*value) {
                    self.set_lti(*value, None);
                }
            }
        }
        let firing = self.chunk_record_recall(goal, &recall.cue, &recall.retrieved);
        self.assert_preferences(recall.retrieved.into_iter().enumerate().map(preference(firing)).collect());
    }

    /// Upstream `clear_result`: removes the status WMEs of a state's last command.
    fn smem_clear_result(&mut self, goal: usize) {
        let status = core::mem::take(&mut self.decider.goals[goal].smem.status);
        if !status.is_empty() {
            self.remove_preferences(|p| p.production == Some(RECALL) && status.contains(&(p.id, p.attr, p.value)));
        }
    }
}

/// A symbol's value as a number, if it is one.
fn number_of(symbols: &crate::symbol::SymbolTable, symbol: SymbolId) -> Option<Number> {
    match symbols.resolve(symbol)? {
        crate::symbol::SymbolData::Integer(i) => Some(Number::Int(*i)),
        crate::symbol::SymbolData::Float(x) => Some(Number::Float(*x)),
        _ => None,
    }
}
