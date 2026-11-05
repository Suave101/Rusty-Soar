//! Chunking: learning a rule from a substate firing that makes a result, ported from upstream
//! `ebc_build.cpp`, `ebc_backtrace.cpp`, `ebc_unify.cpp`, `ebc_constraints.cpp`,
//! `ebc_variablize.cpp` and `ebc_merge.cpp`.
//!
//! Starting from the results, backtracing follows each firing's conditions. Conditions on
//! working memory at or above the superstate become the learned rule's conditions (the
//! *grounds*). Conditions on local structure are followed to the firing that made the WME, and
//! that link joins identities (see [`crate::learning`]). The learned rule is then an ordinary
//! parsed production, so it goes through the same ordering and checks as any other rule.
//!
//! A result that gets no rule gets a justification instead: the rule it would have been, kept
//! only as a firing, so that rules learned later can trace through it. What supports the result
//! is in `decide.rs`.

use super::decide::{FiredGoal, PreferenceRecord};
use super::smem::{Aug, RECALL};
use super::SoarAgent;
use crate::impasse::ImpasseType;
use crate::learning::{ChunkSettings, ChunkStats, Identities, LearnIn, SingletonElement};
use crate::pool::Layout;
use crate::preference::PreferenceType;
use crate::rete::Instantiation;
use crate::soar_parser::SoarPreference;
use crate::soar_parser::{
    SoarAction, SoarCondition, SoarConditionTests, SoarProduction, SoarProductionType, SoarRelation, SoarRhsValue, SoarSupport, SoarSymbol, SoarTest,
};
use crate::symbol::{SymbolData, SymbolId};
use crate::wm::WmeKey;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// A rule as written, with its conditions in Rete order.
#[derive(Debug)]
pub(super) struct RuleSource {
    pub(super) conditions: Vec<SoarCondition>,
    pub(super) actions: Vec<SoarAction>,
}

type Triple = (SymbolId, SymbolId, SymbolId);

/// A WME on a repair path: its key, triple, and whether it's an acceptable preference.
type PathWme = (WmeKey, SymbolId, SymbolId, SymbolId, bool);

/// The preference that put a condition's WME in working memory: the firing and its action.
type Trace = (u64, u32);

/// What chunking keeps of a firing (upstream's `instantiation`).
#[derive(Debug)]
pub(super) struct Firing {
    source: Arc<RuleSource>,
    rule: &'static str,
    /// The state it matched in.
    goal: SymbolId,
    /// The WME each condition level matched.
    levels: Vec<Option<Triple>>,
    keys: Vec<Option<WmeKey>>,
    traces: Vec<Option<Trace>>,
    /// Whether this is a memory retrieval (upstream `tested_LTM`).
    opaque: bool,
    /// Firings behind the operator selections and prohibits it tested (upstream `OSK_prefs`).
    osk: Vec<u64>,
    /// What a justification's analysis found, for rules learned through it.
    tested: Tested,
    /// How many justifications it's learned through (upstream `prod_naming_depth`).
    depth: u32,
}

/// Why a firing can't be learned from (upstream `tested_quiescence`, `tested_local_negation`).
#[derive(Debug, Clone, Copy, Default)]
struct Tested {
    quiescence: bool,
    local_negation: bool,
}

/// A state's operator slot (upstream `slot::OSK_prefs`, `instantiation_with_temp_OSK`).
#[derive(Debug, Default)]
struct SlotOsk {
    firings: Vec<u64>,
    proposal: Option<u64>,
}

/// The agent's chunking state.
#[derive(Debug, Default)]
pub struct Chunker {
    /// Settings.
    pub settings: ChunkSettings,
    /// Statistics.
    pub stats: ChunkStats,
    /// The rules learned, in order.
    pub rules: Vec<SoarProduction>,
    pub(super) sources: BTreeMap<&'static str, Arc<RuleSource>>,
    firings: BTreeMap<u64, Firing>,
    next_firing: u64,
    slots: BTreeMap<SymbolId, SlotOsk>,
    /// A selected operator's selection knowledge, by the firing that proposed it (upstream
    /// `OSK_proposal_prefs`).
    proposal_osk: BTreeMap<u64, Vec<u64>>,
    /// Firings standing for `^item` WMEs, by state and value.
    items: BTreeMap<(SymbolId, SymbolId), u64>,
    /// A learned rule's original rule and how many rules deep it is.
    origins: BTreeMap<&'static str, (&'static str, u32)>,
    /// Rules learned so far, without their names, to notice duplicates.
    learned: BTreeSet<String>,
    /// The decision the counters below are for.
    cycle: usize,
    chunks_this_cycle: u64,
    dupes_this_cycle: BTreeMap<&'static str, u64>,
    /// Singleton patterns by attribute: conditions on one such WME are the same condition.
    singletons: BTreeMap<String, (SingletonElement, SingletonElement)>,
    /// States flagged by `force-learn` and `dont-learn`.
    forced: Vec<SymbolId>,
    free: Vec<SymbolId>,
    /// States with a chunk learned in a state below (upstream `allow_bottom_up_chunks`).
    not_bottom: Vec<SymbolId>,
}

impl Chunker {
    /// Upstream `force-learn` and `dont-learn`.
    pub(super) fn flag_state(&mut self, state: SymbolId, force: bool) {
        let flagged = if force { &mut self.forced } else { &mut self.free };
        if !flagged.contains(&state) {
            flagged.push(state);
        }
    }

    /// Upstream `add_new_singleton` and `remove_singleton` (`pattern` of `None`). Returns what
    /// happened, for the agent's output.
    pub fn set_singleton(&mut self, attr: String, pattern: Option<(SingletonElement, SingletonElement)>) -> String {
        let architectural = ["operator", "superstate", "smem", "type", "impasse", "epmem"].contains(&attr.as_str());
        match pattern {
            Some((SingletonElement::State, _)) if architectural => format!("Soar cannot override the architectural singleton for {}.  Ignoring.", attr),
            None if architectural => format!("Soar cannot remove the architectural singleton for {}.  Ignoring.", attr),
            Some(pattern) => {
                let cleared = self.singletons.insert(attr.clone(), pattern).is_some();
                format!("{}Will unify conditions in super-states that match a WME that fits the pattern for {}.", if cleared { "Clearing previous singleton. " } else { "" }, attr)
            }
            None if self.singletons.remove(&attr).is_some() => format!("Removed. Will no longer unify conditions on {}.", attr),
            None => format!("Could not find pattern for {}.  Did not remove.", attr),
        }
    }

    /// How many firings are kept for backtracing.
    pub fn firing_count(&self) -> usize {
        self.firings.len()
    }
}

/// A result (upstream's list through `next_result`).
#[derive(Debug, Clone)]
struct Result_ {
    preference: PreferenceRecord,
}

/// What a justification keeps of the analysis that made it.
#[derive(Clone, Copy)]
struct Justification {
    firing: u64,
    goal: usize,
    tested: Tested,
    opaque: bool,
}

/// One analysis (upstream's `m_*` members).
#[derive(Default)]
struct Analysis {
    identities: Identities,
    results: Vec<Result_>,
    visited_ids: Vec<SymbolId>,
    visited_firings: BTreeSet<u64>,
    grounds: Vec<(u64, usize)>,
    locals: VecDeque<(u64, usize)>,
    negateds: Vec<(u64, usize)>,
    /// Non-equality tests on local conditions: (firing, level, field, test).
    constraints: Vec<(u64, usize, usize, SoarTest)>,
    tested_quiescence: bool,
    /// A justification it depends on tested a local negation.
    tested_local_negation: bool,
    /// The result depends on a memory retrieval.
    tested_recall: bool,
    /// The result depends on a firing no longer known.
    unsupported: bool,
}

/// The learned rule's variable names.
#[derive(Default)]
struct Names {
    of_set: BTreeMap<usize, String>,
    /// Identifiers the left side binds, for values that reached the right side unjoined.
    of_symbol: BTreeMap<SymbolId, String>,
    /// What each left-side variable matched.
    bound: BTreeMap<String, SymbolId>,
    counters: BTreeMap<char, u32>,
}

impl Names {
    fn fresh(&mut self, prefix: char) -> String {
        let n = self.counters.entry(prefix).or_insert(0);
        *n += 1;
        format!("<{}{}>", prefix, *n)
    }
}

