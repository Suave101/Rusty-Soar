//! Preference semantics and exploration, ported from upstream `decide.cpp`
//! (`run_preference_semantics`) and `exploration.cpp`.

use crate::impasse::ImpasseType;
use crate::soar_rand::MtRand;
use crate::rl::LearningPolicy;
use crate::symbol::{SymbolId, SymbolTable};
use alloc::vec::Vec;

/// Preference types (upstream `PreferenceType`). Binary preferences carry their referent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceType {
    /// `+`
    Acceptable,
    /// `-`
    Reject,
    /// `> referent`
    Better(SymbolId),
    /// `< referent`
    Worse(SymbolId),
    /// `!`
    Require,
    /// `~`
    Prohibit,
    /// `>` with no referent
    Best,
    /// `<` with no referent
    Worst,
    /// `=` with no referent (unary indifferent)
    Indifferent,
    /// `= referent` where the referent is another candidate
    BinaryIndifferent(SymbolId),
    /// `= number`; the referent is the numeric value
    NumericIndifferent(SymbolId),
}

/// A preference for a value in a slot. For operator slots, `state` is the goal and
/// `operator` the candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preference {
    /// Slot identifier.
    pub state: SymbolId,
    /// Candidate value.
    pub operator: SymbolId,
    /// Preference type.
    pub preference_type: PreferenceType,
}

/// Outcome of deciding a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionResult {
    /// One candidate won.
    Selected(SymbolId),
    /// Candidates not all mutually indifferent.
    TieImpasse(Vec<SymbolId>),
    /// Better/worse preferences conflict; the conflicted candidates.
    ConflictImpasse(Vec<SymbolId>),
    /// Several requires, or a require with a prohibit; the required candidates.
    ConstraintFailureImpasse(Vec<SymbolId>),
    /// Nothing to select (or, after a selection, nothing changed).
    NoChangeImpasse,
    /// The agent has halted (the `halt` RHS function, or a full memory pool), so no decision
    /// was made.
    Halted,
}

/// Result of [`run_preference_semantics`].
#[derive(Debug, Clone, PartialEq)]
pub struct Semantics {
    /// `None` when there is no impasse (0 or 1 candidates, or all indifferent).
    pub impasse: Option<ImpasseType>,
    /// Remaining candidates, in upstream's candidate-list order.
    pub candidates: Vec<SymbolId>,
    /// With learning on, the reinforcement-learning update upstream makes for this decision.
    pub rl: Option<RlChoice>,
    /// Indices of the preferences that decided the winner (upstream's `OSK_prefs`); chunking
    /// backtraces through them. Empty for a consistency check or an impasse.
    pub osk: Vec<usize>,
}

/// What upstream's reinforcement learning takes from a decision with a winner (outside the
/// consistency check): the update `rl_perform_update` makes, and the winner's value for
/// `rl_store_data`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RlChoice {
    /// Value the update moves toward: the winner's (sarsa) or the best candidate's (q-learning).
    pub update_value: f64,
    /// Whether an RL rule contributed to `update_value`.
    pub update_rl: bool,
    /// The winner's value (upstream `numeric_value` of the selected candidate).
    pub selected_value: f64,
    /// Q-learning chose a candidate that is not the best: clear eligibility traces (Watkins).
    pub watkins_clear: bool,
    /// The winner's importance-sampling ratio (upstream `rl_rho`): under `off-policy-gq-lambda`,
    /// the greedy policy's chance of choosing it over the exploration policy's; otherwise 1.
    pub rho: f64,
}

/// Exploration policy (upstream `USER_SELECT_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorationPolicy {
    /// `first`
    First,
    /// `last`
    Last,
    /// `random-uniform`
    RandomUniform,
    /// `softmax` (upstream default)
    Softmax,
    /// `epsilon-greedy`
    EpsilonGreedy,
    /// `boltzmann`
    Boltzmann,
}

