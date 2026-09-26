//! Preference resolution semantics for candidate operator selection.

use alloc::vec::Vec;
use crate::symbol::SymbolId;

/// Classification of operator preferences in Soar semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferenceType {
    /// Asserts that an operator is candidate-acceptable for the state.
    Acceptable,
    /// Absolute requirement for operator selection.
    Require,
    /// Explicit rejection of an operator candidate.
    Reject,
    /// Absolute prohibition of an operator candidate.
    Prohibit,
    /// Binary preference declaring candidate A better than candidate B.
    Better(SymbolId),
    /// Binary preference declaring candidate A worse than candidate B.
    Worse(SymbolId),
    /// Unary preference declaring an operator candidate best among choices.
    Best,
    /// Unary preference declaring an operator candidate worst among choices.
    Worst,
    /// Unary preference declaring indifferent selection among candidates.
    Indifferent,
}

/// Assertion representing an operator preference emitted by a production rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preference {
    /// Target state symbol ID.
    pub state: SymbolId,
    /// Target candidate operator symbol ID.
    pub operator: SymbolId,
    /// Specific preference semantic type.
    pub preference_type: PreferenceType,
}

/// Outcome resulting from candidate preference evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionResult {
    /// Successfully selected a single active operator handle.
    Selected(SymbolId),
    /// Encountered a tie impasse among equally valid operator candidates.
    TieImpasse(Vec<SymbolId>),
    /// Encountered conflicting preference assertions.
    ConflictImpasse(Vec<SymbolId>),
    /// No acceptable candidate operators were proposed.
    NoChangeImpasse,
}

/// Evaluates active preferences for a state symbol and resolves candidate selection or impasse.
pub fn resolve_preferences(state: SymbolId, preferences: &[Preference]) -> DecisionResult {
    let state_prefs: Vec<&Preference> = preferences
        .iter()
        .filter(|p| p.state == state)
        .collect();

    // 1. Gather all acceptable candidate operators
    let mut candidates: Vec<SymbolId> = state_prefs
        .iter()
        .filter_map(|p| match p.preference_type {
            PreferenceType::Acceptable | PreferenceType::Require => Some(p.operator),
            _ => None,
        })
        .collect();

    // Deduplicate candidate list
    candidates.dedup();

    if candidates.is_empty() {
        return DecisionResult::NoChangeImpasse;
    }

    // 2. Filter out rejected or prohibited operators
    candidates.retain(|&cand| {
        !state_prefs.iter().any(|p| {
            p.operator == cand
                && matches!(
                    p.preference_type,
                    PreferenceType::Reject | PreferenceType::Prohibit
                )
        })
    });

    if candidates.is_empty() {
        return DecisionResult::NoChangeImpasse;
    }

    if candidates.len() == 1 {
        return DecisionResult::Selected(candidates[0]);
    }

    // 3. Process binary preference (Better / Worse)
    let mut dominant_candidates = candidates.clone();

    for &cand_a in &candidates {
        for &cand_b in &candidates {
            if cand_a == cand_b {
                continue;
            }

            // If cand_b is strictly better than cand_a, remove cand_a
            let b_better_than_a = state_prefs.iter().any(|p| {
                p.operator == cand_b
                    && p.preference_type == PreferenceType::Better(cand_a)
            });

            let a_worse_than_b = state_prefs.iter().any(|p| {
                p.operator == cand_a
                    && p.preference_type == PreferenceType::Worse(cand_b)
            });

            if b_better_than_a || a_worse_than_b {
                dominant_candidates.retain(|&c| c != cand_a);
                break;
            }
        }
    }

    if dominant_candidates.len() == 1 {
        return DecisionResult::Selected(dominant_candidates[0]);
    }

    // 4. Return TieImpasse if multiple candidates remain without resolution
    DecisionResult::TieImpasse(dominant_candidates)
}