fn fields(tests: &SoarConditionTests) -> [&SoarTest; 3] {
    [&tests.id, &tests.attr, &tests.value]
}

fn flatten(test: &SoarTest) -> Vec<&SoarTest> {
    match test {
        SoarTest::Conjunctive(tests) => tests.iter().flat_map(flatten).collect(),
        other => alloc::vec![other],
    }
}

fn is_equal(test: &SoarTest) -> bool {
    matches!(test, SoarTest::Relational(SoarRelation::Equal, _))
}

/// The variable a condition field binds. A field with no equality test gets a name of its own.
fn field_variable(test: &SoarTest, level: usize, field: usize) -> Option<String> {
    match test.eq_test() {
        Some(SoarSymbol::Variable(v)) => Some(v.clone()),
        Some(_) => None,
        None => Some(format!("#{}.{}", level, field)),
    }
}

fn value_variable(value: &SoarRhsValue) -> Option<&str> {
    match value {
        SoarRhsValue::Symbol(SoarSymbol::Variable(v)) => Some(v),
        _ => None,
    }
}

/// An action's identifier, attribute and value, as variables (`None` for constants and calls).
fn action_variables(action: &SoarAction) -> [Option<&str>; 3] {
    match action {
        SoarAction::Make { id, attr, value, .. } => {
            let id = if let SoarSymbol::Variable(v) = id { Some(v.as_str()) } else { None };
            [id, value_variable(attr), value_variable(value)]
        }
        SoarAction::FunctionCall(_) => [None, None, None],
    }
}

/// Upstream's functions that literalize their arguments: "only math functions", and `string`.
fn literalizing(name: &str) -> bool {
    matches!(name, "+" | "*" | "-" | "/" | "div" | "mod" | "abs" | "int" | "float" | "string")
}

/// The fields of an action as written, for function calls.
fn action_values(action: &SoarAction) -> [Option<&SoarRhsValue>; 3] {
    match action {
        SoarAction::Make { attr, value, .. } => [None, Some(attr), Some(value)],
        SoarAction::FunctionCall(_) => [None, None, None],
    }
}

fn reversed(relation: SoarRelation) -> SoarRelation {
    match relation {
        SoarRelation::Less => SoarRelation::Greater,
        SoarRelation::Greater => SoarRelation::Less,
        SoarRelation::LessOrEqual => SoarRelation::GreaterOrEqual,
        SoarRelation::GreaterOrEqual => SoarRelation::LessOrEqual,
        other => other,
    }
}

/// The variable of an equality test.
fn test_variable(test: &SoarTest) -> Option<&String> {
    match test.eq_test() {
        Some(SoarSymbol::Variable(v)) => Some(v),
        _ => None,
    }
}

/// Upstream `add_cond_to_tc`.
fn add_to_tc(condition: &SoarCondition, tc: &mut BTreeSet<String>) {
    if let SoarCondition::Positive(tests) = condition {
        tc.extend(test_variable(&tests.id).cloned());
        tc.extend(test_variable(&tests.value).cloned());
    }
}

/// Upstream `cond_is_in_tc`: a conjunctive negation is in if its conditions connect in some order.
fn in_tc(condition: &SoarCondition, tc: &BTreeSet<String>) -> bool {
    match condition {
        SoarCondition::Positive(tests) | SoarCondition::Negative(tests) => test_variable(&tests.id).is_some_and(|v| tc.contains(v)),
        SoarCondition::ConjunctiveNegation(sub) => {
            let mut tc = tc.clone();
            let mut left: Vec<&SoarCondition> = sub.iter().collect();
            loop {
                let before = left.len();
                left.retain(|c| {
                    let connected = in_tc(c, &tc);
                    if connected {
                        add_to_tc(c, &mut tc);
                    }
                    !connected
                });
                if left.is_empty() {
                    return true;
                }
                if left.len() == before {
                    return false;
                }
            }
        }
    }
}

impl Firing {
    /// The value a variable was bound to.
    fn binding(&self, variable: &str) -> Option<SymbolId> {
        self.source.conditions.iter().zip(&self.levels).find_map(|(c, triple)| {
            let (SoarCondition::Positive(tests), Some(t)) = (c, *triple) else { return None };
            let [id, attr, value] = fields(tests);
            [(id, t.0), (attr, t.1), (value, t.2)].into_iter().find_map(|(test, symbol)| match test.eq_test() {
                Some(SoarSymbol::Variable(v)) if v == variable => Some(symbol),
                _ => None,
            })
        })
    }
}

impl<S: Layout> SoarAgent<S> {
    /// Keeps what chunking needs of a firing (upstream `create_instantiation`'s conditions). A
    /// firing in the top state has nothing above it to learn over.
    pub(super) fn chunk_record_firing(&mut self, inst: &Instantiation) -> Option<u64> {
        if self.chunker.settings.learn == LearnIn::Never {
            return None;
        }
        let goal = self.match_goal(&inst.matched_wmes).filter(|g| self.goal_level(*g).is_some_and(|l| l > 0))?;
        let source = self.chunker.sources.get(inst.rule_name)?.clone();
        let mut matched = inst.matched_wmes.iter();
        let keys = inst.levels.iter().map(|level| level.and_then(|_| matched.next().map(|w| w.key))).collect();
        Some(self.chunk_add_firing(source, inst.rule_name, goal, inst.levels.clone(), keys))
    }

    /// Records a firing, with what put each WME it matched in working memory.
    fn chunk_add_firing(&mut self, source: Arc<RuleSource>, rule: &'static str, goal: SymbolId, levels: Vec<Option<Triple>>, keys: Vec<Option<WmeKey>>) -> u64 {
        let operator = self.symbols.predefined.operator;
        let mut traces = Vec::with_capacity(levels.len());
        let mut osk = Vec::new();
        for (index, level) in levels.iter().enumerate() {
            let Some((id, attr, value)) = *level else {
                traces.push(None);
                continue;
            };
            if attr == self.symbols.predefined.item {
                if let Some(trace) = self.chunk_item_firing(id, value) {
                    traces.push(Some(trace));
                    continue;
                }
            }
            let supporting = self.chunk_supporting(id, attr, value);
            traces.push(supporting.and_then(|p| p.firing.map(|f| (f, p.action))));
            if supporting.is_some() {
                let prohibits = self.preferences.iter().filter(|p| p.id == id && p.attr == attr && p.preference_type == PreferenceType::Prohibit);
                osk.extend(prohibits.filter_map(|p| p.firing));
                let acceptable = matches!(source.conditions.get(index), Some(SoarCondition::Positive(t)) if t.acceptable);
                if id == goal && attr == operator && !acceptable {
                    osk.extend(self.chunker.slots.get(&id).iter().flat_map(|s| s.firings.iter().copied()));
                }
            }
        }
        let firing = self.chunker.next_firing;
        self.chunker.next_firing += 1;
        self.chunker.firings.insert(firing, Firing { source, rule, goal, levels, keys, traces, opaque: false, osk, tested: Tested::default(), depth: 0 });
        firing
    }

    /// The preference behind a WME (upstream `wme::preference`). A result can have a chunk's
    /// preference beside that of the firing that made it.
    fn chunk_supporting(&self, id: SymbolId, attr: SymbolId, value: SymbolId) -> Option<&PreferenceRecord> {
        let mut supporting = self
            .preferences
            .iter()
            .filter(|p| p.id == id && p.attr == attr && p.value == value && matches!(p.preference_type, PreferenceType::Acceptable | PreferenceType::Require));
        let first = supporting.clone().next();
        supporting.find(|p| p.firing.is_some_and(|f| self.chunker.firings.contains_key(&f))).or(first)
    }