/// How several numeric-indifferent values for one candidate combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericIndifferentMode {
    /// Upstream default.
    Sum,
    /// Average.
    Avg,
}

/// Exploration settings and random state used to choose among indifferent candidates.
#[derive(Debug, Clone)]
pub struct Exploration {
    /// Selection policy; upstream defaults to softmax.
    pub policy: ExplorationPolicy,
    /// Epsilon for epsilon-greedy; upstream default 0.1.
    pub epsilon: f64,
    /// Temperature for Boltzmann; upstream default 25.
    pub temperature: f64,
    /// Numeric-indifferent combination; upstream default sum.
    pub numeric_indifferent_mode: NumericIndifferentMode,
    /// Random source. Upstream seeds from the clock; this seeds with 0 so runs are
    /// reproducible. Re-seed with [`MtRand::seed`] (upstream `srand`).
    pub rng: MtRand,
}

impl Default for Exploration {
    fn default() -> Self {
        Self {
            policy: ExplorationPolicy::Softmax,
            epsilon: 0.1,
            temperature: 25.0,
            numeric_indifferent_mode: NumericIndifferentMode::Sum,
            rng: MtRand::new(0),
        }
    }
}

fn number(symbols: &SymbolTable, symbol: SymbolId) -> f64 {
    symbols.number(symbol).unwrap_or(0.0)
}

impl Exploration {
    /// Upstream `exploration_compute_value_of_candidate` (default value 0): the candidate's
    /// value, summing numeric then binary indifferents as upstream does, and whether an RL rule
    /// made one of its numeric indifferents. `from_rl_rule[i]` marks `prefs[i]`; it may be empty.
    fn value_of(&self, candidate: SymbolId, prefs: &[Preference], from_rl_rule: &[bool], symbols: &SymbolTable) -> (f64, bool) {
        let mut total = 0usize;
        let mut value = 0.0;
        let mut rl_contribution = false;
        for (i, p) in prefs.iter().enumerate().filter(|(_, p)| p.operator == candidate) {
            if let PreferenceType::NumericIndifferent(r) = p.preference_type {
                total += 1;
                value += number(symbols, r);
                rl_contribution |= from_rl_rule.get(i).copied().unwrap_or(false);
            }
        }
        for p in prefs.iter().filter(|p| p.operator == candidate) {
            if let PreferenceType::BinaryIndifferent(r) = p.preference_type {
                total += 1;
                value += number(symbols, r);
            }
        }
        if total == 0 {
            total = 1;
        }
        let value = match self.numeric_indifferent_mode {
            NumericIndifferentMode::Sum => value,
            NumericIndifferentMode::Avg => value / total as f64,
        };
        (value, rl_contribution)
    }

    fn randomly_select(&mut self, candidates: &[SymbolId]) -> SymbolId {
        let index = self.rng.rand_int_max(candidates.len() as u32 - 1) as usize;
        candidates[index]
    }

    /// Upstream `exploration_choose_according_to_policy`, with learning off.
    pub fn choose(&mut self, candidates: &[SymbolId], prefs: &[Preference], symbols: &SymbolTable) -> SymbolId {
        let values: Vec<f64> = candidates.iter().map(|c| self.value_of(*c, prefs, &[], symbols).0).collect();
        self.select(candidates, &values)
    }

