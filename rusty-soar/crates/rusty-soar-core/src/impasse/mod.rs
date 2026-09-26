//! Architectural impasse types and substate tracking for Soar subgoaling.

use crate::symbol::SymbolId;

/// Specific category of architectural impasse encountered during the decision phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpasseType {
    /// Multiple candidate operators exist with no distinguishing preferences.
    Tie,
    /// Conflicting preference assertions prevent deterministic selection.
    Conflict,
    /// No acceptable candidate operators were proposed for the state.
    NoChange,
}

/// Record representing an active architectural substate created to resolve an impasse.
#[derive(Debug, Clone)]
pub struct SubstateRecord {
    /// Symbol ID of the newly generated substate (e.g., `S2`).
    pub substate_id: SymbolId,
    /// Parent state symbol ID (e.g., `S1`).
    pub superstate_id: SymbolId,
    /// Reason for substate creation.
    pub impasse_type: ImpasseType,
}