    /// Upstream `make_architectural_instantiation_for_memory_system`: a retrieval's WMEs come from
    /// the WMEs of the command. Upstream's conditions on the links to the command have no trace,
    /// so they add nothing here. Constants have no identity, so what the cue names stays a
    /// constant in a rule learned from it.
    pub(super) fn chunk_record_recall(&mut self, goal: usize, cue: &[Aug], installed: &[Triple]) -> Option<u64> {
        if self.chunker.settings.learn == LearnIn::Never || goal == 0 {
            return None;
        }
        let term = |symbol: SymbolId| self.chunk_constant(symbol).unwrap_or_else(|| SoarSymbol::Variable(format!("<{:?}>", symbol)));
        let equal = |symbol: SymbolId| SoarTest::Relational(SoarRelation::Equal, term(symbol));
        let conditions: Vec<SoarCondition> = cue
            .iter()
            .map(|w| SoarCondition::Positive(SoarConditionTests { id: equal(w.id), attr: equal(w.attr), value: equal(w.value), acceptable: w.acceptable }))
            .collect();
        let actions: Vec<SoarAction> = installed
            .iter()
            .map(|(id, attr, value)| SoarAction::Make {
                id: term(*id),
                attr: SoarRhsValue::Symbol(term(*attr)),
                value: SoarRhsValue::Symbol(term(*value)),
                preference: SoarPreference::Acceptable,
                referent: None,
            })
            .collect();
        let traces = cue.iter().map(|w| self.chunk_supporting(w.id, w.attr, w.value).and_then(|p| p.firing.map(|f| (f, p.action)))).collect();
        let firing = self.chunker.next_firing;
        self.chunker.next_firing += 1;
        let source = Arc::new(RuleSource { conditions, actions });
        let levels = cue.iter().map(|w| Some((w.id, w.attr, w.value))).collect();
        let keys = cue.iter().map(|w| Some(w.key)).collect();
        let goal = self.decider.goals[goal].id;
        self.chunker.firings.insert(firing, Firing { source, rule: RECALL, goal, levels, keys, traces, opaque: true, osk: Vec::new(), tested: Tested::default(), depth: 0 });
        Some(firing)
    }

    /// Upstream `make_architectural_instantiation_for_impasse_item`: an impasse's `^item` comes
    /// from the acceptable preference WME in the superstate and the state's superstate link.
    fn chunk_item_firing(&mut self, state: SymbolId, value: SymbolId) -> Option<Trace> {
        let level = self.goal_level(state).filter(|l| *l > 0)?;
        if let Some(firing) = self.chunker.items.get(&(state, value)) {
            return Some((*firing, 0));
        }
        let p = self.symbols.predefined;
        let superstate = self.decider.goals[level - 1].id;
        let acceptable = self.wm.iter().find(|w| w.id == superstate && w.attr == p.operator && w.val == value && w.acceptable)?.key;
        let link = self.wm.iter().find(|w| w.id == state && w.attr == p.superstate && w.val == superstate).map(|w| w.key);
        let variable = |name: &str| SoarSymbol::Variable(name.into());
        let equal = |symbol: SoarSymbol| SoarTest::Relational(SoarRelation::Equal, symbol);
        let attribute = |symbol: SymbolId| self.chunk_constant(symbol).map(SoarRhsValue::Symbol);
        let (SoarRhsValue::Symbol(operator), SoarRhsValue::Symbol(link_attr)) = (attribute(p.operator)?, attribute(p.superstate)?) else { return None };
        let conditions = alloc::vec![
            SoarCondition::Positive(SoarConditionTests { id: equal(variable("<ss>")), attr: equal(operator), value: equal(variable("<o>")), acceptable: true }),
            SoarCondition::Positive(SoarConditionTests { id: equal(variable("<s>")), attr: equal(link_attr), value: equal(variable("<ss>")), acceptable: false }),
        ];
        let actions = alloc::vec![SoarAction::Make {
            id: variable("<s>"),
            attr: attribute(p.item)?,
            value: SoarRhsValue::Symbol(variable("<o>")),
            preference: SoarPreference::Acceptable,
            referent: None,
        }];
        let firing = self.chunker.next_firing;
        self.chunker.next_firing += 1;
        self.chunker.firings.insert(
            firing,
            Firing {
                source: Arc::new(RuleSource { conditions, actions }),
                rule: "fake-instantiation",
                goal: state,
                levels: alloc::vec![Some((superstate, p.operator, value)), Some((state, p.superstate, superstate))],
                keys: alloc::vec![Some(acceptable), link],
                traces: alloc::vec![None, None],
                opaque: false,
                osk: Vec::new(),
                tested: Tested::default(),
                depth: 0,
            },
        );
        self.chunker.items.insert((state, value), firing);
        Some((firing, 0))
    }

    /// Upstream `update_proposal_OSK` and the set `run_preference_semantics` left in the slot.
    pub(super) fn chunk_update_osk(&mut self, goal: usize, knowledge: Vec<u64>, proposal: Option<u64>) {
        if goal == 0 || !self.chunker.settings.add_osk {
            return;
        }
        let id = self.decider.goals[goal].id;
        let slot = self.chunker.slots.entry(id).or_default();
        if let Some(old) = slot.proposal.take() {
            self.chunker.proposal_osk.remove(&old);
        }
        if let Some(proposal) = proposal {
            self.chunker.proposal_osk.insert(proposal, knowledge.clone());
        }
        slot.proposal = proposal;
        slot.firings = knowledge;
    }

    /// Forgets what's only reachable from a state that's gone. Its results stay, supported by
    /// the chunk; a result with no chunk can't be traced any further.
    pub(super) fn chunk_goal_removed(&mut self, id: SymbolId) {
        let chunker = &mut self.chunker;
        chunker.slots.remove(&id);
        chunker.not_bottom.retain(|g| *g != id);
        chunker.forced.retain(|g| *g != id);
        chunker.free.retain(|g| *g != id);
        chunker.items.retain(|(state, _), _| *state != id);
        chunker.firings.retain(|_, f| f.goal != id);
        let firings = &chunker.firings;
        chunker.proposal_osk.retain(|firing, _| firings.contains_key(firing));
    }

    /// Whether an identifier is at or above the superstate of goal index `goal`.
    fn chunk_is_ground(&self, id: SymbolId, goal: usize) -> bool {
        self.decider.links.level(id).is_some_and(|level| level <= goal)
    }

    /// Upstream `add_pref_to_results`.
    fn chunk_add_result(&self, a: &mut Analysis, made: &[PreferenceRecord], goal: usize, p: &PreferenceRecord, link: Option<(u64, usize)>) {
        let same = |r: &Result_| {
            r.preference.id == p.id && r.preference.attr == p.attr && r.preference.value == p.value && r.preference.preference_type == p.preference_type
        };
        if a.results.iter().any(same) {
            return;
        }
        a.results.push(Result_ { preference: p.clone() });
        // A result reached through another result's value has that value's identity.
        if let (Some(id_identity), Some(link)) = (self.chunk_pref_identity(a, p, 0), link) {
            a.identities.join(id_identity, link.1);
        }
        let value_link = self.chunk_pref_identity(a, p, 2).map(|i| (0, i));
        self.chunk_results_of(a, made, goal, p.value, value_link);
        if let PreferenceType::BinaryIndifferent(referent) | PreferenceType::Better(referent) | PreferenceType::Worse(referent) = p.preference_type {
            self.chunk_results_of(a, made, goal, referent, None);
        }
    }

    /// The identity of a field of the action that made a preference.
    fn chunk_pref_identity(&self, a: &mut Analysis, p: &PreferenceRecord, field: usize) -> Option<usize> {
        let firing = p.firing?;
        let f = self.chunker.firings.get(&firing)?;
        let action = f.source.actions.get(p.action as usize)?;
        action_variables(action)[field].map(|v| a.identities.of(firing, v))
    }

    /// Upstream `add_results_if_needed`.
    fn chunk_results_of(&self, a: &mut Analysis, made: &[PreferenceRecord], goal: usize, symbol: SymbolId, link: Option<(u64, usize)>) {
        if !self.symbols.is_identifier(symbol) || self.chunk_is_ground(symbol, goal) || a.visited_ids.contains(&symbol) {
            return;
        }
        a.visited_ids.push(symbol);
        for p in self.preferences.iter().filter(|p| p.id == symbol) {
            self.chunk_add_result(a, made, goal, p, link);
        }
        for w in self.wm.iter().filter(|w| w.id == symbol) {
            self.chunk_results_of(a, made, goal, w.val, None);
        }
        for p in made.iter().filter(|p| p.id == symbol) {
            self.chunk_add_result(a, made, goal, p, link);
        }
    }

