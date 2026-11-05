//! Working-memory activation, the agent's side (upstream `working_memory_activation.cpp` and
//! its calls in `decide.cpp`, `preference.cpp` and `run_soar.cpp`).
//!
//! An o-supported WME is referenced when a rule match tests it, when a rule asserts a
//! preference for it again, and through the i-supported WMEs that depend on it: an i-supported
//! WME's preference keeps an "o-set" of the o-supported (and architectural) WMEs its
//! instantiation tested, and a reference to it references those. Each decision's references
//! join the WME's history in the output phase, and forgetting removes o-supported WMEs whose
//! activation has fallen below `decay-thresh`.
//!
//! Upstream keeps a pointer from each WME to the preference that made it. Rusty-Soar has no such
//! pointer, so the preference each WME's slot gives it is recorded here when the decider sets
//! it, with the conditions of the instantiation behind each preference. Upstream frees a WME,
//! and its decay element, when nothing refers to it: working memory, an instantiation that
//! tested it, or an o-set. Here a decay element goes when none of these holds it any more,
//! checked before each decision's histories are updated.

use super::SoarAgent;
use crate::pool::Layout;
use crate::preference::PreferenceType;
use crate::symbol::SymbolId;
use crate::wm::{Wme, WmeKey};
use crate::wma::{self as w, Caches, ForgetWme, Forgetting, History, WmaParams, WmaQuery, WmaSetting};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;

/// Ids of the architectural preferences behind impasse `^item`s, apart from rule preferences'.
const ARCHITECTURAL: u64 = 1 << 63;

/// Upstream `wma_decay_element`.
#[derive(Debug, Clone)]
struct DecayElement {
    /// The WME's timetag, for the trace after it has left working memory.
    timetag: u64,
    just_created: bool,
    /// References this decision, added to the history in the output phase.
    num_references: u64,
    touches: History,
    /// When forgetting next checks it (`u64::MAX` until it is queued).
    forget_cycle: u64,
}

/// Upstream's `w->preference`: the preference a WME's slot gives it.
#[derive(Debug, Clone, Copy)]
struct WmePreference {
    /// The preference's `seq`, or an architectural id.
    id: u64,
    o_supported: bool,
    /// The WME keeps the preference alive, as upstream's operator and impasse `^item` WMEs do.
    held: bool,
}

/// Working-memory activation state (upstream's `wma_*` members of working memory).
#[derive(Debug, Default)]
pub struct Wma {
    /// The `wma` settings.
    pub params: WmaParams,
    /// Present while activation is on (upstream `wma_initialized`).
    caches: Option<Caches>,
    /// Upstream `wma_d_cycle_count`.
    pub d_cycle_count: u64,
    elements: BTreeMap<WmeKey, DecayElement>,
    touched: BTreeSet<WmeKey>,
    /// Upstream `wma_forget_pq`: elements by the decision forgetting next checks them.
    forget_pq: BTreeMap<u64, BTreeSet<WmeKey>>,
    touched_sets: BTreeSet<u64>,
    /// Each i-supported preference's o-set.
    o_sets: BTreeMap<u64, BTreeSet<WmeKey>>,
    /// Upstream `wma_val_references`: references to values a slot has no WME for yet.
    val_refs: BTreeMap<(SymbolId, SymbolId), BTreeMap<SymbolId, u64>>,
    wme_prefs: BTreeMap<WmeKey, WmePreference>,
    /// The positive conditions of the instantiation behind each preference.
    pref_conds: BTreeMap<u64, Vec<WmeKey>>,
    /// Preferences alive at the last check, with those asserted since.
    live: BTreeSet<u64>,
    next_architectural: u64,
    /// WMEs removed from working memory, for the forgotten-WMEs statistic.
    pub(super) removals: u64,
    /// The `forgotten-wmes` statistic.
    pub forgotten_wmes: i64,
    /// `trace --wma`.
    pub trace: bool,
}

impl<S: Layout> SoarAgent<S> {
    /// Upstream `wma_enabled`.
    pub fn wma_enabled(&self) -> bool {
        self.wma.params.activation
    }

    /// Carries out `wma --set`. Settings other than `activation` are protected while it is on.
    pub(super) fn set_wma(&mut self, setting: WmaSetting) {
        if setting.protected() && self.wma_enabled() {
            self.output.push_str("ERROR: this parameter is protected while WMA is on.\n");
            return;
        }
        let was = self.wma_enabled();
        self.wma.params.apply(setting);
        match (was, self.wma_enabled()) {
            (false, true) => self.wma_init(),
            (true, false) => self.wma_deinit(),
            _ => {}
        }
    }