    /// Upstream `exploration_choose_according_to_policy`: selects a candidate and, with a
    /// learning policy, works out the RL update upstream makes after selecting.
    fn choose_and_learn(
        &mut self,
        candidates: &[SymbolId],
        prefs: &[Preference],
        from_rl_rule: &[bool],
        learning: Option<LearningPolicy>,
        symbols: &SymbolTable,
    ) -> (SymbolId, Option<RlChoice>) {
        let scored: Vec<(f64, bool)> = candidates.iter().map(|c| self.value_of(*c, prefs, from_rl_rule, symbols)).collect();
        let values: Vec<f64> = scored.iter().map(|s| s.0).collect();
        let chosen = self.select(candidates, &values);
        let Some(policy) = learning else { return (chosen, None) };
        let index = candidates.iter().position(|c| *c == chosen).unwrap_or(0);
        let (selected_value, selected_rl) = scored[index];
        let rho = if policy == LearningPolicy::OffPolicyGq { self.rho(&values, index) } else { 1.0 };
        let choice = if policy.off_policy() {
            // The first candidate with the highest value.
            let (mut top_value, mut top_rl) = scored[0];
            for &(value, rl) in &scored {
                if value > top_value {
                    top_value = value;
                    top_rl = rl;
                }
            }
            let watkins_clear = policy == LearningPolicy::QLearning && selected_value != top_value;
            RlChoice { update_value: top_value, update_rl: top_rl, selected_value, watkins_clear, rho }
        } else {
            RlChoice { update_value: selected_value, update_rl: selected_rl, selected_value, watkins_clear: false, rho }
        };
        (chosen, Some(choice))
    }

    /// Upstream's `rl_rho` for the candidate at `index`, as `exploration_choose_according_to_policy`
    /// and the select functions leave it under an off-policy learning policy: the greedy policy's
    /// probability of the choice, divided by the exploration policy's.
    fn rho(&self, values: &[f64], index: usize) -> f64 {
        if matches!(self.policy, ExplorationPolicy::First | ExplorationPolicy::Last) {
            return 1.0;
        }
        let top = values.iter().copied().fold(values[0], f64::max);
        let num_top = values.iter().filter(|v| **v == top).count() as f64;
        let greedy = if values[index] == top { 1.0 / num_top } else { 0.0 };
        let n = values.len() as f64;
        match self.policy {
            ExplorationPolicy::First | ExplorationPolicy::Last => 1.0,
            ExplorationPolicy::RandomUniform => greedy / (1.0 / n),
            ExplorationPolicy::Softmax => {
                let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
                if total == 0.0 {
                    greedy / (1.0 / n)
                } else if values[index] != 0.0 {
                    greedy / (values[index] / total)
                } else if greedy > 0.0 {
                    f64::MAX
                } else {
                    greedy
                }
            }
            ExplorationPolicy::EpsilonGreedy => greedy / ((1.0 - self.epsilon) * greedy + self.epsilon / n),
            ExplorationPolicy::Boltzmann => {
                let max = values.iter().copied().fold(values[0], f64::max);
                let exps: Vec<f64> = values.iter().map(|v| libm::exp((v - max) / self.temperature)).collect();
                let total: f64 = exps.iter().sum();
                greedy / (exps[index] / total)
            }
        }
    }

    fn select(&mut self, candidates: &[SymbolId], values: &[f64]) -> SymbolId {
        match self.policy {
            ExplorationPolicy::First => candidates[0],
            ExplorationPolicy::Last => candidates[candidates.len() - 1],
            ExplorationPolicy::RandomUniform => self.randomly_select(candidates),
            ExplorationPolicy::Softmax => {
                // Upstream `exploration_probabilistically_select`.
                let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
                if total == 0.0 {
                    return self.randomly_select(candidates);
                }
                let selected = total * self.rng.rand();
                let mut sum = 0.0;
                let mut last_positive = candidates[0];
                for (c, v) in candidates.iter().zip(values) {
                    if *v > 0.0 {
                        sum += v;
                        last_positive = *c;
                        if selected <= sum {
                            return *c;
                        }
                    }
                }
                last_positive
            }
            ExplorationPolicy::EpsilonGreedy => {
                if self.rng.rand() < self.epsilon {
                    return self.randomly_select(candidates);
                }
                // Upstream `exploration_get_highest_q_value_pref`.
                let mut top = values[0];
                let mut top_index = 0;
                let mut count = 0u32;
                for (i, v) in values.iter().enumerate() {
                    if *v > top {
                        top = *v;
                        top_index = i;
                        count = 1;
                    } else if *v == top {
                        count += 1;
                    }
                }
                if count == 1 {
                    return candidates[top_index];
                }
                let mut chosen = self.rng.rand_int_max(count - 1);
                for (c, v) in candidates.iter().zip(values) {
                    if *v == top {
                        if chosen == 0 {
                            return *c;
                        }
                        chosen -= 1;
                    }
                }
                candidates[top_index]
            }
            ExplorationPolicy::Boltzmann => {
                let max = values.iter().copied().fold(values[0], f64::max);
                let exps: Vec<f64> = values.iter().map(|v| libm::exp((v - max) / self.temperature)).collect();
                let total: f64 = exps.iter().sum();
                let r = self.rng.rand_max(total);
                let mut sum = 0.0;
                for (c, e) in candidates.iter().zip(&exps) {
                    sum += e;
                    if sum >= r {
                        return *c;
                    }
                }
                candidates[candidates.len() - 1]
            }
        }
    }
}


