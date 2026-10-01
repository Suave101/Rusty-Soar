//! Architectural preference evaluation and decision procedure.

use alloc::vec::Vec;
use crate::rl::ReinforcementLearning;
use crate::symbol::SymbolId;

/// Categorical semantics for operator preferences asserted into Working Memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferenceType {
    /// Marks an operator candidate as valid for selection.
    Acceptable,
    /// Prohibits an operator candidate from selection.
    Reject,
    /// Asserts that operator A is preferred over operator B.
    Better(SymbolId),
    /// Asserts that operator A is less preferred than operator B.
    Worse(SymbolId),
    /// Requires this operator to be selected.
    Require,
    /// Prohibits this operator from selection.
    Prohibit,
    /// Marks this operator as better than candidates without a best preference.
    Best,
    /// Marks this operator as a fallback choice.
    Worst,
    /// Declares this operator indifferent to the remaining candidates.
    Indifferent,
    /// Declares this operator numerically indifferent; the deterministic resolver uses its RL value.
    NumericIndifferent,
}

/// Architectural preference entry for an operator candidate within a substate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preference {
    /// State symbol identifier.
    pub state: SymbolId,
    /// Candidate operator symbol identifier.
    pub operator: SymbolId,
    /// Preference type classification.
    pub preference_type: PreferenceType,
}

/// Result of evaluating preferences during the Decision Phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionResult {
    /// Unambiguous operator candidate selected for execution.
    Selected(SymbolId),
    /// Impasse triggered by multiple acceptable candidates with identical high Q-values.
    TieImpasse(Vec<SymbolId>),
    /// Impasse triggered by conflicting preference assertions.
    ConflictImpasse(Vec<SymbolId>),
    /// Impasse triggered when no acceptable operators are proposed.
    NoChangeImpasse,
}

/// Evaluates state preferences without RL integration.
pub fn resolve_preferences(state_id: SymbolId, preferences: &[Preference]) -> DecisionResult {
    let dummy_rl = ReinforcementLearning::default();
    resolve_preferences_with_rl(state_id, preferences, &dummy_rl)
}