    /// Upstream `unify_lhs_rhs_connection`: a local condition matched what an action made, so
    /// their fields are the same value.
    fn chunk_unify(&self, a: &mut Analysis, (firing, level): (u64, usize), (made_by, action): Trace) {
        let Some(f) = self.chunker.firings.get(&firing) else { return };
        let Some(m) = self.chunker.firings.get(&made_by) else { return };
        let SoarCondition::Positive(tests) = &f.source.conditions[level] else { return };
        let Some(action) = m.source.actions.get(action as usize) else { return };
        let rhs = action_variables(action);
        let written = action_values(action);
        for (field, test) in fields(tests).into_iter().enumerate() {
            if let Some(call @ SoarRhsValue::FunctionCall { .. }) = written[field] {
                self.chunk_literalize_args(a, made_by, call);
            }
            let lhs = field_variable(test, level, field).map(|v| a.identities.of(firing, &v));
            let rhs = rhs[field].map(|v| a.identities.of(made_by, v));
            match (rhs, lhs) {
                (Some(r), Some(l)) => a.identities.join(r, l),
                (Some(r), None) => a.identities.literalize(r),
                (None, Some(l)) => a.identities.literalize(l),
                (None, None) => {}
            }
        }
    }

    /// Upstream `literalize_RHS_function_args`.
    fn chunk_literalize_args(&self, a: &mut Analysis, firing: u64, call: &SoarRhsValue) {
        let SoarRhsValue::FunctionCall { name, args } = call else { return };
        if !literalizing(name) {
            return;
        }
        for arg in args {
            match arg {
                SoarRhsValue::FunctionCall { .. } => self.chunk_literalize_args(a, firing, arg),
                SoarRhsValue::Symbol(SoarSymbol::Variable(v)) => {
                    let identifier = self.chunker.firings[&firing].binding(v).is_some_and(|b| self.symbols.is_identifier(b));
                    if !identifier {
                        let identity = a.identities.of(firing, v);
                        a.identities.literalize(identity);
                    }
                }
                SoarRhsValue::Symbol(_) => {}
            }
        }
    }

    /// Upstream `literalize_constant_match_tests`: `$<x>` keeps the matched value in the rule.
    fn chunk_literalize_constant_match(a: &mut Analysis, firing: u64, test: &SoarTest) {
        let tests = flatten(test);
        for t in &tests {
            let SoarTest::Relational(SoarRelation::ConstantMatch, SoarSymbol::Variable(v)) = t else { continue };
            if tests.iter().any(|e| matches!(e, SoarTest::Relational(SoarRelation::Equal, SoarSymbol::Variable(x)) if x == v)) {
                let identity = a.identities.of(firing, v);
                a.identities.literalize(identity);
            }
        }
    }

    /// Upstream `backtrace_through_instantiation`.
    fn chunk_backtrace(&self, a: &mut Analysis, goal: usize, firing: u64) {
        if !a.visited_firings.insert(firing) {
            return;
        }
        let Some(f) = self.chunker.firings.get(&firing) else {
            a.unsupported = true;
            return;
        };
        a.tested_recall |= f.opaque;
        a.tested_quiescence |= f.tested.quiescence;
        a.tested_local_negation |= f.tested.local_negation;
        // Prohibits use this as well, so it doesn't depend on `add-osk`.
        for knowledge in &f.osk {
            self.chunk_backtrace(a, goal, *knowledge);
        }
        for knowledge in self.chunker.proposal_osk.get(&firing).into_iter().flatten() {
            self.chunk_backtrace(a, goal, *knowledge);
        }
        for (level, condition) in f.source.conditions.iter().enumerate() {
            match condition {
                SoarCondition::Positive(tests) => {
                    let Some(triple) = f.levels[level] else { continue };
                    for test in fields(tests) {
                        Self::chunk_literalize_constant_match(a, firing, test);
                    }
                    if self.chunk_is_ground(triple.0, goal) {
                        a.grounds.push((firing, level));
                    } else {
                        for (field, test) in fields(tests).into_iter().enumerate() {
                            for t in flatten(test).into_iter().filter(|t| matches!(t, SoarTest::Relational(..)) && !is_equal(t)) {
                                a.constraints.push((firing, level, field, t.clone()));
                            }
                        }
                        a.locals.push_back((firing, level));
                    }
                }
                SoarCondition::Negative(_) | SoarCondition::ConjunctiveNegation(_) => a.negateds.push((firing, level)),
            }
        }
    }

    /// Upstream `trace_locals`.
    fn chunk_trace_locals(&self, a: &mut Analysis, goal: usize) {
        let p = self.symbols.predefined;
        while let Some((firing, level)) = a.locals.pop_front() {
            let f = &self.chunker.firings[&firing];
            if let Some(trace) = f.traces[level] {
                self.chunk_unify(a, (firing, level), trace);
                self.chunk_backtrace(a, goal, trace.0);
                continue;
            }
            let Some(triple) = f.levels[level] else { continue };
            if self.symbols.is_goal(triple.0) && triple.1 == p.quiescence && triple.2 == p.t {
                a.tested_quiescence = true;
            }
        }
    }

    fn chunk_is_operator(&self, symbol: SymbolId) -> bool {
        self.decider.goals.iter().any(|g| g.operator.is_some_and(|(o, _)| o == symbol))
    }

    /// Upstream `wme_is_a_singleton`, with the architecture's own: `add_new_singleton` keeps
    /// these attributes on states to itself.
    fn chunk_is_singleton(&self, (id, attr, value): Triple) -> bool {
        let p = self.symbols.predefined;
        if self.symbols.is_goal(id) && [p.superstate, p.smem, p.epmem, p.type_, p.impasse].contains(&attr) {
            return true;
        }
        let Some(SymbolData::String(name)) = self.symbols.resolve(attr) else { return false };
        let automatic = self.chunker.settings.automatic_singletons.then_some((SingletonElement::Any, SingletonElement::Any));
        let Some((id_type, value_type)) = self.chunker.singletons.get(name).copied().or(automatic) else { return false };
        let of = |kind: SingletonElement, symbol: SymbolId| match kind {
            SingletonElement::Any => true,
            SingletonElement::Constant => !self.symbols.is_identifier(symbol),
            SingletonElement::State => self.symbols.is_goal(symbol),
            SingletonElement::Operator => self.chunk_is_operator(symbol),
            SingletonElement::Identifier => self.symbols.is_identifier(symbol) && !self.symbols.is_goal(symbol) && !self.chunk_is_operator(symbol),
        };
        of(id_type, id) && of(value_type, value)
    }

    /// Upstream `check_for_singleton_unification`: conditions that matched one singleton WME, or
    /// a state's selected operator, have the same value.
    fn chunk_unify_singletons(&self, a: &mut Analysis) {
        let mut first: BTreeMap<WmeKey, (u64, usize)> = BTreeMap::new();
        for (firing, level) in a.grounds.clone() {
            let f = &self.chunker.firings[&firing];
            let (Some(key), Some(triple)) = (f.keys[level], f.levels[level]) else { continue };
            let Some(&(other, other_level)) = first.get(&key) else {
                first.insert(key, (firing, level));
                continue;
            };
            let SoarCondition::Positive(tests) = &f.source.conditions[level] else { continue };
            let SoarCondition::Positive(other_tests) = &self.chunker.firings[&other].source.conditions[other_level] else { continue };
            let mine = field_variable(&tests.value, level, 2).map(|v| a.identities.of(firing, &v));
            let theirs = field_variable(&other_tests.value, other_level, 2).map(|v| a.identities.of(other, &v));
            if self.chunk_is_singleton(triple) {
                match (mine, theirs) {
                    (Some(m), Some(t)) => a.identities.join(m, t),
                    (Some(i), None) | (None, Some(i)) => a.identities.literalize(i),
                    (None, None) => {}
                }
            } else if triple.1 == self.symbols.predefined.operator && self.chunk_is_operator(triple.2) && !tests.acceptable {
                if let (Some(m), Some(t)) = (mine, theirs) {
                    a.identities.join(m, t);
                }
            }
        }
    }

    /// The symbol as it would be written in a rule.
    fn chunk_constant(&self, symbol: SymbolId) -> Option<SoarSymbol> {
        match self.symbols.resolve(symbol)? {
            SymbolData::String(s) => Some(SoarSymbol::Str(s.clone())),
            SymbolData::Integer(i) => Some(SoarSymbol::Int(*i)),
            SymbolData::Float(x) => Some(SoarSymbol::Float(*x)),
            SymbolData::Identifier { .. } => None,
        }
    }