/// Values of preferences of a given kind, in slot-list order.
fn values_of<'a>(prefs: &'a [Preference], kind: impl Fn(&PreferenceType) -> bool + 'a) -> impl Iterator<Item = SymbolId> + 'a {
    prefs.iter().filter(move |p| kind(&p.preference_type)).map(|p| p.operator)
}

/// Upstream builds candidate lists by prepending each new (unmarked) value while walking a
/// preference list, so the result is the distinct values in reverse order.
fn prepend_distinct(values: impl Iterator<Item = SymbolId>, eligible: impl Fn(SymbolId) -> bool) -> Vec<SymbolId> {
    let mut seen: Vec<SymbolId> = Vec::new();
    for v in values {
        if eligible(v) && !seen.contains(&v) {
            seen.push(v);
        }
    }
    seen.reverse();
    seen
}

/// Upstream `add_to_OSK`: `unique` also skips a preference that says the same as one in the set.
fn add_to_osk(prefs: &[Preference], osk: &mut Vec<usize>, index: usize, unique: bool) {
    use PreferenceType as P;
    let p = &prefs[index];
    let same = |other: &usize| {
        let q = &prefs[*other];
        if *other == index {
            return true;
        }
        if !unique {
            return false;
        }
        match (p.preference_type, q.preference_type) {
            (P::Better(a), P::Better(b)) | (P::Worse(a), P::Worse(b)) => p.operator == q.operator && a == b,
            (P::Better(a), P::Worse(b)) | (P::Worse(a), P::Better(b)) => p.operator == b && a == q.operator,
            (P::BinaryIndifferent(a), P::BinaryIndifferent(b)) => (p.operator == q.operator && a == b) || (p.operator == b && a == q.operator),
            (a, b) => p.operator == q.operator && core::mem::discriminant(&a) == core::mem::discriminant(&b),
        }
    };
    if !osk.iter().any(same) {
        osk.push(index);
    }
}

