//! The decision procedure, ported from upstream `decide.cpp` and `consistency.cpp`:
//! preference memory and slots, the goal stack and impasse substates, context and
//! non-context slot decisions, the consistency check, and the propose/apply "waterfall"
//! that fires rules for the highest active goal first.
//!
//! Goal dependency sets and the inner waterfall loop are ported. Identifier levels, which
//! upstream maintains incrementally through link counts, are recomputed by walking working
//! memory from the goals. Results from substates use a lightweight justification (the
//! higher-level WMEs a firing depends on) instead of upstream's EBC justifications.

use super::SoarAgent;
use crate::impasse::{ImpasseType, SubstateRecord};
use crate::preference::{run_preference_semantics, DecisionResult, Preference, PreferenceType, Semantics};
use crate::rete::{MatchId, TokenId, WmeRecord};
use crate::symbol::SymbolId;
use crate::wm::{SupportType, WmeKey};
use alloc::vec::Vec;

/// A preference in preference memory (upstream `preference`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceRecord {
    /// Slot identifier.
    pub id: SymbolId,
    /// Slot attribute.
    pub attr: SymbolId,
    /// Value.
    pub value: SymbolId,
    /// Type, with the referent for binary preferences.
    pub preference_type: PreferenceType,
    /// O-supported preferences outlive the match that made them.
    pub o_supported: bool,
    /// The rule firing that made it; `None` for preferences asserted directly.
    pub instantiation: Option<MatchId>,
    /// The production whose firing made it; `None` for preferences asserted directly.
    pub production: Option<&'static str>,
    /// Goal the making rule matched in (upstream `match_goal`).
    pub match_goal: Option<SymbolId>,
    /// For a result returned from a substate: the higher-level WMEs it depends on. It is
    /// retracted (unless o-supported) when one of them leaves working memory.
    pub justification: Option<Vec<WmeKey>>,
    /// Assertion order, used to order slot lists as upstream does.
    pub(crate) seq: u64,
}