    /// Upstream's choice of variable prefix: an identifier's letter, except an `s` or `o` that
    /// isn't a state or operator, and `c` for constants.
    fn chunk_prefix(&self, symbol: SymbolId) -> char {
        match self.symbols.resolve(symbol) {
            Some(SymbolData::Identifier { prefix, .. }) => {
                let letter = prefix.to_ascii_lowercase();
                if (letter == 's' && !self.symbols.is_goal(symbol)) || (letter == 'o' && !self.chunk_is_operator(symbol)) {
                    'c'
                } else {
                    letter
                }
            }
            _ => 'c',
        }
    }

    /// Upstream `variablize_equality_tests`: the term for a ground condition's field.
    fn chunk_lhs_term(&self, a: &Analysis, names: &mut Names, identity: Option<usize>, symbol: SymbolId) -> Option<SoarSymbol> {
        let identifier = self.symbols.is_identifier(symbol);
        let variable = match identity {
            Some(i) if identifier || !a.identities.literalized(i) => {
                let root = a.identities.root(i);
                if !names.of_set.contains_key(&root) {
                    // Two sets can hold one identifier if no firing linked them.
                    let name = names.of_symbol.get(&symbol).cloned().unwrap_or_else(|| names.fresh(self.chunk_prefix(symbol)));
                    names.of_set.insert(root, name);
                }
                names.of_set.get(&root).cloned()
            }
            _ => None,
        };
        if identifier {
            let name = variable.or_else(|| names.of_symbol.get(&symbol).cloned()).unwrap_or_else(|| names.fresh(self.chunk_prefix(symbol)));
            names.of_symbol.insert(symbol, name.clone());
            names.bound.insert(name.clone(), symbol);
            return Some(SoarSymbol::Variable(name));
        }
        if let Some(name) = &variable {
            names.bound.insert(name.clone(), symbol);
        }
        variable.map(SoarSymbol::Variable).or_else(|| self.chunk_constant(symbol))
    }

    /// A relational test's referent in the learned rule; `None` if the test has to go.
    fn chunk_referent(&self, a: &mut Analysis, names: &Names, firing: u64, referent: &SoarSymbol) -> Option<SoarSymbol> {
        let SoarSymbol::Variable(v) = referent else { return Some(referent.clone()) };
        let identity = a.identities.of(firing, v);
        if let Some(name) = names.of_set.get(&a.identities.root(identity)) {
            return Some(SoarSymbol::Variable(name.clone()));
        }
        let bound = self.chunker.firings[&firing].binding(v)?;
        match names.of_symbol.get(&bound) {
            Some(name) => Some(SoarSymbol::Variable(name.clone())),
            None => self.chunk_constant(bound),
        }
    }

    /// The tests besides equality on a field, for the learned rule.
    fn chunk_other_tests(&self, a: &mut Analysis, names: &Names, firing: u64, test: &SoarTest) -> Vec<SoarTest> {
        let mut out = Vec::new();
        for t in flatten(test) {
            match t {
                SoarTest::Relational(SoarRelation::Equal, _) | SoarTest::Goal | SoarTest::Impasse => {}
                SoarTest::Relational(relation, referent) => {
                    if let Some(referent) = self.chunk_referent(a, names, firing, referent) {
                        out.push(SoarTest::Relational(*relation, referent));
                    }
                }
                other => out.push(other.clone()),
            }
        }
        out
    }

    /// Upstream `create_initial_chunk_condition_lists`, `variablize_condition_list`,
    /// `merge_conditions`, `add_additional_constraints` and `add_goal_or_impasse_tests`.
    fn chunk_conditions(&self, a: &mut Analysis, names: &mut Names) -> Option<Vec<SoarCondition>> {
        // Name every ground field first: a test can refer to a variable of a later condition.
        let grounds = core::mem::take(&mut a.grounds);
        let mut terms: Vec<[SoarSymbol; 3]> = Vec::with_capacity(grounds.len());
        for &(firing, level) in &grounds {
            let f = &self.chunker.firings[&firing];
            let SoarCondition::Positive(tests) = &f.source.conditions[level] else { continue };
            let t = f.levels[level]?;
            let mut term = |field: usize, test: &SoarTest, symbol: SymbolId, a: &mut Analysis| {
                let identity = field_variable(test, level, field).map(|v| a.identities.of(firing, &v));
                self.chunk_lhs_term(a, names, identity, symbol)
            };
            terms.push([term(0, &tests.id, t.0, a)?, term(1, &tests.attr, t.1, a)?, term(2, &tests.value, t.2, a)?]);
        }

        // Conditions by their three terms: conditions that came to be the same are merged.
        let mut conditions: Vec<(usize, [Vec<SoarTest>; 3], bool)> = Vec::new();
        let mut operational: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        let mut keys: Vec<([SoarSymbol; 3], bool)> = Vec::new();
        for (&(firing, level), term) in grounds.iter().zip(&terms) {
            let f = &self.chunker.firings[&firing];
            let SoarCondition::Positive(tests) = &f.source.conditions[level] else { continue };
            let key = (term.clone(), tests.acceptable);
            let index = match keys.iter().position(|k| *k == key) {
                Some(i) => i,
                None => {
                    keys.push(key);
                    conditions.push((level, [Vec::new(), Vec::new(), Vec::new()], tests.acceptable));
                    keys.len() - 1
                }
            };
            for (field, test) in fields(tests).into_iter().enumerate() {
                if let Some(v) = field_variable(test, level, field) {
                    let identity = a.identities.of(firing, &v);
                    operational.entry(a.identities.root(identity)).or_insert((index, field));
                }
                for t in self.chunk_other_tests(a, names, firing, test) {
                    if !conditions[index].1[field].contains(&t) {
                        conditions[index].1[field].push(t);
                    }
                }
            }
        }

        // Tests on local conditions constrain the ground conditions the values came from.
        for (firing, level, field, test) in core::mem::take(&mut a.constraints) {
            let SoarTest::Relational(relation, referent) = &test else { continue };
            let f = &self.chunker.firings[&firing];
            let SoarCondition::Positive(tests) = &f.source.conditions[level] else { continue };
            let Some(v) = field_variable(fields(tests)[field], level, field) else { continue };
            let eq = a.identities.of(firing, &v);
            if let (Some(&(index, target)), false) = (operational.get(&a.identities.root(eq)), a.identities.literalized(eq)) {
                if let Some(r) = self.chunk_referent(a, names, firing, referent) {
                    let t = SoarTest::Relational(*relation, r);
                    if !conditions[index].1[target].contains(&t) {
                        conditions[index].1[target].push(t);
                    }
                }
            } else if let SoarSymbol::Variable(r) = referent {
                let other = a.identities.of(firing, r);
                let Some(&(index, target)) = operational.get(&a.identities.root(other)) else { continue };
                let Some(own) = names.of_set.get(&a.identities.root(eq)).cloned().map(SoarSymbol::Variable).or_else(|| {
                    let bound = self.chunker.firings[&firing].binding(&v)?;
                    self.chunk_constant(bound)
                }) else {
                    continue;
                };
                let t = SoarTest::Relational(reversed(*relation), own);
                if !conditions[index].1[target].contains(&t) {
                    conditions[index].1[target].push(t);
                }
            }
        }

        let goals: Vec<SymbolId> = self.decider.goals.iter().map(|g| g.id).collect();
        let mut tested_goal: Vec<String> = Vec::new();
        let mut out = Vec::with_capacity(keys.len());
        for ((term, acceptable), (_, others, _)) in keys.into_iter().zip(conditions) {
            let [mut id_tests, attr_tests, value_tests] = others;
            if let SoarSymbol::Variable(name) = &term[0] {
                let is_goal = names.of_symbol.iter().any(|(s, n)| n == name && goals.contains(s));
                if is_goal && !tested_goal.contains(name) {
                    tested_goal.push(name.clone());
                    id_tests.push(SoarTest::Goal);
                }
            }
            let [id, attr, value] = term;
            let full = |eq: SoarSymbol, rest: Vec<SoarTest>| {
                let eq = SoarTest::Relational(SoarRelation::Equal, eq);
                if rest.is_empty() {
                    eq
                } else {
                    let mut all = alloc::vec![eq];
                    all.extend(rest);
                    SoarTest::Conjunctive(all)
                }
            };
            out.push(SoarCondition::Positive(SoarConditionTests {
                id: full(id, id_tests),
                attr: full(attr, attr_tests),
                value: full(value, value_tests),
                acceptable,
            }));
        }
        Some(out)
    }