    /// Carries out `wma` with no option, `--get`, `--history` or `--stats`.
    pub(super) fn wma_query(&mut self, query: &WmaQuery) {
        let text = match query {
            WmaQuery::Settings => {
                let p = &self.wma.params;
                let get = |name: &str| p.get(name).unwrap_or_default();
                alloc::format!(
                    "\nWMA activation: {}\n\nActivation\n----------\ndecay-rate: {}\npetrov-approx: {}\n\nForgetting\n----------\ndecay-thresh: {}\nforgetting: {}\nforget-wme: {}\nfake-forgetting: {}\n\nPerformance\n-----------\ntimers: {}\nmax-pow-cache: {}\n\n",
                    get("activation"),
                    get("decay-rate"),
                    get("petrov-approx"),
                    get("decay-thresh"),
                    get("forgetting"),
                    get("forget-wme"),
                    get("fake-forgetting"),
                    get("timers"),
                    get("max-pow-cache"),
                )
            }
            WmaQuery::Get(name) => match self.wma.params.get(name) {
                Some(value) => value,
                None => String::from("Invalid activation setting."),
            },
            WmaQuery::History(timetag) => match self.wm.iter().find(|w| w.timetag == *timetag).map(|w| w.key) {
                Some(key) => self.wma_history(key),
                None => String::new(),
            },
            WmaQuery::Stats(None) => alloc::format!("Forgotten WMEs: {}\n", self.wma.forgotten_wmes),
            WmaQuery::Stats(Some(name)) if name == "forgotten-wmes" => alloc::format!("{}", self.wma.forgotten_wmes),
            WmaQuery::Stats(Some(_)) => String::from("Invalid statistic."),
        };
        self.output.push_str(&text);
    }

    /// Upstream `wma_init`.
    fn wma_init(&mut self) {
        if self.wma.caches.is_none() {
            self.wma.caches = Some(Caches::new(&self.wma.params));
        }
    }

    /// Upstream `wma_deinit`: decay elements stay, as upstream's stay on their WMEs.
    fn wma_deinit(&mut self) {
        if self.wma.caches.take().is_some() {
            self.wma.touched.clear();
            self.wma.touched_sets.clear();
            self.wma.forget_pq.clear();
        }
    }

    /// Upstream `wma_get_wme_activation`: the WME's activation (`log`) or time sum, or
    /// `None` while activation is off.
    pub fn wme_activation(&self, key: WmeKey, log: bool) -> Option<f64> {
        let caches = self.wma.caches.as_ref()?;
        Some(self.wma_activation_with(caches, key, log))
    }

    fn wma_activation_with(&self, caches: &Caches, key: WmeKey, log: bool) -> f64 {
        match self.wma.elements.get(&key) {
            Some(el) => caches.decay_activation(&self.wma.params, &el.touches, self.wma.d_cycle_count, log),
            None if log => w::ACTIVATION_NONE,
            None => w::TIME_SUM_NONE,
        }
    }

    fn wma_time_sum(&self, key: WmeKey) -> f64 {
        match &self.wma.caches {
            Some(caches) => self.wma_activation_with(caches, key, false),
            None => w::TIME_SUM_NONE,
        }
    }

    /// Upstream `wma_get_wme_history`.
    fn wma_history(&self, key: WmeKey) -> String {
        let Some(el) = self.wma.elements.get(&key) else { return String::from("WME has no decay history") };
        let h = &el.touches;
        let mut text = alloc::format!("history ({}/{}, first @ d{}):", h.history_references, h.total_references, h.first_reference);
        let mut p = h.next_p;
        for _ in 0..h.history_ct {
            p = w::history_prev(p);
            text.push_str("\n ");
            text.push_str(&w::reference_to_string(&h.access_history[p], self.wma.d_cycle_count));
        }
        if matches!(self.wma.params.forgetting, Forgetting::Bsearch | Forgetting::Approx) {
            text.push_str(&alloc::format!("\n\nconsidering WME for decay @ d{}", el.forget_cycle as i64));
        }
        text
    }

    // ------------------------------------------------------------------
    // Which preference each WME has (upstream `w->preference`)
    // ------------------------------------------------------------------

