//! Architectural impasse types and substate tracking for Soar subgoaling.

use crate::symbol::SymbolId;

/// Kind of impasse (upstream `*_IMPASSE_TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpasseType {
    /// Several candidates that are not all mutually indifferent.
    Tie,
    /// Better/worse preferences conflict.
    Conflict,
    /// Several requires, or a require that is also prohibited.
    ConstraintFailure,
    /// No candidate (state no-change), or no progress after a selection (operator no-change).
    NoChange,
}

/// A substate on the goal stack.
#[derive(Debug, Clone)]
pub struct SubstateRecord {
    /// The substate (e.g. `S2`).
    pub substate_id: SymbolId,
    /// Its superstate (e.g. `S1`).
    pub superstate_id: SymbolId,
    /// Why it was created.
    pub impasse_type: ImpasseType,
}