    /// A variable of a negated condition, in the learned rule's terms. One the positive conditions
    /// don't bind stays a variable of its own.
    fn chunk_negated_term(&self, a: &mut Analysis, names: &mut Names, firing: u64, variable: &str) -> SoarSymbol {
        let identity = a.identities.of(firing, variable);
        let root = a.identities.root(identity);
        if let Some(name) = names.of_set.get(&root) {
            return SoarSymbol::Variable(name.clone());
        }
        if let Some(bound) = self.chunker.firings[&firing].binding(variable) {
            if let Some(symbol) = names.of_symbol.get(&bound).cloned().map(SoarSymbol::Variable).or_else(|| self.chunk_constant(bound)) {
                return symbol;
            }
        }
        let prefix = variable.chars().find(char::is_ascii_alphabetic).unwrap_or('v').to_ascii_lowercase();
        let name = names.fresh(prefix);
        names.of_set.insert(root, name.clone());
        SoarSymbol::Variable(name)
    }

    fn chunk_negated_test(&self, a: &mut Analysis, names: &mut Names, firing: u64, test: &SoarTest) -> SoarTest {
        match test {
            SoarTest::Relational(relation, SoarSymbol::Variable(v)) => SoarTest::Relational(*relation, self.chunk_negated_term(a, names, firing, v)),
            SoarTest::Conjunctive(tests) => SoarTest::Conjunctive(tests.iter().map(|t| self.chunk_negated_test(a, names, firing, t)).collect()),
            other => other.clone(),
        }
    }

    fn chunk_negated_condition(&self, a: &mut Analysis, names: &mut Names, firing: u64, condition: &SoarCondition) -> SoarCondition {
        let mut tests = |t: &SoarConditionTests| SoarConditionTests {
            id: self.chunk_negated_test(a, names, firing, &t.id),
            attr: self.chunk_negated_test(a, names, firing, &t.attr),
            value: self.chunk_negated_test(a, names, firing, &t.value),
            acceptable: t.acceptable,
        };
        match condition {
            SoarCondition::Positive(t) => SoarCondition::Positive(tests(t)),
            SoarCondition::Negative(t) => SoarCondition::Negative(tests(t)),
            SoarCondition::ConjunctiveNegation(sub) => {
                SoarCondition::ConjunctiveNegation(sub.iter().map(|c| self.chunk_negated_condition(a, names, firing, c)).collect())
            }
        }
    }

    /// Upstream's end of `create_initial_chunk_condition_lists`: negated conditions connected to
    /// the positive ones join them; the rest test local structure.
    fn chunk_negated(&self, a: &mut Analysis, names: &mut Names, conditions: &mut Vec<SoarCondition>) -> bool {
        let mut tc = BTreeSet::new();
        conditions.iter().for_each(|c| add_to_tc(c, &mut tc));
        let mut local = false;
        for (firing, level) in core::mem::take(&mut a.negateds) {
            let f = &self.chunker.firings[&firing];
            let condition = self.chunk_negated_condition(a, names, firing, &f.source.conditions[level]);
            if !in_tc(&condition, &tc) {
                local = true;
            } else if !conditions.contains(&condition) {
                conditions.push(condition);
            }
        }
        local
    }

    /// The right-side term for a field of a result: `variable` is its variable in the firing
    /// that made it.
    fn chunk_rhs_term(&self, a: &mut Analysis, names: &mut Names, firing: Option<u64>, variable: Option<&str>, symbol: SymbolId) -> Option<SoarSymbol> {
        let identifier = self.symbols.is_identifier(symbol);
        let root = match (firing, variable) {
            (Some(firing), Some(v)) => Some(a.identities.of(firing, v)).map(|i| a.identities.root(i)),
            _ => None,
        };
        if let Some(name) = root.and_then(|r| names.of_set.get(&r)).filter(|_| identifier || !root.is_some_and(|r| a.identities.literalized(r))) {
            return Some(SoarSymbol::Variable(name.clone()));
        }
        if !identifier {
            return self.chunk_constant(symbol);
        }
        if let Some(name) = names.of_symbol.get(&symbol) {
            return Some(SoarSymbol::Variable(name.clone()));
        }
        // First sight of an identifier the rule creates.
        let letter = self.chunk_prefix(symbol);
        let name = names.fresh(letter);
        names.of_symbol.insert(symbol, name.clone());
        if let Some(r) = root {
            names.of_set.insert(r, name.clone());
        }
        Some(SoarSymbol::Variable(name))
    }

    /// A function call's arguments, in the learned rule's variables.
    fn chunk_rhs_call(&self, a: &mut Analysis, names: &Names, firing: u64, value: &SoarRhsValue) -> Option<SoarRhsValue> {
        match value {
            SoarRhsValue::Symbol(symbol) => self.chunk_referent(a, names, firing, symbol).map(SoarRhsValue::Symbol),
            SoarRhsValue::FunctionCall { name, args } => {
                let args = args.iter().map(|arg| self.chunk_rhs_call(a, names, firing, arg)).collect::<Option<Vec<_>>>()?;
                Some(SoarRhsValue::FunctionCall { name: name.clone(), args })
            }
        }
    }

    /// Upstream `variablize_results_into_actions`.
    fn chunk_actions(&self, a: &mut Analysis, names: &mut Names) -> Option<Vec<SoarAction>> {
        let results = core::mem::take(&mut a.results);
        let mut actions = Vec::with_capacity(results.len());
        for Result_ { preference: p } in &results {
            let firing = p.firing?;
            let f = self.chunker.firings.get(&firing)?;
            let action = f.source.actions.get(p.action as usize)?;
            let SoarAction::Make { attr, value, preference, referent, .. } = action else { return None };
            let variables = action_variables(action);
            let SoarSymbol::Variable(id) = self.chunk_rhs_term(a, names, Some(firing), variables[0], p.id)? else { return None };
            let term = |variable: Option<&str>, written: &SoarRhsValue, symbol: SymbolId, a: &mut Analysis, names: &mut Names| match written {
                SoarRhsValue::FunctionCall { .. } => self.chunk_rhs_call(a, names, firing, written),
                SoarRhsValue::Symbol(_) => self.chunk_rhs_term(a, names, Some(firing), variable, symbol).map(SoarRhsValue::Symbol),
            };
            let attr = term(variables[1], attr, p.attr, a, names)?;
            let value = term(variables[2], value, p.value, a, names)?;
            let referent = match (referent, p.preference_type) {
                (Some(r), PreferenceType::BinaryIndifferent(symbol) | PreferenceType::Better(symbol) | PreferenceType::Worse(symbol)) => {
                    Some(term(value_variable(r), r, symbol, a, names)?)
                }
                (Some(r), _) => Some(r.clone()),
                (None, _) => None,
            };
            actions.push(SoarAction::Make { id: SoarSymbol::Variable(id), attr, value, preference: *preference, referent });
        }
        Some(actions)
    }

    /// The variables a rule's conditions reach from its states, following values.
    fn chunk_connected(conditions: &[SoarCondition]) -> BTreeSet<String> {
        let variable = |test: &SoarTest| match test.eq_test() {
            Some(SoarSymbol::Variable(v)) => Some(v.clone()),
            _ => None,
        };
        let mut connected = BTreeSet::new();
        for c in conditions {
            if let SoarCondition::Positive(tests) = c {
                if flatten(&tests.id).iter().any(|t| matches!(t, SoarTest::Goal)) {
                    connected.extend(variable(&tests.id));
                }
            }
        }
        loop {
            let before = connected.len();
            for c in conditions {
                let SoarCondition::Positive(tests) = c else { continue };
                if variable(&tests.id).is_some_and(|id| connected.contains(&id)) {
                    connected.extend(variable(&tests.attr));
                    connected.extend(variable(&tests.value));
                }
            }
            if connected.len() == before {
                return connected;
            }
        }
    }