/// Evaluates state preferences using RL Q-values $Q(s, a)$ to break tie impasses dynamically.
pub fn resolve_preferences_with_rl(
    state_id: SymbolId,
    preferences: &[Preference],
    rl: &ReinforcementLearning,
) -> DecisionResult {
    let state_prefs: Vec<&Preference> = preferences
        .iter()
        .filter(|p| p.state == state_id)
        .collect();

    // 1. RequireTest: duplicate assertions do not create duplicate candidates.
    let rejected = sorted_unique(
        state_prefs
        .iter()
        .filter(|p| {
            matches!(
                p.preference_type,
                PreferenceType::Reject | PreferenceType::Prohibit
            )
        })
        .map(|p| p.operator)
        .collect(),
    );
    let required = sorted_unique(
        state_prefs
        .iter()
        .filter(|p| p.preference_type == PreferenceType::Require)
        .map(|p| p.operator)
        .collect(),
    );

    if required.len() > 1 || required.iter().any(|operator| rejected.contains(operator)) {
        return DecisionResult::ConflictImpasse(required);
    }

    if let Some(&operator) = required.first() {
        return DecisionResult::Selected(operator);
    }

    // 2-4. AcceptableCollect, ProhibitFilter, and RejectFilter.
    let mut candidates = sorted_unique(
        state_prefs
        .iter()
        .filter(|p| p.preference_type == PreferenceType::Acceptable)
        .map(|p| p.operator)
        .filter(|op| !rejected.contains(op))
        .collect(),
    );

    if candidates.is_empty() {
        return DecisionResult::NoChangeImpasse;
    }

    if candidates.len() == 1 {
        return DecisionResult::Selected(candidates[0]);
    }

    // 5. BetterWorseFilter.
    let explicitly_dominated = sorted_unique(
        state_prefs
        .iter()
        .filter_map(|pref| match pref.preference_type {
            PreferenceType::Better(other) if candidates.contains(&other) => Some(other),
            PreferenceType::Worse(other) if candidates.contains(&other) => Some(pref.operator),
            _ => None,
        })
        .collect(),
    );

    candidates.retain(|op| !explicitly_dominated.contains(op));

    if candidates.is_empty() {
        return DecisionResult::ConflictImpasse(
            sorted_unique(state_prefs
                .iter()
                .filter(|p| p.preference_type == PreferenceType::Acceptable)
                .map(|p| p.operator)
                .collect()),
        );
    }

    if candidates.len() == 1 {
        return DecisionResult::Selected(candidates[0]);
    }

    // 6. BestFilter.
    let best: Vec<SymbolId> = candidates
        .iter()
        .copied()
        .filter(|operator| {
            state_prefs.iter().any(|preference| {
                preference.operator == *operator
                    && preference.preference_type == PreferenceType::Best
            })
        })
        .collect();
    if !best.is_empty() {
        candidates = best;
    }

    if candidates.len() == 1 {
        return DecisionResult::Selected(candidates[0]);
    }

    // 7. WorstFilter.
    let non_worst: Vec<SymbolId> = candidates
        .iter()
        .copied()
        .filter(|operator| {
            !state_prefs.iter().any(|preference| {
                preference.operator == *operator
                    && preference.preference_type == PreferenceType::Worst
            })
        })
        .collect();
    if !non_worst.is_empty() {
        candidates = non_worst;
    }

    // 8. IndifferentFilter. Numeric-indifferent candidates use deterministic
    // Q-value ordering; ordinary ties remain explicit impasses.
    let numeric_candidates: Vec<SymbolId> = candidates
        .iter()
        .copied()
        .filter(|operator| {
            state_prefs.iter().any(|preference| {
                preference.operator == *operator
                    && preference.preference_type == PreferenceType::NumericIndifferent
            })
        })
        .collect();

    if numeric_candidates.is_empty() {
        DecisionResult::TieImpasse(candidates)
    } else {
        let mut max_q = f32::NEG_INFINITY;
        let mut best_candidates = Vec::new();
        for operator in numeric_candidates {
            let q = rl.get_q_value(state_id, operator);
            if q > max_q + 1e-6 {
                max_q = q;
                best_candidates.clear();
                best_candidates.push(operator);
            } else if (q - max_q).abs() <= 1e-6 {
                best_candidates.push(operator);
            }
        }

        if best_candidates.len() == 1 {
            DecisionResult::Selected(best_candidates[0])
        } else {
            DecisionResult::TieImpasse(best_candidates)
        }
    }
}

fn sorted_unique(mut values: Vec<SymbolId>) -> Vec<SymbolId> {
    values.sort_unstable();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_rl_tie_breaking() {
        let s1 = SymbolId(1);
        let o1 = SymbolId(10);
        let o2 = SymbolId(20);

        let prefs = vec![
            Preference {
                state: s1,
                operator: o1,
                preference_type: PreferenceType::Acceptable,
            },
            Preference {
                state: s1,
                operator: o1,
                preference_type: PreferenceType::NumericIndifferent,
            },
            Preference {
                state: s1,
                operator: o2,
                preference_type: PreferenceType::Acceptable,
            },
            Preference {
                state: s1,
                operator: o2,
                preference_type: PreferenceType::NumericIndifferent,
            },
        ];

        let mut rl = ReinforcementLearning::default();

        // With zero Q-values, both remain tied
        assert_eq!(
            resolve_preferences_with_rl(s1, &prefs, &rl),
            DecisionResult::TieImpasse(vec![o1, o2])
        );

        // Update Q-value for O1
        rl.set_q_value(s1, o1, 5.0);

        // O1 should now be selected automatically
        assert_eq!(
            resolve_preferences_with_rl(s1, &prefs, &rl),
            DecisionResult::Selected(o1)
        );
    }
}