impl PreferenceRecord {
    pub(super) fn architectural(p: &Preference, attr: SymbolId, match_goal: Option<SymbolId>) -> Self {
        Self {
            id: p.state,
            attr,
            value: p.operator,
            preference_type: p.preference_type,
            o_supported: true,
            instantiation: None,
            production: None,
            match_goal,
            justification: None,
            seq: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum FiringType {
    /// I-supported productions and retractions (upstream `IE_PRODS`).
    #[default]
    Ie,
    /// O-supported productions (upstream `PE_PRODS`).
    Pe,
}

#[derive(Debug, Clone)]
pub(super) struct Goal {
    pub(super) id: SymbolId,
    /// Selected operator and its `(goal ^operator O)` WME.
    pub(super) operator: Option<(SymbolId, WmeKey)>,
    slot_changed: bool,
    pub(super) impasse_wmes: Vec<WmeKey>,
    /// The impasse this goal was created for; `None` for the top goal.
    impasse: Option<(ImpasseType, SymbolId)>,
    saved_firing_type: Option<FiringType>,
    /// Reinforcement-learning data (upstream `rl_info`).
    pub(super) rl: crate::rl::RlGoal,
}

impl Goal {
    fn new(id: SymbolId, impasse: Option<(ImpasseType, SymbolId)>) -> Self {
        Self { id, operator: None, slot_changed: false, impasse_wmes: Vec::new(), impasse, saved_firing_type: None, rl: Default::default() }
    }
}

/// The goal a fired instantiation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FiredGoal {
    Goal(SymbolId),
    /// Fired before any goal existed (hand-built agents): belongs to the top level.
    Top,
    /// Its goal was removed (upstream's nil-goal retractions).
    Removed,
}

#[derive(Debug, Default)]
pub(super) struct Decider {
    /// Goal stack, top goal first; a goal's level is its index plus one.
    pub(super) goals: Vec<Goal>,
    /// Each fired instantiation: its goal (for its retraction) and the WMEs it matched.
    fired: Vec<(MatchId, FiredGoal, Vec<WmeKey>)>,
    /// WMEs made from non-context slot preferences.
    slot_wmes: Vec<(SymbolId, SymbolId, SymbolId, WmeKey)>,
    /// Acceptable-preference WMEs `(goal ^operator value +)`.
    acceptable_wmes: Vec<(SymbolId, SymbolId, WmeKey)>,
    changed_slots: Vec<(SymbolId, SymbolId)>,
    /// Identifier link counts and levels.
    pub(super) links: super::links::Links,
    /// Goal dependency sets: each WME and the goal whose GDS holds it.
    gds: Vec<(WmeKey, SymbolId)>,
    /// Instantiations already traced into a GDS (upstream `GDS_evaluated_already`).
    gds_evaluated: Vec<MatchId>,
    highest_goal_whose_context_changed: Option<usize>,
    active_goal: Option<usize>,
    active_level: usize,
    firing_type: FiringType,
    pub(super) e_cycles_this_d_cycle: usize,
    next_seq: u64,
}

impl Decider {
    pub(super) fn forget_wme(&mut self, key: WmeKey) {
        self.slot_wmes.retain(|w| w.3 != key);
        self.acceptable_wmes.retain(|w| w.2 != key);
        self.gds.retain(|g| g.0 != key);
        for goal in &mut self.goals {
            goal.impasse_wmes.retain(|k| *k != key);
            if goal.operator.is_some_and(|(_, k)| k == key) {
                goal.operator = None;
            }
        }
    }
}

/// Pending rule activity, by goal (upstream's per-goal `ms_*` lists).
pub(super) struct Activity {
    /// (match, goal index, firing type), newest first.
    assertions: Vec<(TokenId, usize, FiringType)>,
    /// The pending matches themselves, parallel to `assertions`.
    pending: Vec<crate::rete::PendingMatch>,
    /// (instantiation, goal index or `None` when its goal is gone).
    retractions: Vec<(MatchId, Option<usize>)>,
}

impl Activity {
    fn i_activity(&self, goal: usize) -> bool {
        self.assertions.iter().any(|a| a.1 == goal && a.2 == FiringType::Ie)
            || self.retractions.iter().any(|r| r.1 == Some(goal))
    }

    fn any_activity(&self, goal: usize) -> bool {
        self.assertions.iter().any(|a| a.1 == goal) || self.retractions.iter().any(|r| r.1 == Some(goal))
    }

    fn nil_goal_retractions(&self) -> bool {
        self.retractions.iter().any(|r| r.1.is_none())
    }
}

impl<S: crate::pool::Layout> SoarAgent<S> {
    /// Index of a goal on the stack.
    pub(super) fn goal_level(&self, id: SymbolId) -> Option<usize> {
        self.decider.goals.iter().position(|g| g.id == id)
    }

    pub(super) fn goal_of(&self, id: SymbolId) -> Option<SymbolId> {
        self.goal_level(id).map(|_| id)
    }

    /// The deepest goal whose identifier a match tests (upstream
    /// `find_goal_for_match_set_change_assertion`).
    pub(super) fn match_goal(&self, matched: &[WmeRecord]) -> Option<SymbolId> {
        matched
            .iter()
            .filter_map(|w| self.goal_level(w.triple.0).map(|level| (level, w.triple.0)))
            .max_by_key(|(level, _)| *level)
            .map(|(_, id)| id)
    }

    /// Makes `state_id` the top goal if the stack is empty (for agents set up by hand).
    pub(super) fn ensure_top_goal(&mut self, state_id: SymbolId) {
        if self.decider.goals.is_empty() {
            self.symbols.mark_goal(state_id);
            self.decider.goals.push(Goal::new(state_id, None));
            self.link_goal(state_id, 1);
            // Operator preferences made before the state became a goal sat in an ordinary
            // slot; they now belong to its operator (context) slot.
            let operator = self.symbols.predefined.operator;
            for (_, _, _, key) in self.decider.slot_wmes.clone().into_iter().filter(|w| w.0 == state_id && w.1 == operator) {
                self.remove_wme_quietly(key);
            }
            self.decider.changed_slots.retain(|slot| *slot != (state_id, operator));
            if self.preferences.iter().any(|p| p.id == state_id && p.attr == operator) {
                self.mark_slot_changed(state_id, operator);
                self.do_working_memory_phase();
            }
            self.do_buffered_link_changes();
            self.sync_goal_records();
        }
    }

    fn sync_goal_records(&mut self) {
        let goals = &self.decider.goals;
        self.selected_operator = goals.first().and_then(|g| g.operator.map(|(op, _)| op));
        self.substates = goals
            .windows(2)
            .map(|pair| SubstateRecord {
                substate_id: pair[1].id,
                superstate_id: pair[0].id,
                impasse_type: pair[1].impasse.map_or(ImpasseType::NoChange, |(t, _)| t),
            })
            .collect();
    }

    // ------------------------------------------------------------------
    // Goal stack (upstream create_new_context, create_new_impasse,
    // update_impasse_items, remove_existing_context_and_descendents)
    // ------------------------------------------------------------------

    fn add_impasse_wme(&mut self, goal: usize, id: SymbolId, attr: SymbolId, value: SymbolId) -> WmeKey {
        let key = self.insert_wme_with_flag(id, attr, value, false, SupportType::ISupport);
        self.decider.goals[goal].impasse_wmes.push(key);
        key
    }

    /// Pushes a new goal: the top goal when the stack is empty, else a substate for an
    /// impasse of the bottom goal. Returns its identifier.
    pub(super) fn create_new_context(&mut self, attr: Option<SymbolId>, impasse: Option<ImpasseType>) -> SymbolId {
        let level = self.decider.goals.len() + 1;
        let id = self.new_identifier('S', level);
        self.symbols.mark_goal(id);
        let superstate = self.decider.goals.last().map(|g| g.id);
        let attr_symbol = attr.unwrap_or(self.symbols.predefined.state);
        let record = superstate.map(|_| (impasse.unwrap_or(ImpasseType::NoChange), attr_symbol));
        self.decider.goals.push(Goal::new(id, record));
        let g = self.decider.goals.len() - 1;
        self.link_goal(id, level);

        // Upstream create_new_impasse, in its order (which fixes identifier numbering).
        let state = self.symbols.predefined.state;
        self.add_impasse_wme(g, id, self.symbols.predefined.type_, state);
        let superstate_value = superstate.unwrap_or(self.symbols.predefined.nil);
        self.add_impasse_wme(g, id, self.symbols.predefined.superstate, superstate_value);
        let reward = self.new_identifier('R', level);
        self.add_impasse_wme(g, id, self.symbols.predefined.reward_link, reward);
        self.decider.goals[g].rl.reward_link = Some(reward);
        let epmem = self.new_identifier('E', level);
        self.add_impasse_wme(g, id, self.symbols.predefined.epmem, epmem);
        let epmem_command = self.new_identifier('C', level);
        self.add_impasse_wme(g, epmem, self.symbols.predefined.command, epmem_command);
        let epmem_result = self.new_identifier('R', level);
        self.add_impasse_wme(g, epmem, self.symbols.predefined.result, epmem_result);
        let one = self.symbols.intern_int(1);
        self.add_impasse_wme(g, epmem, self.symbols.predefined.present_id, one);
        let smem = self.new_identifier('L', level);
        let smem_command = self.new_identifier('C', level);
        let smem_result = self.new_identifier('R', level);
        self.add_impasse_wme(g, id, self.symbols.predefined.smem, smem);
        self.add_impasse_wme(g, smem, self.symbols.predefined.command, smem_command);
        self.add_impasse_wme(g, smem, self.symbols.predefined.result, smem_result);

        if let Some(attr) = attr {
            self.add_impasse_wme(g, id, self.symbols.predefined.attribute, attr);
        }
        if let Some(impasse) = impasse {
            let p = self.symbols.predefined;
            let (name, choices) = match impasse {
                ImpasseType::ConstraintFailure => (p.constraint_failure, p.none),
                ImpasseType::Conflict => (p.conflict, p.multiple),
                ImpasseType::Tie => (p.tie, p.multiple),
                ImpasseType::NoChange => (p.no_change, p.none),
            };
            self.add_impasse_wme(g, id, self.symbols.predefined.impasse, name);
            self.add_impasse_wme(g, id, self.symbols.predefined.choices, choices);
        }
        if superstate.is_some() {
            let t = self.symbols.predefined.t;
            self.add_impasse_wme(g, id, self.symbols.predefined.quiescence, t);
            self.state_counter += 1;
        }
        self.sync_goal_records();
        id
    }

    fn type_of_existing_impasse(&self, goal: usize) -> Option<(ImpasseType, SymbolId)> {
        self.decider.goals.get(goal + 1).and_then(|g| g.impasse)
    }

    /// Upstream `update_impasse_items`: `^item`/`^item-count`, and `^non-numeric` /
    /// `^non-numeric-count` for items without numeric-indifferent preferences.
    fn update_impasse_items(&mut self, goal: usize, items: &[SymbolId]) {
        let id = self.decider.goals[goal].id;
        let numeric: Vec<SymbolId> = if goal > 0 {
            let superstate = self.decider.goals[goal - 1].id;
            let operator = self.symbols.predefined.operator;
            self.preferences
                .iter()
                .filter(|p| p.id == superstate && p.attr == operator && matches!(p.preference_type, PreferenceType::NumericIndifferent(_)))
                .map(|p| p.value)
                .collect()
        } else {
            Vec::new()
        };
        let p = self.symbols.predefined;
        for (attr, count_attr, numeric_pass) in [(p.item, p.item_count, false), (p.non_numeric, p.non_numeric_count, true)] {
            let wanted: Vec<SymbolId> = items
                .iter()
                .copied()
                .filter(|item| !(numeric_pass && !items.is_empty() && numeric.contains(item)))
                .collect();
            for key in self.decider.goals[goal].impasse_wmes.clone() {
                let Some(w) = self.wm.get(key) else { continue };
                if w.id == id && ((w.attr == attr && !wanted.contains(&w.val)) || w.attr == count_attr) {
                    self.remove_wme_quietly(key);
                }
            }
            for item in &wanted {
                if self.wm.find(id, attr, *item).is_none() {
                    self.add_impasse_wme(goal, id, attr, *item);
                }
            }
            if !wanted.is_empty() {
                let count = self.symbols.intern_int(wanted.len() as i64);
                self.add_impasse_wme(goal, id, count_attr, count);
            }
        }
    }

    /// Identifiers at a level above goal index `goal` (upstream `id->level`).
    fn reachable_from_goals_above(&self, goal: usize) -> Vec<SymbolId> {
        self.decider.links.ids_at_or_above(goal)
    }

    /// Upstream `remove_existing_context_and_descendents`.
    pub(super) fn remove_existing_context_and_descendents(&mut self, goal: usize) {
        if goal + 1 < self.decider.goals.len() {
            self.remove_existing_context_and_descendents(goal + 1);
        }
        let id = self.decider.goals[goal].id;
        if goal > 0 && self.rl_learning().is_some() {
            self.rl_update_for_removed_goal(goal);
        }

        // Preferences the goal's rules made go with it. Results (preferences on structure
        // reachable from higher goals) stay, now belonging to the superstate; without
        // justifications, i-supported results retract when their match does.
        let results = if goal > 0 { self.reachable_from_goals_above(goal) } else { Vec::new() };
        let superstate = goal.checked_sub(1).map(|g| self.decider.goals[g].id);
        let mut removed = Vec::new();
        for p in &mut self.preferences {
            if p.match_goal == Some(id) {
                if results.contains(&p.id) {
                    p.match_goal = superstate;
                } else {
                    removed.push((p.id, p.attr));
                }
            }
        }
        self.remove_preferences(|p| p.match_goal == Some(id));
        for (slot_id, attr) in removed {
            self.mark_slot_changed(slot_id, attr);
        }

        self.remove_wmes_for_context_slot(goal);
        self.update_impasse_items(goal, &[]);
        for key in core::mem::take(&mut self.decider.goals[goal].impasse_wmes) {
            self.remove_wme_quietly(key);
        }
        for (_, match_goal, _) in &mut self.decider.fired {
            if *match_goal == FiredGoal::Goal(id) {
                *match_goal = FiredGoal::Removed;
            }
        }
        self.decider.gds.retain(|g| g.1 != id);
        self.symbols.unmark_goal(id);
        self.decider.goals.truncate(goal);
        self.unlink_goal(id);
        if self.decider.highest_goal_whose_context_changed.is_some_and(|g| g >= goal) {
            self.decider.highest_goal_whose_context_changed = None;
        }
        if self.decider.active_goal.is_some_and(|g| g >= goal) {
            self.decider.active_goal = None;
        }
        self.sync_goal_records();
    }

    fn remove_wmes_for_context_slot(&mut self, goal: usize) {
        if let Some((_, key)) = self.decider.goals[goal].operator.take() {
            self.remove_wme_quietly(key);
        }
        self.sync_goal_records();
    }

    // ------------------------------------------------------------------
    // Preference memory and slots
    // ------------------------------------------------------------------

    fn is_context_slot(&mut self, id: SymbolId, attr: SymbolId) -> bool {
        attr == self.symbols.predefined.operator && self.goal_level(id).is_some()
    }

    fn mark_slot_changed(&mut self, id: SymbolId, attr: SymbolId) {
        if self.is_context_slot(id, attr) {
            let g = self.goal_level(id).unwrap_or(0);
            self.decider.goals[g].slot_changed = true;
            let highest = &mut self.decider.highest_goal_whose_context_changed;
            *highest = Some(highest.map_or(g, |h| h.min(g)));
        } else if !self.decider.changed_slots.contains(&(id, attr)) {
            self.decider.changed_slots.push((id, attr));
        }
    }

    /// A slot's preferences in upstream's list order: by match-goal level, newest first.
    pub(super) fn slot_preference_records(&self, id: SymbolId, attr: SymbolId) -> Vec<&PreferenceRecord> {
        let mut prefs: Vec<&PreferenceRecord> = self.preferences.iter().filter(|p| p.id == id && p.attr == attr).collect();
        let level = |p: &PreferenceRecord| p.match_goal.and_then(|g| self.goal_level(g)).unwrap_or(0);
        prefs.sort_by(|a, b| level(a).cmp(&level(b)).then(b.seq.cmp(&a.seq)));
        prefs
    }

    /// Runs preference semantics on a goal's operator slot. Outside the consistency check, the
    /// reinforcement-learning update upstream makes during semantics is applied at once, since
    /// it can change values that later slots' decisions use.
    fn operator_slot_semantics(&mut self, goal: usize, consistency: bool) -> Semantics {
        let id = self.decider.goals[goal].id;
        let operator = self.symbols.predefined.operator;
        let records = self.slot_preference_records(id, operator);
        let prefs: Vec<Preference> =
            records.iter().map(|p| Preference { state: p.id, operator: p.value, preference_type: p.preference_type }).collect();
        let from_rl_rule: Vec<bool> = records.iter().map(|p| p.production.is_some_and(|name| self.rl_rule_index(name).is_some())).collect();
        let learning = self.rl_learning();
        let semantics = run_preference_semantics(&prefs, &from_rl_rule, consistency, learning, &mut self.exploration, &self.symbols);
        if let Some(choice) = semantics.rl {
            self.rl_apply_choice(goal, choice);
        }
        semantics
    }

    /// Upstream `assert_new_preferences`: o-supported rejects first remove every preference
    /// in their slot with the same value; the other new preferences are then added.
    pub(super) fn assert_preferences(&mut self, new: Vec<PreferenceRecord>) {
        let is_o_reject = |p: &PreferenceRecord| p.preference_type == PreferenceType::Reject && p.o_supported;
        for reject in new.iter().filter(|p| is_o_reject(p)) {
            self.remove_preferences(|p| p.id == reject.id && p.attr == reject.attr && p.value == reject.value);
        }
        let top = self.decider.goals.first().map(|g| g.id);
        let mut reach_cache: Vec<(usize, Vec<SymbolId>)> = Vec::new();
        for mut p in new.into_iter().filter(|p| !is_o_reject(p)) {
            self.make_result_if_needed(&mut p, &mut reach_cache);
            // Upstream DECIDER_KEEP_TOP_OPREFS (off): one top-state o-supported acceptable per value.
            if p.match_goal.is_some()
                && p.match_goal == top
                && p.o_supported
                && p.preference_type == PreferenceType::Acceptable
                && !self.is_context_slot(p.id, p.attr)
                && self.preferences.iter().any(|q| {
                    q.id == p.id && q.attr == p.attr && q.value == p.value && q.o_supported && q.match_goal == top
                })
            {
                continue;
            }
            p.seq = self.decider.next_seq;
            self.decider.next_seq += 1;
            let slot = (p.id, p.attr);
            self.post_preference_links(p.id, p.value, p.preference_type, true);
            self.preferences.push(p);
            self.mark_slot_changed(slot.0, slot.1);
        }
    }

    /// A preference a substate rule makes for structure reachable from a higher goal is a
    /// result. Upstream builds a justification for it; here the firing is traced back to the
    /// higher-level WMEs it depends on, and the result is supported by exactly those, at the
    /// level of the deepest goal they test. Support is recalculated as for an instantiation:
    /// o-support when they include that goal's selected operator.
    fn make_result_if_needed(&mut self, p: &mut PreferenceRecord, reach_cache: &mut Vec<(usize, Vec<SymbolId>)>) {
        let Some(inst) = p.instantiation else { return };
        let Some(goal) = p.match_goal.and_then(|g| self.goal_level(g)).filter(|g| *g > 0) else { return };
        if !reach_cache.iter().any(|(g, _)| *g == goal) {
            let reach = self.reachable_from_goals_above(goal);
            reach_cache.push((goal, reach));
        }
        let reach = &reach_cache.iter().find(|(g, _)| *g == goal).map(|(_, r)| r.clone()).unwrap_or_default();
        if !reach.contains(&p.id) {
            return;
        }
        let ground = self.backtrace(inst, reach);
        let level = ground
            .iter()
            .filter_map(|k| self.wm.get(*k))
            .filter_map(|w| self.goal_level(w.id))
            .max()
            .unwrap_or(goal - 1);
        let goal_id = self.decider.goals[level].id;
        let operator = self.symbols.predefined.operator;
        let selected = ground
            .iter()
            .filter_map(|k| self.wm.get(*k))
            .find(|w| w.id == goal_id && w.attr == operator && !w.acceptable)
            .map(|w| w.val);
        let is_proposal = p.attr == operator && p.preference_type == PreferenceType::Acceptable && self.goal_level(p.id).is_some();
        let is_operator_elaboration = selected == Some(p.id);
        p.o_supported = selected.is_some() && !is_proposal && !is_operator_elaboration;
        p.match_goal = Some(goal_id);
        p.instantiation = None;
        p.justification = Some(ground);
    }

    /// The WMEs from goals above a substate that a substate firing ultimately depends on,
    /// found by tracing local WMEs back through the preferences and firings that made them
    /// (impasse `^item`s trace to the superstate's acceptable preferences).
    fn backtrace(&mut self, inst: MatchId, reach: &[SymbolId]) -> Vec<WmeKey> {
        let item = self.symbols.predefined.item;
        let operator = self.symbols.predefined.operator;
        let mut ground = Vec::new();
        let mut stack = alloc::vec![inst];
        let mut visited = Vec::new();
        while let Some(i) = stack.pop() {
            if visited.contains(&i) {
                continue;
            }
            visited.push(i);
            let keys = self.decider.fired.iter().find(|(m, _, _)| *m == i).map(|(_, _, k)| k.clone()).unwrap_or_default();
            for key in keys {
                let Some(w) = self.wm.get(key).cloned() else { continue };
                if reach.contains(&w.id) {
                    if !ground.contains(&key) {
                        ground.push(key);
                    }
                    continue;
                }
                let (id, attr, value) = (w.id, w.attr, w.val);
                // An impasse ^item traces to the superstate's acceptable preference WME.
                if attr == item {
                    if let Some(level) = self.goal_level(id).filter(|l| *l > 0) {
                        let superstate = self.decider.goals[level - 1].id;
                        if let Some(&(_, _, k)) = self.decider.acceptable_wmes.iter().find(|a| a.0 == superstate && a.1 == value) {
                            if reach.contains(&superstate) {
                                if !ground.contains(&k) {
                                    ground.push(k);
                                }
                            } else {
                                stack.extend(self.supporting_firing(superstate, operator, value));
                            }
                        }
                        continue;
                    }
                }
                // Local WMEs made from preferences trace to the firing that made them.
                let made_by_preferences = self.decider.slot_wmes.iter().any(|s| s.3 == key)
                    || self.decider.acceptable_wmes.iter().any(|a| a.2 == key)
                    || self.decider.goals.iter().any(|g| g.operator.is_some_and(|(_, k)| k == key));
                if made_by_preferences {
                    stack.extend(self.supporting_firing(id, attr, value));
                }
            }
        }
        ground
    }

    fn supporting_firing(&self, id: SymbolId, attr: SymbolId, value: SymbolId) -> Option<MatchId> {
        self.preferences
            .iter()
            .find(|p| {
                p.id == id
                    && p.attr == attr
                    && p.value == value
                    && matches!(p.preference_type, PreferenceType::Acceptable | PreferenceType::Require)
                    && p.instantiation.is_some()
            })
            .and_then(|p| p.instantiation)
    }

    /// Retracts i-supported results whose justifying WMEs have left working memory.
    fn retract_invalid_results(&mut self) {
        let invalid: Vec<u64> = self
            .preferences
            .iter()
            .filter(|p| !p.o_supported && p.justification.as_ref().is_some_and(|keys| keys.iter().any(|k| self.wm.get(*k).is_none())))
            .map(|p| p.seq)
            .collect();
        self.remove_preferences(|p| p.justification.is_some() && invalid.contains(&p.seq));
    }

    /// Removes the preferences matching `remove` from preference memory, updating links and
    /// marking their slots changed. Returns them.
    pub(super) fn remove_preferences(&mut self, remove: impl Fn(&PreferenceRecord) -> bool) -> Vec<PreferenceRecord> {
        let (removed, kept): (Vec<PreferenceRecord>, Vec<PreferenceRecord>) =
            core::mem::take(&mut self.preferences).into_iter().partition(|p| remove(p));
        self.preferences = kept;
        for p in &removed {
            self.post_preference_links(p.id, p.value, p.preference_type, false);
            self.mark_slot_changed(p.id, p.attr);
        }
        removed
    }

    /// Upstream `retract_instantiation`: the match's i-supported preferences go away.
    fn retract_instantiation(&mut self, id: MatchId) {
        self.remove_preferences(|p| p.instantiation == Some(id) && !p.o_supported);
        self.decider.fired.retain(|(m, _, _)| *m != id);
    }

    /// Upstream `do_working_memory_phase`: non-context slots get one WME per acceptable,
    /// non-rejected value; operator slots get acceptable-preference WMEs for their
    /// acceptable and required values.
    pub(super) fn do_working_memory_phase(&mut self) {
        for (id, attr) in core::mem::take(&mut self.decider.changed_slots) {
            let rejected: Vec<SymbolId> = self
                .preferences
                .iter()
                .filter(|p| p.id == id && p.attr == attr && p.preference_type == PreferenceType::Reject)
                .map(|p| p.value)
                .collect();
            let mut wanted: Vec<(SymbolId, bool)> = Vec::new();
            for p in self.preferences.iter().filter(|p| p.id == id && p.attr == attr && p.preference_type == PreferenceType::Acceptable) {
                if rejected.contains(&p.value) {
                    continue;
                }
                match wanted.iter_mut().find(|(v, _)| *v == p.value) {
                    Some(entry) => entry.1 |= p.o_supported,
                    None => wanted.push((p.value, p.o_supported)),
                }
            }
            let existing: Vec<(SymbolId, WmeKey)> = self
                .decider
                .slot_wmes
                .iter()
                .filter(|w| w.0 == id && w.1 == attr)
                .map(|w| (w.2, w.3))
                .collect();
            for (value, key) in &existing {
                if !wanted.iter().any(|(v, _)| v == value) {
                    self.remove_wme(*key);
                }
            }
            for (value, o_supported) in wanted {
                if existing.iter().any(|(v, _)| *v == value) || self.wm.find(id, attr, value).is_some() {
                    continue;
                }
                let support = if o_supported { SupportType::OSupport } else { SupportType::ISupport };
                let key = self.insert_wme_with_flag(id, attr, value, false, support);
                self.decider.slot_wmes.push((id, attr, value, key));
                if o_supported {
                    self.update_gds_for_new_wme(key, id, attr, value);
                }
            }
        }

        let operator = self.symbols.predefined.operator;
        // Acceptable-preference WMEs of goals that have been removed.
        let stale: Vec<WmeKey> = self
            .decider
            .acceptable_wmes
            .iter()
            .filter(|w| self.goal_level(w.0).is_none())
            .map(|w| w.2)
            .collect();
        for key in stale {
            self.remove_wme_quietly(key);
        }
        for goal in self.decider.goals.iter().map(|g| g.id).collect::<Vec<_>>() {
            let mut wanted: Vec<SymbolId> = Vec::new();
            for p in self.preferences.iter().filter(|p| {
                p.id == goal && p.attr == operator && matches!(p.preference_type, PreferenceType::Acceptable | PreferenceType::Require)
            }) {
                if !wanted.contains(&p.value) {
                    wanted.push(p.value);
                }
            }
            let existing: Vec<(SymbolId, WmeKey)> =
                self.decider.acceptable_wmes.iter().filter(|w| w.0 == goal).map(|w| (w.1, w.2)).collect();
            for (value, key) in &existing {
                if !wanted.contains(value) {
                    self.remove_wme_quietly(*key);
                }
            }
            for value in wanted {
                if !existing.iter().any(|(v, _)| *v == value) {
                    let key = self.insert_wme_with_flag(goal, operator, value, true, SupportType::ISupport);
                    self.decider.acceptable_wmes.push((goal, value, key));
                }
            }
        }
        self.do_buffered_link_changes();
    }

    // ------------------------------------------------------------------
    // Goal dependency sets (upstream elaborate_gds, gds_invalid_so_remove_goal)
    // ------------------------------------------------------------------

    /// A slot's acceptable preferences in slot-list order.
    fn acceptable_records(&self, id: SymbolId, attr: SymbolId) -> Vec<PreferenceRecord> {
        let mut prefs: Vec<&PreferenceRecord> = self
            .preferences
            .iter()
            .filter(|p| p.id == id && p.attr == attr && p.preference_type == PreferenceType::Acceptable)
            .collect();
        let level = |p: &PreferenceRecord| self.pref_level(p);
        prefs.sort_by(|a, b| level(a).cmp(&level(b)).then(b.seq.cmp(&a.seq)));
        prefs.into_iter().cloned().collect()
    }

    /// Goal index of the goal a preference was made in (0 = top).
    fn pref_level(&self, p: &PreferenceRecord) -> usize {
        p.match_goal.and_then(|g| self.goal_level(g)).unwrap_or(0)
    }

    fn add_to_gds(&mut self, key: WmeKey, level: usize) {
        let goal = self.decider.goals[level].id;
        match self.decider.gds.iter().position(|(k, _)| *k == key) {
            Some(i) => {
                // Move it up from a lower goal's GDS (or a removed goal's).
                let owner = self.goal_level(self.decider.gds[i].1);
                if owner.is_none_or(|o| o > level) {
                    self.decider.gds[i].1 = goal;
                }
            }
            None => self.decider.gds.push((key, goal)),
        }
    }

    /// How a WME was made: by preferences (with the support of the first acceptable one and
    /// its slot), as an impasse `^item` (an architectural i-supported preference with no
    /// slot), or neither (other architectural WMEs and input).
    fn wme_preference(&self, key: WmeKey, id: SymbolId, attr: SymbolId, value: SymbolId) -> Option<(bool, Option<(SymbolId, SymbolId)>)> {
        let made_by_slot = self.decider.slot_wmes.iter().any(|w| w.3 == key)
            || self.decider.acceptable_wmes.iter().any(|w| w.2 == key)
            || self.decider.goals.iter().any(|g| g.operator.is_some_and(|(_, k)| k == key));
        if made_by_slot {
            let first = self.acceptable_records(id, attr).into_iter().find(|p| p.value == value);
            return Some((first.is_some_and(|p| p.o_supported), Some((id, attr))));
        }
        let is_item = self.decider.goals.iter().any(|g| g.impasse_wmes.contains(&key))
            && attr == self.symbols.predefined.item;
        is_item.then_some((false, None))
    }

    /// Upstream: when an o-supported WME is added by a substate rule, its goal gets a GDS,
    /// and the instantiations behind the WME are traced (`elaborate_gds`).
    fn update_gds_for_new_wme(&mut self, _key: WmeKey, id: SymbolId, attr: SymbolId, value: SymbolId) {
        let prefs = self.acceptable_records(id, attr);
        let Some(start) = prefs.iter().position(|p| p.value == value) else { return };
        let first = &prefs[start];
        let level = self.pref_level(first);
        if !first.o_supported || level == 0 || first.instantiation.is_none() {
            return;
        }
        // Upstream walks the slot's acceptable list from this WME's preference onward.
        let mut parents = Vec::new();
        for p in &prefs[start..] {
            if let Some(inst) = p.instantiation {
                if self.pref_level(p) == level && !self.decider.gds_evaluated.contains(&inst) {
                    self.decider.gds_evaluated.push(inst);
                    parents.push(inst);
                }
            }
        }
        self.elaborate_gds(parents, level);
    }

    /// Upstream `elaborate_gds`: a firing's conditions on higher-goal or architectural WMEs
    /// join the GDS; i-supported local WMEs are traced back to the firings that made them.
    fn elaborate_gds(&mut self, mut parents: Vec<MatchId>, level: usize) {
        let reach = self.reachable_from_goals_above(level);
        while let Some(inst) = parents.pop() {
            let keys = self.decider.fired.iter().find(|(m, _, _)| *m == inst).map(|(_, _, k)| k.clone()).unwrap_or_default();
            for key in keys {
                let Some(w) = self.wm.get(key).cloned() else { continue };
                let preference = self.wme_preference(key, w.id, w.attr, w.val);
                match preference {
                    _ if reach.contains(&w.id) => self.add_to_gds(key, level),
                    None => self.add_to_gds(key, level),
                    Some((true, _)) => {}
                    Some((false, _)) if level == 0 => {}
                    Some((false, None)) => {
                        // An impasse ^item stands for the superstate's acceptable preference WME.
                        let Some(item_goal) = self.goal_level(w.id).filter(|g| *g > 0) else { continue };
                        let superstate = self.decider.goals[item_goal - 1].id;
                        if let Some(&(_, _, k)) = self.decider.acceptable_wmes.iter().find(|a| a.0 == superstate && a.1 == w.val) {
                            self.add_to_gds(k, level);
                        }
                    }
                    Some((false, Some((slot_id, slot_attr)))) => {
                        for p in self.acceptable_records(slot_id, slot_attr) {
                            let Some(parent) = p.instantiation.filter(|_| p.value == w.val) else { continue };
                            if self.decider.gds_evaluated.contains(&parent) {
                                continue;
                            }
                            if self.pref_level(&p) <= level {
                                parents.push(parent);
                            }
                            self.decider.gds_evaluated.push(parent);
                        }
                    }
                }
            }
        }
    }

    /// Upstream `gds_invalid_so_remove_goal`: a WME in a live goal's GDS is going away, so the
    /// goal (and everything below it) is removed and the goal above is re-decided.
    pub(super) fn gds_invalid_so_remove_goal(&mut self, key: WmeKey) {
        let Some(&(_, goal)) = self.decider.gds.iter().find(|(k, _)| *k == key) else { return };
        let Some(level) = self.goal_level(goal).filter(|l| *l > 0) else { return };
        let higher = level - 1;
        match self.decider.highest_goal_whose_context_changed {
            Some(h) if h >= level => self.decider.highest_goal_whose_context_changed = Some(higher),
            Some(_) => {}
            None => {
                self.decider.highest_goal_whose_context_changed = Some(higher);
                self.decider.goals[higher].slot_changed = true;
            }
        }
        self.remove_existing_context_and_descendents(level);
    }

    /// The WMEs in a goal's dependency set.
    pub fn goal_dependency_set(&self, goal: SymbolId) -> Vec<WmeKey> {
        self.decider.gds.iter().filter(|(_, g)| *g == goal).map(|(k, _)| *k).collect()
    }

    // ------------------------------------------------------------------
    // Context decisions (upstream decide_context_slots / decide_context_slot)
    // ------------------------------------------------------------------

    fn context_slot_is_decidable(&self, goal: usize) -> bool {
        let g = &self.decider.goals[goal];
        g.operator.is_none() && g.slot_changed
    }

    pub(super) fn decide_context_slots(&mut self) -> DecisionResult {
        if self.decider.goals.is_empty() {
            return DecisionResult::NoChangeImpasse;
        }
        let mut goal = self.decider.highest_goal_whose_context_changed.unwrap_or(self.decider.goals.len() - 1);
        let result = loop {
            while !self.context_slot_is_decidable(goal) && goal + 1 < self.decider.goals.len() {
                goal += 1;
            }
            let (done, result) = self.decide_context_slot(goal);
            if done {
                break result;
            }
        };
        self.decider.highest_goal_whose_context_changed = None;
        self.sync_goal_records();
        result
    }

    /// Decides one goal's operator slot. Returns `false` (with the result) when the only
    /// change was to the `^item`s of an existing impasse, as upstream does.
    fn decide_context_slot(&mut self, goal: usize) -> (bool, DecisionResult) {
        let mut selected_value = None;
        let (impasse, candidates) = if !self.context_slot_is_decidable(goal) {
            // Only the bottom slot is decided when undecidable: a no-change impasse.
            (Some(ImpasseType::NoChange), Vec::new())
        } else {
            let semantics = self.operator_slot_semantics(goal, false);
            self.remove_wmes_for_context_slot(goal);
            selected_value = semantics.rl.map(|choice| choice.selected_value);
            match (semantics.impasse, semantics.candidates.is_empty()) {
                (None, true) => (Some(ImpasseType::NoChange), Vec::new()),
                (impasse, _) => (impasse, semantics.candidates),
            }
        };
        self.decider.goals[goal].slot_changed = false;

        let operator = self.symbols.predefined.operator;
        let state = self.symbols.predefined.state;
        let attr = match impasse {
            Some(ImpasseType::NoChange) if self.decider.goals[goal].operator.is_none() => state,
            _ => operator,
        };
        if attr == state {
            self.remove_wmes_for_context_slot(goal);
        }

        let Some(impasse) = impasse else {
            let winner = candidates[0];
            if goal + 1 < self.decider.goals.len() {
                self.remove_existing_context_and_descendents(goal + 1);
            }
            let id = self.decider.goals[goal].id;
            let key = self.insert_wme_with_flag(id, operator, winner, false, SupportType::ISupport);
            self.decider.goals[goal].operator = Some((winner, key));
            self.sync_goal_records();
            if let Some(value) = selected_value {
                self.rl_store_data(goal, winner, value);
            }
            return (true, DecisionResult::Selected(winner));
        };

        let result = match impasse {
            ImpasseType::Tie => DecisionResult::TieImpasse(candidates.clone()),
            ImpasseType::Conflict => DecisionResult::ConflictImpasse(candidates.clone()),
            ImpasseType::ConstraintFailure => DecisionResult::ConstraintFailureImpasse(candidates.clone()),
            ImpasseType::NoChange => DecisionResult::NoChangeImpasse,
        };
        if self.type_of_existing_impasse(goal) == Some((impasse, attr)) {
            self.update_impasse_items(goal + 1, &candidates);
            return (false, result);
        }
        if goal + 1 < self.decider.goals.len() {
            self.remove_existing_context_and_descendents(goal + 1);
        }
        self.create_new_context(Some(attr), Some(impasse));
        let new_goal = self.decider.goals.len() - 1;
        self.update_impasse_items(new_goal, &candidates);
        (true, result)
    }

    // ------------------------------------------------------------------
    // Consistency (upstream consistency.cpp)
    // ------------------------------------------------------------------

    /// Upstream `decision_consistent_with_current_preferences`.
    fn decision_consistent(&mut self, goal: usize) -> bool {
        let operator = self.decider.goals[goal].operator.map(|(op, _)| op);
        let current = match self.type_of_existing_impasse(goal) {
            // An operator no-change counts as "no impasse": only a better operator or another
            // kind of impasse makes it inconsistent.
            Some((ImpasseType::NoChange, _)) if operator.is_some() => None,
            Some((t, _)) => Some(t),
            None => None,
        };
        let semantics = self.operator_slot_semantics(goal, true);
        if current != semantics.impasse {
            return false;
        }
        match semantics.impasse {
            None => match operator {
                Some(op) => semantics.candidates.contains(&op),
                None => goal + 1 >= self.decider.goals.len(),
            },
            Some(_) => true,
        }
    }

    /// Upstream `check_context_slot_decisions`: re-checks changed slots from the highest
    /// changed goal down to `level`; an inconsistent decision is removed.
    fn check_context_slot_decisions(&mut self, level: usize) -> bool {
        let Some(mut goal) = self.decider.highest_goal_whose_context_changed else { return true };
        while goal < self.decider.goals.len() && goal <= level {
            let g = &self.decider.goals[goal];
            if (goal + 1 < self.decider.goals.len() || g.operator.is_some()) && g.slot_changed && !self.decision_consistent(goal) {
                self.remove_wmes_for_context_slot(goal);
                if goal + 1 < self.decider.goals.len() {
                    self.remove_existing_context_and_descendents(goal + 1);
                }
                self.do_buffered_link_changes();
                return false;
            }
            goal += 1;
        }
        true
    }

    // ------------------------------------------------------------------
    // Elaboration waves (upstream do_preference_phase and the
    // determine_highest_active_production_level_in_stack_* routines)
    // ------------------------------------------------------------------

    fn goal_index_or_top(&self, goal: Option<SymbolId>) -> usize {
        goal.and_then(|g| self.goal_level(g)).unwrap_or(0)
    }

    fn activity(&mut self) -> Activity {
        let mut assertions = Vec::new();
        let pending = self.rete.pending_matches();
        for m in &pending {
            let goal = self.goal_index_or_top(self.match_goal(&m.matched_wmes));
            let o_support = self.calculate_support(m.production, &m.matched_wmes, &m.levels, &[], false);
            assertions.push((m.token, goal, if o_support { FiringType::Pe } else { FiringType::Ie }));
        }
        let retractions = self
            .rete
            .retracting_matches()
            .into_iter()
            .map(|id| {
                let goal = self.decider.fired.iter().find(|(m, _, _)| *m == id).map_or(FiredGoal::Top, |(_, g, _)| *g);
                let index = match goal {
                    FiredGoal::Goal(g) => self.goal_level(g),
                    FiredGoal::Top => Some(0),
                    FiredGoal::Removed => None,
                };
                (id, index)
            })
            .collect();
        Activity { assertions, pending, retractions }
    }

    fn goal_count(&self) -> usize {
        self.decider.goals.len().max(1)
    }

    fn highest_active_goal(&self, activity: &Activity, apply: bool) -> Option<usize> {
        (0..self.goal_count()).find(|&g| if apply { activity.any_activity(g) } else { activity.i_activity(g) })
    }

    fn minor_quiescence_at_goal(&self, activity: &Activity, goal: usize) -> bool {
        self.decider.firing_type == FiringType::Ie && !activity.i_activity(goal)
    }

    fn active_production_type(activity: &Activity, goal: usize) -> FiringType {
        if activity.i_activity(goal) {
            FiringType::Ie
        } else {
            FiringType::Pe
        }
    }

    /// Upstream `determine_highest_active_production_level_in_stack_{propose,apply}`.
    /// Returns `false` when the phase is over.
    fn determine_active_level(&mut self, apply: bool) -> bool {
        let activity = self.activity();
        let bottom = self.goal_count() - 1;
        let ready = if apply {
            !activity.assertions.is_empty() || !activity.retractions.is_empty()
        } else {
            !activity.retractions.is_empty() || activity.assertions.iter().any(|a| a.2 == FiringType::Ie)
        };
        if !ready {
            if self.minor_quiescence_at_goal(&activity, bottom) {
                self.check_context_slot_decisions(bottom);
            }
            return false;
        }
        if self.decider.e_cycles_this_d_cycle >= self.max_elaborations {
            self.output.push_str("Warning: reached max-elaborations; proceeding to next phase.\n");
            return false;
        }

        let previous_goal = self.decider.active_goal;
        let previous_level = self.decider.active_level;
        let active = self.highest_active_goal(&activity, apply);
        if active.is_none() && !activity.nil_goal_retractions() {
            return false;
        }
        self.decider.active_goal = active;
        self.decider.active_level = active.map_or(0, |g| g + 1);

        let Some(goal) = active else {
            self.decider.firing_type = FiringType::Ie;
            return true;
        };
        let level = goal + 1;
        if previous_level == 0 {
            self.decider.firing_type = if apply { Self::active_production_type(&activity, goal) } else { FiringType::Ie };
        } else if level > previous_level {
            let previous = previous_goal.unwrap_or(goal);
            if (!apply || self.minor_quiescence_at_goal(&activity, previous)) && !self.check_context_slot_decisions(previous) {
                return false;
            }
            if apply {
                if let Some(saved) = self.decider.goals.get(goal).and_then(|g| g.saved_firing_type) {
                    // Upstream resumes the saved firing type and re-runs this routine, which
                    // then takes the same-level path.
                    self.decider.firing_type = saved;
                    if self.minor_quiescence_at_goal(&activity, goal) && !self.check_context_slot_decisions(goal) {
                        return false;
                    }
                    self.decider.firing_type = Self::active_production_type(&activity, goal);
                    return true;
                }
                self.decider.firing_type = Self::active_production_type(&activity, goal);
            } else {
                self.decider.firing_type = FiringType::Ie;
            }
        } else if level == previous_level {
            if apply {
                if self.minor_quiescence_at_goal(&activity, goal) && !self.check_context_slot_decisions(goal) {
                    return false;
                }
                self.decider.firing_type = Self::active_production_type(&activity, goal);
            } else {
                self.decider.firing_type = FiringType::Ie;
            }
        } else {
            if let Some(g) = previous_goal.and_then(|p| self.decider.goals.get_mut(p)) {
                g.saved_firing_type = Some(self.decider.firing_type);
            }
            if !self.check_context_slot_decisions(goal) {
                return false;
            }
            self.decider.firing_type = if apply { Self::active_production_type(&activity, goal) } else { FiringType::Ie };
        }
        true
    }

    /// Goal level (1 = top) of each identifier linked to the goal stack (upstream `id->level`).
    fn identifier_levels(&self) -> Vec<(SymbolId, usize)> {
        self.decider.links.known_levels()
    }

    /// Upstream `shouldCreateInstantiation`: below the highest active goal, a match is
    /// postponed if any of its actions would change an identifier at or above `change_level`.
    fn should_create_instantiation(&self, m: &crate::rete::PendingMatch, change_level: usize, levels: &[(SymbolId, usize)]) -> bool {
        use super::rules::{ActionKind, FlatAction};
        use super::FlatTerm;
        use crate::production::compile::{CompiledAction, RhsTerm};
        let bound = |loc: crate::rete::VarLocation| {
            m.levels.get(loc.depth.wrapping_sub(1)).and_then(|l| l.as_ref()).map(|triple| loc.field.extract(triple))
        };
        let action_id = |action: FlatAction| -> Option<SymbolId> {
            match (action.kind, action.terms[0]) {
                (ActionKind::Add | ActionKind::Make, FlatTerm::Constant(s)) => Some(s),
                (ActionKind::Make, FlatTerm::Bound(loc)) => bound(loc),
                // New identifiers (upstream's unbound variables) never block.
                (ActionKind::Add | ActionKind::Make, FlatTerm::NewIdentifier(_)) => None,
                (ActionKind::Add, FlatTerm::Bound(_)) => None,
                (ActionKind::Prefer, _) => Some(action.prefer.state),
                (ActionKind::Remove, _) => None,
                (ActionKind::Complex, _) => match self.rules.complex(action.index) {
                    CompiledAction::Make { id: RhsTerm::Constant(s), .. } => Some(*s),
                    CompiledAction::Make { id: RhsTerm::Bound(loc), .. } => bound(*loc),
                    _ => None,
                },
            }
        };
        !(0..self.rules.action_count(m.production))
            .filter_map(|index| action_id(self.rules.action(m.production, index)))
            .any(|id| levels.iter().any(|(v, level)| *v == id && *level <= change_level))
    }

    /// One elaboration cycle (upstream `do_preference_phase`), with its inner waterfall loop:
    /// the highest active goal fires its matches of the current firing type, then each lower
    /// goal with activity fires those of its matches that change nothing at or above the
    /// deepest level that has fired so far. Retractions are then processed for the highest
    /// active goal and for goals that are gone. Returns the number of firings.
    fn preference_phase(&mut self) -> usize {
        let activity = self.activity();
        let apply = self.current_phase == super::Phase::Application;
        let mut fired = 0;
        if let Some(highest) = self.decider.active_goal {
            let firing_type = self.decider.firing_type;
            let levels = self.identifier_levels();
            let highest_level = highest + 1;
            let mut next_change_level = highest_level;
            let mut goal = Some(highest);
            while let Some(g) = goal {
                let change_level = next_change_level;
                let active_level = g + 1;
                let mut assertions_exist = false;
                let mut new_preferences = Vec::new();
                for (i, &(token, _, _)) in activity.assertions.iter().enumerate().filter(|(_, a)| a.1 == g && a.2 == firing_type) {
                    assertions_exist = true;
                    let pending = activity.pending[i].clone();
                    if active_level != highest_level && !self.should_create_instantiation(&pending, change_level, &levels) {
                        continue;
                    }
                    let Some(inst) = self.rete.fire_match(token) else { continue };
                    let match_goal = self.match_goal(&inst.matched_wmes).map_or(FiredGoal::Top, FiredGoal::Goal);
                    let keys = inst.matched_wmes.iter().map(|w| w.key).collect();
                    self.decider.fired.push((inst.id, match_goal, keys));
                    new_preferences.extend(self.create_instantiation(&inst));
                    fired += 1;
                }
                if assertions_exist && active_level > next_change_level {
                    next_change_level = active_level;
                }
                self.assert_preferences(new_preferences);
                goal = (g + 1..self.goal_count()).find(|&x| if apply { activity.any_activity(x) } else { activity.i_activity(x) });
            }
        }

        self.retract_invalid_results();
        let active = self.decider.active_goal;
        for (id, goal) in activity.retractions {
            if goal.is_none() || (active.is_some() && goal == active) {
                if let Some(inst) = self.rete.take_retraction(id) {
                    self.retract_instantiation(inst.id);
                }
            }
        }
        fired
    }

    /// Runs elaboration cycles for the propose (`apply == false`) or apply phase.
    pub(super) fn run_elaboration_waves(&mut self, apply: bool) -> usize {
        // Upstream initialize_consistency_calculations_for_new_decision.
        self.decider.active_level = 0;
        self.decider.active_goal = None;
        for goal in &mut self.decider.goals {
            goal.saved_firing_type = None;
        }
        self.decider.firing_type = if apply { FiringType::Pe } else { FiringType::Ie };
        // Changes made directly to working memory (input) since the last phase.
        self.do_buffered_link_changes();

        let mut fired = 0;
        let mut running = self.determine_active_level(apply);
        while running {
            fired += self.preference_phase();
            self.do_working_memory_phase();
            self.decider.e_cycles_this_d_cycle += 1;
            if self.halted {
                break;
            }
            running = self.determine_active_level(apply);
        }
        self.sync_goal_records();
        fired
    }
}