    /// Recomputes which preferences are alive: those in preference memory, and those a WME in
    /// working memory holds.
    fn wma_refresh_live(&mut self) {
        let mut live: BTreeSet<u64> = self.preferences.iter().map(|p| p.seq).collect();
        for (key, pref) in &self.wma.wme_prefs {
            if pref.held && self.wm.get(*key).is_some() {
                live.insert(pref.id);
            }
        }
        self.wma.live = live;
    }

    /// The first preference of `types` for `(id ^attr value)` in slot-list order.
    fn wma_first_preference(&self, id: SymbolId, attr: SymbolId, value: SymbolId, types: &[PreferenceType]) -> Option<WmePreference> {
        for t in types {
            if let Some(p) = self.slot_preference_records(id, attr).into_iter().find(|p| p.value == value && p.preference_type == *t) {
                return Some(WmePreference { id: p.seq, o_supported: p.o_supported, held: false });
            }
        }
        None
    }

    /// A non-context slot WME's preference: its value's first acceptable preference. A new WME
    /// is also given the references its slot collected for its value.
    pub(super) fn wma_slot_wme(&mut self, key: WmeKey, id: SymbolId, attr: SymbolId, value: SymbolId, new: bool) {
        match self.wma_first_preference(id, attr, value, &[PreferenceType::Acceptable]) {
            Some(pref) => {
                self.wma.wme_prefs.insert(key, pref);
            }
            None => {
                self.wma.wme_prefs.remove(&key);
            }
        }
        if !new {
            return;
        }
        let Some(refs) = self.wma.val_refs.get_mut(&(id, attr)) else { return };
        let Some(count) = refs.remove(&value) else { return };
        if refs.is_empty() {
            self.wma.val_refs.remove(&(id, attr));
        }
        self.wma_refresh_live();
        // Only an o-supported WME is activated here.
        self.wma_activate_wme(key, count, None, true);
    }

    /// An acceptable-preference WME's preference: the first require, else the first acceptable.
    pub(super) fn wma_acceptable_wme(&mut self, key: WmeKey, goal: SymbolId, value: SymbolId) {
        let operator = self.symbols.predefined.operator;
        match self.wma_first_preference(goal, operator, value, &[PreferenceType::Require, PreferenceType::Acceptable]) {
            Some(pref) => self.wma.wme_prefs.insert(key, pref),
            None => self.wma.wme_prefs.remove(&key),
        };
    }

    /// The selected operator's WME holds the candidate preference that won.
    pub(super) fn wma_operator_wme(&mut self, key: WmeKey, goal: SymbolId, value: SymbolId) {
        let operator = self.symbols.predefined.operator;
        if let Some(mut pref) = self.wma_first_preference(goal, operator, value, &[PreferenceType::Require, PreferenceType::Acceptable]) {
            pref.held = true;
            self.wma.wme_prefs.insert(key, pref);
        }
    }

    /// An impasse `^item` WME's preference: an architectural one whose instantiation tests the
    /// superstate's acceptable-preference WME for the item and the `^superstate` WME (upstream
    /// `make_architectural_instantiation_for_impasse_item`).
    pub(super) fn wma_item_wme(&mut self, key: WmeKey, conditions: Vec<WmeKey>) {
        let id = ARCHITECTURAL | self.wma.next_architectural;
        self.wma.next_architectural += 1;
        self.wma.pref_conds.insert(id, conditions);
        self.wma.wme_prefs.insert(key, WmePreference { id, o_supported: false, held: true });
    }

