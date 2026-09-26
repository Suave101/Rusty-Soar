use crate::symbol::SymbolId;
use crate::wm::WmeKey;

/// Symbolic preference assertions dictating candidate selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreferenceType {
    /// Operator is a valid candidate (+)
    Acceptable,
    /// Disqualifies candidate (-)
    Reject,
    /// Relative ranking: O1 > O2
    Better,
    /// Relative ranking: O1 < O2
    Worse,
    /// Equal weight comparison (=)
    Indifferent,
    /// Mandatory candidate selection (!)
    Require,
    /// Absolute veto (~)
    Prohibit,
}

/// Preference assertion generated during operator proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preference {
    /// Preference type
    pub pref_type: PreferenceType,
    /// Target state ID (e.g., S1)
    pub state_id: SymbolId,
    /// Target attribute (e.g., ^operator)
    pub attribute: SymbolId,
    /// Proposed operator identifier
    pub candidate: SymbolId,
    /// Referent for binary comparisons (e.g., Candidate A > Referent B)
    pub referent: Option<SymbolId>,
    /// Back-reference to the generating WME instantiation key
    pub source_wme: Option<WmeKey>,
}