/// Upstream `run_preference_semantics` for a context (operator) slot.
///
/// `prefs` are the slot's preferences in upstream's per-type list order (ascending match-goal
/// level, newest first within a level). With `consistency` set, indifferent candidates are all
/// returned rather than one being chosen, as in upstream's consistency check.
///
/// With `learning` set (and outside the consistency check), a decision with a winner also
/// reports the reinforcement-learning update upstream makes (`rl_update_for_one_candidate`, or
/// the update after exploration). `from_rl_rule[i]` marks preferences made by RL rules.
pub fn run_preference_semantics(
    prefs: &[Preference],
    from_rl_rule: &[bool],
    consistency: bool,
    learning: Option<LearningPolicy>,
    exploration: &mut Exploration,
    symbols: &SymbolTable,
) -> Semantics {
    use PreferenceType as P;
    let learning = if consistency { None } else { learning };
    // Upstream `rl_update_for_one_candidate` at each exit with a single winner.
    let done = |candidates: Vec<SymbolId>, osk: Vec<usize>| {
        let rl = match (learning, candidates.as_slice()) {
            (Some(_), [winner]) => {
                let (value, rl) = exploration.value_of(*winner, prefs, from_rl_rule, symbols);
                Some(RlChoice { update_value: value, update_rl: rl, selected_value: value, watkins_clear: false, rho: 1.0 })
            }
            _ => None,
        };
        Semantics { impasse: None, candidates, rl, osk }
    };
    let track = !consistency;
    let of_type = |kind: fn(&PreferenceType) -> bool| prefs.iter().enumerate().filter(move |(_, p)| kind(&p.preference_type));

    // === Requires ===
    if prefs.iter().any(|p| p.preference_type == P::Require) {
        let candidates = prepend_distinct(values_of(prefs, |t| *t == P::Require), |_| true);
        if candidates.len() > 1 {
            return Semantics { impasse: Some(ImpasseType::ConstraintFailure), candidates, rl: None, osk: Vec::new() };
        }
        if values_of(prefs, |t| *t == P::Prohibit).any(|v| v == candidates[0]) {
            return Semantics { impasse: Some(ImpasseType::ConstraintFailure), candidates, rl: None, osk: Vec::new() };
        }
        return done(candidates, Vec::new());
    }

    // === Acceptables, prohibits, rejects ===
    let excluded: Vec<SymbolId> = values_of(prefs, |t| matches!(t, P::Prohibit | P::Reject)).collect();
    let mut candidates = prepend_distinct(values_of(prefs, |t| *t == P::Acceptable), |v| !excluded.contains(&v));
    let mut osk = Vec::new();
    if track {
        for (i, _) in of_type(|t| *t == P::Prohibit).chain(of_type(|t| *t == P::Reject)) {
            add_to_osk(prefs, &mut osk, i, true);
        }
    }
    if candidates.is_empty() {
        osk.clear();
    }
    if candidates.len() <= 1 {
        return done(candidates, osk);
    }

    // === Better / worse ===
    let binary: Vec<(SymbolId, SymbolId, bool)> = prefs
        .iter()
        .filter_map(|p| match p.preference_type {
            P::Better(r) => Some((p.operator, r, true)),
            P::Worse(r) => Some((p.operator, r, false)),
            _ => None,
        })
        .collect();
    if !binary.is_empty() {
        #[derive(Clone, Copy, PartialEq)]
        enum Flag {
            Nothing,
            Candidate,
            Conflicted,
        }
        let mut flags: Vec<(SymbolId, Flag)> = Vec::new();
        let flag = |flags: &Vec<(SymbolId, Flag)>, s: SymbolId| {
            flags.iter().rev().find(|(v, _)| *v == s).map_or(Flag::Nothing, |(_, f)| *f)
        };
        for c in &candidates {
            flags.push((*c, Flag::Candidate));
        }
        // Upstream walks the better list then the worse list, marking as it goes, so the
        // result depends on list order exactly as here.
        for better in [true, false] {
            for &(j, k, is_better) in binary.iter().filter(|b| b.2 == better) {
                if j == k {
                    continue;
                }
                let (fj, fk) = (flag(&flags, j), flag(&flags, k));
                if fj != Flag::Nothing && fk != Flag::Nothing && (fj == Flag::Candidate || fk == Flag::Candidate) {
                    flags.push((if is_better { k } else { j }, Flag::Conflicted));
                }
            }
        }
        if !candidates.iter().any(|c| flag(&flags, *c) == Flag::Candidate) {
            candidates.retain(|c| flag(&flags, *c) == Flag::Conflicted);
            return Semantics { impasse: Some(ImpasseType::Conflict), candidates, rl: None, osk: Vec::new() };
        }
        candidates.retain(|c| flag(&flags, *c) != Flag::Conflicted);
        if track {
            for c in &candidates {
                for (i, p) in prefs.iter().enumerate() {
                    if matches!(p.preference_type, P::Better(_)) && p.operator == *c {
                        add_to_osk(prefs, &mut osk, i, true);
                    }
                }
                for (i, p) in prefs.iter().enumerate() {
                    if matches!(p.preference_type, P::Worse(r) if r == *c) {
                        add_to_osk(prefs, &mut osk, i, true);
                    }
                }
            }
        }
        if candidates.len() <= 1 {
            return done(candidates, osk);
        }
    }

    // === Bests ===
    let best: Vec<SymbolId> = values_of(prefs, |t| *t == P::Best).collect();
    if !best.is_empty() && candidates.iter().any(|c| best.contains(c)) {
        candidates.retain(|c| best.contains(c));
        if track {
            for c in &candidates {
                for (i, p) in of_type(|t| *t == P::Best) {
                    if p.operator == *c {
                        add_to_osk(prefs, &mut osk, i, true);
                    }
                }
            }
        }
    }
    if candidates.len() <= 1 {
        return done(candidates, osk);
    }

    // === Worsts ===
    let worst: Vec<SymbolId> = values_of(prefs, |t| *t == P::Worst).collect();
    if !worst.is_empty() && candidates.iter().any(|c| !worst.contains(c)) {
        if track {
            for c in candidates.iter().filter(|c| worst.contains(c)) {
                for (i, p) in of_type(|t| *t == P::Worst) {
                    if p.operator == *c {
                        add_to_osk(prefs, &mut osk, i, true);
                    }
                }
            }
        }
        candidates.retain(|c| !worst.contains(c));
    }
    if candidates.len() <= 1 {
        return done(candidates, osk);
    }

    // === Indifferents ===
    let unary: Vec<SymbolId> = values_of(prefs, |t| *t == P::Indifferent).collect();
    let numeric: Vec<SymbolId> = values_of(prefs, |t| matches!(t, P::NumericIndifferent(_))).collect();
    let binary_indifferent: Vec<(SymbolId, SymbolId)> = prefs
        .iter()
        .filter_map(|p| match p.preference_type {
            P::BinaryIndifferent(r) => Some((p.operator, r)),
            _ => None,
        })
        .collect();
    let all_indifferent = candidates.iter().all(|&c| {
        // Numeric marks win over unary marks, as upstream sets them second.
        if numeric.contains(&c) || unary.contains(&c) {
            return true;
        }
        candidates.iter().filter(|&&o| o != c).all(|&o| {
            binary_indifferent.iter().any(|&(v, r)| (v == c && r == o) || (v == o && r == c))
        })
    });
    if all_indifferent {
        if consistency {
            return done(candidates, osk);
        }
        let some_numeric = candidates.iter().any(|c| numeric.contains(c));
        let (chosen, rl) = exploration.choose_and_learn(&candidates, prefs, from_rl_rule, learning, symbols);
        if some_numeric {
            // Numeric preferences never count as duplicates.
            for (i, p) in of_type(|t| matches!(t, P::NumericIndifferent(_))) {
                if p.operator == chosen {
                    add_to_osk(prefs, &mut osk, i, false);
                }
            }
        }
        // Upstream leaves out unary indifferents: "they were causing problems".
        for (i, p) in prefs.iter().enumerate() {
            if let P::BinaryIndifferent(r) = p.preference_type {
                let both_numeric = numeric.contains(&r) && numeric.contains(&p.operator);
                if (p.operator == chosen || r == chosen) && !(some_numeric && both_numeric) {
                    add_to_osk(prefs, &mut osk, i, true);
                }
            }
        }
        return Semantics { impasse: None, candidates: alloc::vec![chosen], rl, osk };
    }
    Semantics { impasse: Some(ImpasseType::Tie), candidates, rl: None, osk: Vec::new() }
}