    /// Upstream `add_preference_to_tm`'s and `assert_new_preferences`' parts: a slot collects
    /// references to values it has no WME for, and an acceptable preference references the
    /// WMEs it is for.
    pub(super) fn wma_preference_added(&mut self, seq: u64, slot_was_changed: bool) {
        let Some(p) = self.preferences.iter().find(|p| p.seq == seq).cloned() else { return };
        let conditions = match (&p.justification, p.instantiation) {
            (Some(ground), _) => ground.clone(),
            (None, Some(inst)) => self.decider.fired.iter().find(|(m, _, _)| *m == inst).map(|(_, _, k)| k.clone()).unwrap_or_default(),
            (None, None) => Vec::new(),
        };
        self.wma.pref_conds.insert(seq, conditions);
        self.wma.live.insert(seq);
        let operator = self.symbols.predefined.operator;
        let context = p.attr == operator && self.goal_level(p.id).is_some();
        if !context {
            if !slot_was_changed {
                if let Some(refs) = self.wma.val_refs.get_mut(&(p.id, p.attr)) {
                    refs.clear();
                }
            }
            let exists = self.decider.slot_wmes.iter().any(|s| s.0 == p.id && s.1 == p.attr && s.2 == p.value);
            if !exists {
                *self.wma.val_refs.entry((p.id, p.attr)).or_default().entry(p.value).or_default() += 1;
            }
        }
        // Upstream `wma_activate_wmes_in_pref`.
        if p.preference_type == PreferenceType::Acceptable {
            let wmes: Vec<WmeKey> = if context {
                self.decider.goals.iter().filter(|g| g.id == p.id).filter_map(|g| g.operator).filter(|(op, _)| *op == p.value).map(|(_, k)| k).collect()
            } else {
                self.decider.slot_wmes.iter().filter(|s| s.0 == p.id && s.1 == p.attr && s.2 == p.value).map(|s| s.3).collect()
            };
            for key in wmes {
                self.wma_activate_wme(key, 1, None, false);
            }
        }
    }

    // ------------------------------------------------------------------
    // References (upstream wma_activate_wme and its callers)
    // ------------------------------------------------------------------

    /// Upstream `wma_activate_wmes_tested_in_prods`: each WME the pending matches test.
    pub(super) fn wma_activate_wmes(&mut self, keys: &[WmeKey]) {
        self.wma_refresh_live();
        for key in keys {
            self.wma_activate_wme(*key, 1, None, false);
        }
    }

    /// Upstream `wma_activate_wme`. With `o_set`, the WME (or the o-set behind it) joins that
    /// preference's o-set instead of being referenced. With `o_only`, only an o-supported WME
    /// is activated.
    fn wma_activate_wme(&mut self, key: WmeKey, num_references: u64, o_set: Option<u64>, o_only: bool) {
        let Some(pref) = self.wma.wme_prefs.get(&key).copied() else {
            // Architectural: it only joins o-sets.
            if let Some(owner) = o_set.filter(|_| !o_only) {
                self.wma.o_sets.entry(owner).or_default().insert(key);
            }
            return;
        };
        // A preference that has gone supports nothing.
        if !self.wma.live.contains(&pref.id) {
            return;
        }
        if pref.o_supported {
            if !self.wma.elements.contains_key(&key) {
                let Some(wme) = self.wm.get(key).cloned() else { return };
                let boost = self.wma_initial_boost(&wme);
                self.wma.elements.insert(
                    key,
                    DecayElement { timetag: wme.timetag, just_created: true, num_references: boost, touches: History::default(), forget_cycle: u64::MAX },
                );
                if self.wma.trace {
                    self.wma_trace_add(&wme);
                }
            }
            match o_set {
                Some(owner) => {
                    self.wma.o_sets.entry(owner).or_default().insert(key);
                }
                None => {
                    if let Some(el) = self.wma.elements.get_mut(&key) {
                        el.num_references += num_references;
                    }
                    self.wma.touched.insert(key);
                }
            }
        } else if !o_only {
            if let alloc::collections::btree_map::Entry::Vacant(entry) = self.wma.o_sets.entry(pref.id) {
                entry.insert(BTreeSet::new());
                let conditions = self.wma.pref_conds.get(&pref.id).cloned().unwrap_or_default();
                for condition in conditions {
                    self.wma_activate_wme(condition, 0, Some(pref.id), false);
                }
            }
            let members: Vec<WmeKey> = self.wma.o_sets.get(&pref.id).map(|s| s.iter().copied().collect()).unwrap_or_default();
            for member in members {
                match o_set {
                    Some(owner) => {
                        self.wma.o_sets.entry(owner).or_default().insert(member);
                    }
                    None => {
                        if let Some(el) = self.wma.elements.get_mut(&member) {
                            el.num_references += num_references;
                            self.wma.touched.insert(member);
                        }
                    }
                }
            }
        }
    }