    /// Upstream `find_path_to_goal_for_symbol`: the WMEs from a state to `target`, depth first
    /// over identifiers.
    fn chunk_path_to(&self, target: SymbolId) -> Option<Vec<PathWme>> {
        let level = self.decider.links.level(target)?;
        let goal = self.decider.goals.iter().rev().find(|g| self.decider.links.level(g.id).is_some_and(|l| l <= level))?.id;
        let top = self.decider.goals.iter().map(|g| g.id).find(|g| *g == goal)?;
        let mut seen = alloc::vec![top];
        let mut walk: Vec<(SymbolId, Vec<PathWme>)> = alloc::vec![(top, Vec::new())];
        while let Some((id, path)) = walk.pop() {
            for w in self.wm.iter().filter(|w| w.id == id && !w.acceptable && self.symbols.is_identifier(w.val)) {
                if seen.contains(&w.val) {
                    continue;
                }
                let mut next = path.clone();
                next.push((w.key, w.id, w.attr, w.val, w.acceptable));
                if w.val == target {
                    return Some(next);
                }
                seen.push(w.val);
                walk.push((w.val, next));
            }
        }
        None
    }

    /// Upstream `repair_rule`: adds conditions connecting identifiers no state reaches, from the
    /// WMEs on the path to them. `symbols` are the identifiers bound by the rule.
    fn chunk_repair(&self, names: &mut Names, conditions: &mut Vec<SoarCondition>, ids_in_use: &[String], keys: &mut Vec<WmeKey>) -> Option<()> {
        let connected = Self::chunk_connected(conditions);
        let dangling: Vec<SymbolId> =
            names.of_symbol.iter().filter(|(_, name)| ids_in_use.contains(name) && !connected.contains(*name)).map(|(symbol, _)| *symbol).collect();
        let goals: Vec<SymbolId> = self.decider.goals.iter().map(|g| g.id).collect();
        for symbol in dangling {
            if goals.contains(&symbol) {
                // A state with no link to the rule: test it as a state.
                let name = names.of_symbol[&symbol].clone();
                for c in conditions.iter_mut() {
                    let SoarCondition::Positive(tests) = c else { continue };
                    if tests.id.eq_test() == Some(&SoarSymbol::Variable(name.clone())) {
                        let id = core::mem::replace(&mut tests.id, SoarTest::Goal);
                        tests.id = SoarTest::Conjunctive(alloc::vec![id, SoarTest::Goal]);
                        break;
                    }
                }
                continue;
            }
            for (key, id, attr, value, acceptable) in self.chunk_path_to(symbol)? {
                keys.push(key);
                let term = |names: &mut Names, s: SymbolId| {
                    if !self.symbols.is_identifier(s) {
                        return self.chunk_constant(s);
                    }
                    let name = match names.of_symbol.get(&s) {
                        Some(name) => name.clone(),
                        None => {
                            let name = names.fresh(self.chunk_prefix(s));
                            names.of_symbol.insert(s, name.clone());
                            name
                        }
                    };
                    names.bound.insert(name.clone(), s);
                    Some(SoarSymbol::Variable(name))
                };
                let id_term = term(names, id)?;
                let (attr_term, value_term) = (term(names, attr)?, term(names, value)?);
                let equal = |s: SoarSymbol| SoarTest::Relational(SoarRelation::Equal, s);
                let mut id_test = equal(id_term.clone());
                if goals.contains(&id) {
                    id_test = SoarTest::Conjunctive(alloc::vec![id_test, SoarTest::Goal]);
                }
                let tests = SoarConditionTests { id: id_test, attr: equal(attr_term), value: equal(value_term), acceptable };
                let same = |c: &SoarCondition| {
                    if let SoarCondition::Positive(t) = c {
                        t.id.eq_test() == tests.id.eq_test() && t.attr == tests.attr && t.value == tests.value && t.acceptable == acceptable
                    } else {
                        false
                    }
                };
                if !conditions.iter().any(same) {
                    conditions.push(SoarCondition::Positive(tests));
                }
            }
        }
        Some(())
    }

    /// Upstream `generate_name_for_new_rule` with the default rule naming, and the learned rule's
    /// original rule and depth.
    fn chunk_name(&self, rule: &'static str, through: u32, goal: usize) -> (String, &'static str, u32) {
        let (original, depth) = self.chunker.origins.get(rule).copied().unwrap_or((rule, 0));
        let depth = depth + through;
        let state = self.symbols.predefined.state;
        let impasse = match self.decider.goals[goal].impasse {
            Some((ImpasseType::ConstraintFailure, _)) => "*Failure",
            Some((ImpasseType::Conflict, _)) => "*Conflict",
            Some((ImpasseType::Tie, _)) => "*Tie",
            Some((ImpasseType::NoChange, attr)) if attr == state => "*StateNoChange",
            Some((ImpasseType::NoChange, _)) => "*OpNoChange",
            None => "",
        };
        let learned_from = if depth > 0 { format!("x{}", depth + 1) } else { String::new() };
        let base = format!("chunk{}*{}{}*t{}-{}", learned_from, original, impasse, self.cycle_counter, self.chunker.chunks_this_cycle);
        if !self.chunker.sources.contains_key(base.as_str()) {
            return (base, original, depth + 1);
        }
        let mut n = 2;
        loop {
            let name = format!("{}{}", base, n);
            if !self.chunker.sources.contains_key(name.as_str()) {
                return (name, original, depth + 1);
            }
            n += 1;
        }
    }