/// Decides one state's operator preferences in isolation, with default exploration settings.
/// `preferences` are in assertion order; they all count as the same goal level.
pub fn resolve_preferences(state_id: SymbolId, preferences: &[Preference]) -> DecisionResult {
    // Upstream keeps the newest preference first within a level.
    let mut prefs: Vec<Preference> = preferences.iter().filter(|p| p.state == state_id).cloned().collect();
    prefs.reverse();
    let symbols = SymbolTable::new();
    let semantics = run_preference_semantics(&prefs, &[], false, None, &mut Exploration::default(), &symbols);
    decision_result(semantics)
}

/// Converts preference semantics into a [`DecisionResult`].
pub fn decision_result(semantics: Semantics) -> DecisionResult {
    match (semantics.impasse, semantics.candidates.as_slice()) {
        (None, []) => DecisionResult::NoChangeImpasse,
        (None, [only, ..]) => DecisionResult::Selected(*only),
        (Some(ImpasseType::Tie), _) => DecisionResult::TieImpasse(semantics.candidates),
        (Some(ImpasseType::Conflict), _) => DecisionResult::ConflictImpasse(semantics.candidates),
        (Some(ImpasseType::ConstraintFailure), _) => DecisionResult::ConstraintFailureImpasse(semantics.candidates),
        (Some(ImpasseType::NoChange), _) => DecisionResult::NoChangeImpasse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn p(op: u32, t: PreferenceType) -> Preference {
        Preference { state: SymbolId(1), operator: SymbolId(op), preference_type: t }
    }

    fn run(prefs: &[Preference]) -> Semantics {
        run_preference_semantics(prefs, &[], false, None, &mut Exploration::default(), &SymbolTable::new())
    }

    use PreferenceType::*;

    #[test]
    fn two_acceptables_without_indifference_tie() {
        let s = run(&[p(10, Acceptable), p(20, Acceptable)]);
        assert_eq!(s.impasse, Some(ImpasseType::Tie));
        // Candidate lists are built by prepending, so they come out reversed.
        assert_eq!(s.candidates, vec![SymbolId(20), SymbolId(10)]);
    }

    #[test]
    fn require_with_prohibit_is_a_constraint_failure() {
        let s = run(&[p(10, Require), p(10, Prohibit), p(20, Acceptable)]);
        assert_eq!(s.impasse, Some(ImpasseType::ConstraintFailure));
        let s = run(&[p(10, Require), p(20, Require)]);
        assert_eq!(s.impasse, Some(ImpasseType::ConstraintFailure));
    }

    #[test]
    fn better_cycle_is_a_conflict_of_the_cycle_members() {
        let s = run(&[
            p(10, Acceptable),
            p(20, Acceptable),
            p(10, Better(SymbolId(20))),
            p(20, Better(SymbolId(10))),
        ]);
        assert_eq!(s.impasse, Some(ImpasseType::Conflict));
        assert_eq!(s.candidates.len(), 2);
    }

    #[test]
    fn unary_indifference_selects_one() {
        let s = run(&[p(10, Acceptable), p(20, Acceptable), p(10, Indifferent), p(20, Indifferent)]);
        assert_eq!(s.impasse, None);
        assert_eq!(s.candidates.len(), 1);
    }

    #[test]
    fn worst_only_removes_when_something_else_remains() {
        let s = run(&[p(10, Acceptable), p(10, Worst)]);
        assert_eq!(s.candidates, vec![SymbolId(10)]);
        let s = run(&[p(10, Acceptable), p(20, Acceptable), p(10, Worst)]);
        assert_eq!(s.candidates, vec![SymbolId(20)]);
    }

    #[test]
    fn softmax_follows_numeric_values() {
        let mut symbols = SymbolTable::new();
        let (zero, one) = (symbols.intern_float(0.0), symbols.intern_float(1.0));
        let prefs = [p(10, Acceptable), p(20, Acceptable), p(10, NumericIndifferent(zero)), p(20, NumericIndifferent(one))];
        let mut exploration = Exploration::default();
        for _ in 0..20 {
            let s = run_preference_semantics(&prefs, &[], false, None, &mut exploration, &symbols);
            assert_eq!(s.candidates, vec![SymbolId(20)], "only the positive value can be chosen");
        }
    }
}