    /// Upstream `wma_calculate_initial_boost`: a new element starts with the average time sum
    /// of the WMEs the instantiations behind its o-supported preferences tested.
    fn wma_initial_boost(&self, wme: &Wme) -> u64 {
        let mut seen: BTreeSet<WmeKey> = BTreeSet::new();
        let mut count = 0u64;
        let mut sum = 0.0;
        for p in self.preferences.iter().filter(|p| {
            p.id == wme.id && p.attr == wme.attr && p.value == wme.val && p.preference_type == PreferenceType::Acceptable && p.o_supported
        }) {
            for &condition in self.wma.pref_conds.get(&p.seq).map(Vec::as_slice).unwrap_or(&[]) {
                if !seen.insert(condition) {
                    continue;
                }
                if let Some(el) = self.wma.elements.get(&condition) {
                    if !el.just_created {
                        count += 1;
                        sum += self.wma_time_sum(condition);
                    }
                } else if let Some(pref) = self.wma.wme_prefs.get(&condition) {
                    for &member in self.wma.o_sets.get(&pref.id).into_iter().flatten() {
                        if !seen.contains(&member) && self.wma.elements.get(&member).is_none_or(|el| !el.just_created) {
                            count += 1;
                            sum += self.wma_time_sum(member);
                            seen.insert(member);
                        }
                    }
                } else {
                    count += 1;
                    sum += self.wma_time_sum(condition);
                }
            }
        }
        if count == 0 { 0 } else { libm::floor(sum / count as f64) as u64 }
    }

    // ------------------------------------------------------------------
    // Histories and forgetting (upstream wma_go)
    // ------------------------------------------------------------------

    /// Upstream `deallocate_wme`'s `wma_remove_decay_element`, for every WME nothing refers to
    /// any more: not working memory, an instantiation that tested it or a live o-set.
    fn wma_collect(&mut self) {
        self.wma_refresh_live();
        let live = &self.wma.live;
        self.wma.o_sets.retain(|id, _| live.contains(id));
        self.wma.pref_conds.retain(|id, _| live.contains(id));
        let mut referenced: BTreeSet<WmeKey> = BTreeSet::new();
        referenced.extend(self.wma.pref_conds.values().flatten().copied());
        referenced.extend(self.wma.o_sets.values().flatten().copied());
        referenced.extend(self.decider.fired.iter().flat_map(|(_, _, keys)| keys.iter().copied()));
        let gone: Vec<WmeKey> = self
            .wma
            .elements
            .keys()
            .chain(self.wma.wme_prefs.keys())
            .copied()
            .filter(|k| self.wm.get(*k).is_none() && !referenced.contains(k))
            .collect();
        for key in gone {
            self.wma.wme_prefs.remove(&key);
            if let Some(el) = self.wma.elements.remove(&key) {
                self.wma.touched.remove(&key);
                if matches!(self.wma.params.forgetting, Forgetting::Approx | Forgetting::Bsearch) {
                    self.wma_remove_from_p_queue(key, el.forget_cycle);
                }
                if self.wma.trace {
                    self.output.push_str(&alloc::format!("WMA @{}: remove {}\n", self.wma.d_cycle_count, el.timetag));
                }
            }
        }
    }

    /// Upstream `wma_update_decay_histories`.
    fn wma_update_decay_histories(&mut self) {
        let current_cycle = self.wma.d_cycle_count;
        let forgetting = matches!(self.wma.params.forgetting, Forgetting::Approx | Forgetting::Bsearch);
        for key in core::mem::take(&mut self.wma.touched) {
            let Some(el) = self.wma.elements.get_mut(&key) else { continue };
            let h = &mut el.touches;
            h.history_references = h.history_references.wrapping_add(el.num_references.wrapping_sub(h.access_history[h.next_p].num_references));
            h.access_history[h.next_p] = w::CycleReference { num_references: el.num_references, d_cycle: current_cycle };
            if h.total_references == 0 {
                h.first_reference = current_cycle;
            }
            if h.history_ct < w::DECAY_HISTORY {
                h.history_ct += 1;
            }
            h.next_p = w::history_next(h.next_p);
            h.total_references += el.num_references;
            let references = el.num_references;
            el.num_references = 0;
            let (just_created, timetag) = (el.just_created, el.timetag);
            if self.wma.trace {
                self.output.push_str(&alloc::format!("WMA @{}: activate {} {}\n", current_cycle, timetag, references));
            }
            if forgetting {
                let cycle = self.wma_forgetting_estimate_cycle(key, true);
                if just_created {
                    self.wma_add_to_p_queue(key, cycle);
                } else {
                    self.wma_move_in_p_queue(key, cycle);
                }
            }
            if let Some(el) = self.wma.elements.get_mut(&key) {
                el.just_created = false;
            }
        }
    }