    /// Upstream `learn_rule_from_instance`: if a firing made results, learns a rule for them.
    /// Returns the preferences of the learned rule's instantiation, and of those learned from it
    /// in the states above.
    pub(super) fn chunk_learn(&mut self, firing: Option<u64>, made: &mut [PreferenceRecord], goal: Option<usize>) -> Vec<PreferenceRecord> {
        let (Some(firing), Some(goal)) = (firing, goal.filter(|g| *g > 0)) else { return Vec::new() };
        if self.cycle_counter != self.chunker.cycle {
            self.chunker.cycle = self.cycle_counter;
            self.chunker.chunks_this_cycle = 0;
            self.chunker.dupes_this_cycle.clear();
        }
        let settings = self.chunker.settings;
        if self.chunker.chunks_this_cycle >= settings.max_chunks {
            return Vec::new();
        }
        let goal_id = self.decider.goals[goal].id;
        // Upstream `set_learning_for_instantiation`: otherwise results only get a justification.
        let learning = !((settings.learn == LearnIn::Only && !self.chunker.forced.contains(&goal_id))
            || (settings.learn == LearnIn::Except && self.chunker.free.contains(&goal_id))
            || (settings.bottom_only && self.chunker.not_bottom.contains(&goal_id)));
        let rule = self.chunker.firings[&firing].rule;
        if self.chunker.dupes_this_cycle.get(rule).is_some_and(|n| *n >= settings.max_dupes) {
            return Vec::new();
        }

        let mut a = Analysis::default();
        for p in made.iter() {
            if self.chunk_is_ground(p.id, goal) && !a.visited_ids.contains(&p.id) {
                self.chunk_add_result(&mut a, made, goal, p, None);
            }
        }
        if a.results.is_empty() {
            return Vec::new();
        }
        self.chunker.not_bottom.extend(self.decider.goals[..goal].iter().map(|g| g.id));

        let sources: Vec<u64> = a.results.iter().filter_map(|r| r.preference.firing).collect();
        for source in sources {
            self.chunk_backtrace(&mut a, goal, source);
        }
        self.chunk_trace_locals(&mut a, goal);
        self.chunk_unify_singletons(&mut a);

        let mut ground_keys: Vec<WmeKey> = a.grounds.iter().filter_map(|&(f, level)| self.chunker.firings[&f].keys[level]).collect();
        let results = a.results.clone();
        let mut names = Names::default();
        let Some(mut conditions) = self.chunk_conditions(&mut a, &mut names) else {
            self.chunker.stats.justifications += 1;
            return Vec::new();
        };
        if conditions.is_empty() {
            return Vec::new();
        }
        let local_negation = self.chunk_negated(&mut a, &mut names, &mut conditions) || a.tested_local_negation;
        if a.unsupported {
            self.chunker.stats.justifications += 1;
            return Vec::new();
        }
        let tested = Tested { quiescence: a.tested_quiescence, local_negation };
        let opaque = a.tested_recall;
        let filtered = tested.quiescence || (opaque && !settings.allow_opaque) || (local_negation && !settings.allow_local_negations);
        let Some(actions) = self.chunk_actions(&mut a, &mut names) else {
            self.chunker.stats.justifications += 1;
            return Vec::new();
        };
        let justification = Justification { firing, goal, tested, opaque };
        if filtered || !learning {
            return self.chunk_justify(justification, (conditions, actions), &names, &ground_keys, &results, made);
        }

        let mut ids_in_use: Vec<String> = Vec::new();
        for c in &conditions {
            if let SoarCondition::Positive(tests) = c {
                if let Some(SoarSymbol::Variable(v)) = tests.id.eq_test() {
                    ids_in_use.push(v.clone());
                }
            }
        }
        for action in &actions {
            if let SoarAction::Make { id: SoarSymbol::Variable(v), .. } = action {
                ids_in_use.push(v.clone());
            }
        }
        // Identifiers only the right side makes need no connection.
        let created: Vec<String> = actions
            .iter()
            .filter_map(|action| match action {
                SoarAction::Make { value, .. } => value_variable(value).map(String::from),
                SoarAction::FunctionCall(_) => None,
            })
            .collect();
        ids_in_use.retain(|v| !created.contains(v) || conditions.iter().any(|c| matches!(c, SoarCondition::Positive(t) if t.value.eq_test() == Some(&SoarSymbol::Variable(v.clone())) || t.id.eq_test() == Some(&SoarSymbol::Variable(v.clone())))));
        if self.chunk_repair(&mut names, &mut conditions, &ids_in_use, &mut ground_keys).is_none() {
            return self.chunk_justify(justification, (conditions, actions), &names, &ground_keys, &results, made);
        }

        let key = format!("{:?}{:?}", conditions, actions);
        if !self.chunker.learned.insert(key.clone()) {
            self.chunker.stats.duplicates += 1;
            *self.chunker.dupes_this_cycle.entry(rule).or_insert(0) += 1;
            return Vec::new();
        }
        self.chunker.chunks_this_cycle += 1;
        let through = self.chunker.firings[&firing].depth;
        let (name, original, depth) = self.chunk_name(rule, through, goal);
        let production = SoarProduction {
            name: name.clone(),
            documentation: None,
            support: SoarSupport::Undeclared,
            production_type: SoarProductionType::Chunk,
            interrupt: false,
            explain: false,
            conditions,
            actions,
            warnings: Vec::new(),
        };
        if self.install_production(&production).is_err() {
            self.chunker.learned.remove(&key);
            self.chunker.chunks_this_cycle -= 1;
            return self.chunk_justify(justification, (production.conditions, production.actions), &names, &ground_keys, &results, made);
        }
        self.chunker.stats.chunks += 1;
        if let Some((installed, _)) = self.chunker.sources.get_key_value(name.as_str()) {
            self.chunker.origins.insert(*installed, (original, depth));
        }
        self.chunker.rules.push(production);
        self.chunk_refract(&name, &ground_keys, &results)
    }

    /// The WME each of a rule's conditions came from, among those the analysis found (upstream
    /// `reinstantiate_current_rule`).
    fn chunk_bind(&self, names: &Names, conditions: &[SoarCondition], grounds: &[WmeKey]) -> (Vec<Option<Triple>>, Vec<Option<WmeKey>>) {
        let same = |term: Option<&SoarSymbol>, symbol: SymbolId| match term {
            Some(SoarSymbol::Variable(v)) => names.bound.get(v) == Some(&symbol),
            Some(constant) => self.chunk_constant(symbol).as_ref() == Some(constant),
            None => false,
        };
        conditions
            .iter()
            .map(|c| {
                let SoarCondition::Positive(tests) = c else { return (None, None) };
                let [id, attr, value] = fields(tests).map(|t| t.eq_test());
                let wme = grounds.iter().filter_map(|k| self.wm.get(*k).map(|w| (*k, w))).find(|(_, w)| {
                    w.acceptable == tests.acceptable && same(id, w.id) && same(attr, w.attr) && same(value, w.val)
                });
                match wme {
                    Some((key, w)) => (Some((w.id, w.attr, w.val)), Some(key)),
                    None => (None, None),
                }
            })
            .unzip()
    }

    /// Upstream's `ebc_justification`: what would have been a rule stays a firing, in the deepest
    /// state it depends on, and owns the results (the preferences keep their support). A
    /// justification is variablized here, where upstream's keeps identities on its conditions.
    /// It's learned from in the states above like a chunk's instantiation.
    fn chunk_justify(
        &mut self,
        justification: Justification,
        (conditions, actions): (Vec<SoarCondition>, Vec<SoarAction>),
        names: &Names,
        grounds: &[WmeKey],
        results: &[Result_],
        made: &mut [PreferenceRecord],
    ) -> Vec<PreferenceRecord> {
        self.chunker.stats.justifications += 1;
        let (levels, keys) = self.chunk_bind(names, &conditions, grounds);
        let ids = levels.iter().flatten().map(|t| t.0).chain(results.iter().map(|r| r.preference.id));
        let level = ids.filter_map(|id| self.decider.links.level(id)).max().unwrap_or(1).clamp(1, justification.goal) - 1;
        // Everything in the top state is ground: nothing traces this far.
        if level == 0 {
            return Vec::new();
        }
        let (rule, depth) = {
            let f = &self.chunker.firings[&justification.firing];
            (f.rule, f.depth + 1)
        };
        let source = Arc::new(RuleSource { conditions, actions });
        let state = self.decider.goals[level].id;
        let firing = self.chunk_add_firing(source, rule, state, levels, keys);
        if let Some(f) = self.chunker.firings.get_mut(&firing) {
            f.opaque = justification.opaque;
            f.tested = justification.tested;
            f.depth = depth;
        }
        let mut owned = Vec::with_capacity(results.len());
        for (action, Result_ { preference: r }) in results.iter().enumerate() {
            let same = |p: &&mut PreferenceRecord| {
                p.firing == r.firing && p.id == r.id && p.attr == r.attr && p.value == r.value && p.preference_type == r.preference_type
            };
            for p in self.preferences.iter_mut().chain(made.iter_mut()).filter(same) {
                p.firing = Some(firing);
                p.action = action as u32;
            }
            owned.push(PreferenceRecord { firing: Some(firing), action: action as u32, ..r.clone() });
        }
        self.chunk_learn(Some(firing), &mut owned, Some(level))
    }

    /// Upstream adds a learned rule with a refracted instantiation: the match this firing already
    /// produced doesn't fire again, but its results are the chunk's own too (upstream
    /// `make_clones_of_results`), and it's what the state above learns from (bottom-up chunking).
    fn chunk_refract(&mut self, name: &str, grounds: &[WmeKey], results: &[Result_]) -> Vec<PreferenceRecord> {
        let mut clones = Vec::new();
        for pending in self.rete.pending_matches() {
            if pending.rule_name != name || !pending.matched_wmes.iter().all(|w| grounds.contains(&w.key)) {
                continue;
            }
            let Some(inst) = self.rete.fire_match(pending.token) else { continue };
            let goal = self.match_goal(&inst.matched_wmes).map_or(FiredGoal::Top, FiredGoal::Goal);
            let keys = inst.matched_wmes.iter().map(|w| w.key).collect();
            self.decider.fired.push((inst.id, goal, keys));
            if !clones.is_empty() {
                continue;
            }
            let firing = self.chunk_record_firing(&inst);
            let match_goal = self.match_goal(&inst.matched_wmes);
            let o_supported = self.calculate_support(inst.production, &inst.matched_wmes, &inst.levels, &[], false);
            clones.extend(results.iter().enumerate().map(|(action, r)| PreferenceRecord {
                o_supported,
                instantiation: Some(inst.id),
                production: Some(inst.rule_name),
                match_goal,
                justification: None,
                firing,
                action: action as u32,
                ..r.preference.clone()
            }));
            let level = match_goal.and_then(|g| self.goal_level(g));
            let above = self.chunk_learn(firing, &mut clones, level);
            clones.extend(above);
        }
        clones
    }
}