    fn wma_add_to_p_queue(&mut self, key: WmeKey, cycle: u64) {
        if let Some(el) = self.wma.elements.get_mut(&key) {
            el.forget_cycle = cycle;
            self.wma.forget_pq.entry(cycle).or_default().insert(key);
        }
    }

    fn wma_remove_from_p_queue(&mut self, key: WmeKey, cycle: u64) {
        if let Some(set) = self.wma.forget_pq.get_mut(&cycle) {
            if set.remove(&key) && set.is_empty() {
                self.wma.touched_sets.insert(cycle);
            }
        }
    }

    fn wma_move_in_p_queue(&mut self, key: WmeKey, cycle: u64) {
        let Some(old) = self.wma.elements.get(&key).map(|el| el.forget_cycle) else { return };
        if old != cycle {
            self.wma_remove_from_p_queue(key, old);
            self.wma_add_to_p_queue(key, cycle);
        }
    }

    /// Upstream `wma_forgetting_estimate_cycle`: when the element will fall below threshold,
    /// estimated from its history (`forgetting on`) and then found by binary search.
    fn wma_forgetting_estimate_cycle(&self, key: WmeKey, fresh_reference: bool) -> u64 {
        let Some(caches) = &self.wma.caches else { return self.wma.d_cycle_count };
        let Some(el) = self.wma.elements.get(&key) else { return self.wma.d_cycle_count };
        let params = &self.wma.params;
        let now = self.wma.d_cycle_count;
        let mut result = now;
        if fresh_reference && params.forgetting == Forgetting::Approx {
            let h = &el.touches;
            let mut to_add = 0u64;
            let mut p = h.next_p;
            for _ in 0..h.history_ct {
                p = w::history_prev(p);
                let cycle_diff = result.wrapping_sub(h.access_history[p].d_cycle);
                let refs = h.access_history[p].num_references.min(w::REFERENCES_PER_DECISION - 1) as usize;
                let approx = caches.approx[refs];
                if approx > cycle_diff {
                    to_add += approx - cycle_diff;
                }
            }
            result += to_add;
        }
        if result == now {
            let thresh = caches.thresh_exp;
            let activation = |cycle: u64| caches.decay_activation(params, &el.touches, cycle, false);
            let mut to_add = 1u64;
            let mut act = activation(result + to_add);
            if act >= thresh {
                while act >= thresh {
                    to_add *= 2;
                    act = activation(result + to_add);
                }
                let upper_bound_start = to_add;
                let mut upper = upper_bound_start;
                let mut lower = if to_add < 4 { upper } else { to_add / 2 };
                while lower != upper {
                    let mid = (lower + upper) / 2;
                    act = activation(result + mid);
                    if act < thresh {
                        upper = mid;
                        if upper - lower <= 1 {
                            lower = mid;
                        }
                    } else {
                        lower = mid;
                        if upper - lower <= 1 {
                            lower = upper;
                        }
                    }
                }
                to_add = upper;
            }
            result += to_add;
        }
        result
    }

    /// Upstream `wma_forgetting_forget_wme`: removes the WME's o-supported preferences.
    fn wma_forget_wme(&mut self, key: WmeKey) -> bool {
        if !self.wma.wme_prefs.contains_key(&key) {
            return false;
        }
        let Some(wme) = self.wm.get(key).cloned() else { return false };
        if self.wma.params.fake_forgetting {
            return false;
        }
        let removed = self.remove_preferences(|p| p.id == wme.id && p.attr == wme.attr && p.value == wme.val && p.o_supported);
        !removed.is_empty()
    }

    /// Whether the WME's identifier is a long-term identifier.
    fn wma_is_lti(&self, key: WmeKey) -> bool {
        self.wm.get(key).is_some_and(|w| self.symbols.lti(w.id) != 0)
    }

    /// Upstream `wma_forgetting_update_p_queue`.
    fn wma_forgetting_update_p_queue(&mut self) -> bool {
        let mut forgot = false;
        let Some((&first, _)) = self.wma.forget_pq.iter().next() else { return false };
        let current_cycle = self.wma.d_cycle_count;
        let lti_only = self.wma.params.forget_wme == ForgetWme::Lti;
        if first == current_cycle {
            let due: Vec<WmeKey> = self.wma.forget_pq.get(&first).map(|s| s.iter().copied().collect()).unwrap_or_default();
            for key in due {
                let Some(caches) = &self.wma.caches else { break };
                let Some(el) = self.wma.elements.get(&key) else { continue };
                let below = caches.decay_activation(&self.wma.params, &el.touches, current_cycle, false) < caches.thresh_exp;
                if !below {
                    let cycle = self.wma_forgetting_estimate_cycle(key, false);
                    self.wma_move_in_p_queue(key, cycle);
                    continue;
                }
                if let Some(el) = self.wma.elements.get_mut(&key) {
                    el.forget_cycle = w::FORGOTTEN_CYCLE;
                }
                if lti_only && !self.wma_is_lti(key) {
                    continue;
                }
                if lti_only {
                    // All or nothing: every o-supported WME of the identifier must be forgotten.
                    let Some(id) = self.wm.get(key).map(|w| w.id) else { continue };
                    let siblings = self.wm.wmes_by_id(id);
                    let all = siblings.iter().all(|k| {
                        !self.wma.wme_prefs.get(k).is_some_and(|p| p.o_supported)
                            || self.wma.elements.get(k).is_some_and(|el| el.forget_cycle == w::FORGOTTEN_CYCLE)
                    });
                    if all {
                        for sibling in siblings {
                            forgot |= self.wma_forget_wme(sibling);
                        }
                    }
                } else {
                    forgot |= self.wma_forget_wme(key);
                }
            }
            self.wma.touched_sets.insert(first);
            if let Some(set) = self.wma.forget_pq.get_mut(&first) {
                set.clear();
            }
        }
        for cycle in core::mem::take(&mut self.wma.touched_sets) {
            if self.wma.forget_pq.get(&cycle).is_some_and(BTreeSet::is_empty) {
                self.wma.forget_pq.remove(&cycle);
            }
        }
        forgot
    }

    /// Upstream `wma_forgetting_naive_sweep`.
    fn wma_forgetting_naive_sweep(&mut self) -> bool {
        let Some(caches) = &self.wma.caches else { return false };
        let current_cycle = self.wma.d_cycle_count;
        let lti_only = self.wma.params.forget_wme == ForgetWme::Lti;
        let due: Vec<WmeKey> = self
            .wm
            .iter()
            .filter_map(|wme| self.wma.elements.get(&wme.key).map(|el| (wme.key, el)))
            .filter(|(key, el)| {
                (!lti_only || self.wma_is_lti(*key))
                    && el.touches.total_references > 0
                    && el.touches.access_history[w::history_prev(el.touches.next_p)].d_cycle < current_cycle
                    && caches.decay_activation(&self.wma.params, &el.touches, current_cycle, false) < caches.thresh_exp
            })
            .map(|(key, _)| key)
            .collect();
        let mut forgot = false;
        for key in due {
            forgot |= self.wma_forget_wme(key);
        }
        forgot
    }

    /// Upstream `wma_go(wma_histories)`.
    pub(super) fn wma_go_histories(&mut self) {
        self.wma_collect();
        self.wma_update_decay_histories();
    }

    /// Upstream `wma_go(wma_forgetting)`.
    pub(super) fn wma_go_forgetting(&mut self) {
        let forgot = match self.wma.params.forgetting {
            Forgetting::Off => return,
            Forgetting::Naive => self.wma_forgetting_naive_sweep(),
            Forgetting::Bsearch | Forgetting::Approx => self.wma_forgetting_update_p_queue(),
        };
        if !forgot {
            return;
        }
        if self.wma.trace {
            self.output.push_str("\n\nWMA: BEGIN FORGOTTEN WME LIST\n\n");
        }
        let before = self.wma.removals;
        self.do_working_memory_phase();
        self.wma.forgotten_wmes += (self.wma.removals - before) as i64;
        if self.wma.trace {
            self.output.push_str("\nWMA: END FORGOTTEN WME LIST\n\n");
        }
    }

    /// Upstream's `add` trace line. Like upstream, an identifier value prints as the text before
    /// it.
    fn wma_trace_add(&mut self, wme: &Wme) {
        let text = |s: SymbolId| super::trace::symbol_to_string(&self.symbols, s, false);
        let id = text(wme.id);
        let attr = if self.symbols.is_identifier(wme.attr) { id.clone() } else { text(wme.attr) };
        let value = if self.symbols.is_identifier(wme.val) { attr.clone() } else { text(wme.val) };
        let line = alloc::format!("WMA @{}: add {} {} {} {}\n", self.wma.d_cycle_count, wme.timetag, id, attr, value);
        self.output.push_str(&line);
    }
}
