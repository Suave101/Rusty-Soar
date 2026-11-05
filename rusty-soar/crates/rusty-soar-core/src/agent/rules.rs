//! The agent's productions: each one's actions, new-identifier letters and declared support,
//! stored by production in fixed-capacity arrays.
//!
//! Actions without function calls are kept as flat `Copy` records ([`FlatAction`]), in storage
//! the agent's slot family chooses, following the Rete's layout rules (see the `rete` module
//! docs). An action that calls a function, the one kind with strings and nested
//! argument lists, is kept whole on the heap; only rules that call functions read it.

use super::Action;
use crate::pool::{BoundedVec, Layout};
use crate::preference::{Preference, PreferenceType};
use crate::production::compile::{CompiledAction, RhsTerm};
use crate::rete::{ProductionId, VarLocation};
use crate::soar_parser::{SoarPreference, SoarSupport};
use crate::symbol::SymbolId;
use crate::wm::WmeKey;
use alloc::vec::Vec;

/// A RHS value without function calls, for [`FlatAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlatTerm {
    /// A constant.
    Constant(SymbolId),
    /// A variable bound on the LHS.
    Bound(VarLocation),
    /// A new identifier, the index into the production's new-identifier letters.
    NewIdentifier(usize),
}

impl FlatTerm {
    /// The term as an [`RhsTerm`], or `None` for a function call.
    fn from_rhs(term: &RhsTerm) -> Option<Self> {
        match term {
            RhsTerm::Constant(s) => Some(Self::Constant(*s)),
            RhsTerm::Bound(location) => Some(Self::Bound(*location)),
            RhsTerm::NewIdentifier(i) => Some(Self::NewIdentifier(*i)),
            RhsTerm::Call { .. } => None,
        }
    }
}

/// What a [`FlatAction`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum ActionKind {
    Add,
    Remove,
    Prefer,
    Make,
    /// An action that calls a function, kept in [`Rules::complex`].
    Complex,
}

/// An action without function calls, the form of [`Action`] a Kani proof can build; the agent
/// stores every action this way.
///
/// It is a struct with a kind rather than an enum because Kani loses the tag of a
/// data-carrying enum (here [`FlatTerm`]) stored in another enum's variant, which leaves the
/// loops that depend on it unbounded. Fields a kind does not use hold placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatAction {
    pub(super) kind: ActionKind,
    /// `Add`: the triple, as constants. `Make`: identifier, attribute and value.
    pub(super) terms: [FlatTerm; 3],
    /// `Make`: the preference type.
    pub(super) preference: SoarPreference,
    /// `Make`: the referent of a binary preference, when `has_referent`.
    pub(super) referent: FlatTerm,
    pub(super) has_referent: bool,
    /// `Prefer`: the preference.
    pub(super) prefer: Preference,
    /// `Remove`: the WME.
    pub(super) wme: WmeKey,
    /// `Complex`: the action's index in [`Rules::complex`].
    pub(super) index: usize,
}

impl FlatAction {
    const fn of_kind(kind: ActionKind) -> Self {
        let placeholder = FlatTerm::NewIdentifier(0);
        Self {
            kind,
            terms: [placeholder; 3],
            preference: SoarPreference::Acceptable,
            referent: placeholder,
            has_referent: false,
            prefer: Preference { state: SymbolId(0), operator: SymbolId(0), preference_type: PreferenceType::Acceptable },
            wme: WmeKey(0),
            index: 0,
        }
    }

    /// As [`Action::Add`].
    pub const fn add(id: SymbolId, attr: SymbolId, val: SymbolId) -> Self {
        Self { terms: [FlatTerm::Constant(id), FlatTerm::Constant(attr), FlatTerm::Constant(val)], ..Self::of_kind(ActionKind::Add) }
    }

    /// As [`Action::Remove`].
    pub const fn remove(wme: WmeKey) -> Self {
        Self { wme, ..Self::of_kind(ActionKind::Remove) }
    }

    /// As [`Action::Prefer`].
    pub const fn prefer(preference: Preference) -> Self {
        Self { prefer: preference, ..Self::of_kind(ActionKind::Prefer) }
    }

    /// As [`CompiledAction::Make`] without a referent.
    pub const fn make(id: FlatTerm, attr: FlatTerm, value: FlatTerm, preference: SoarPreference) -> Self {
        Self { terms: [id, attr, value], preference, ..Self::of_kind(ActionKind::Make) }
    }

    /// As [`CompiledAction::Make`] for a binary preference.
    pub const fn make_binary(id: FlatTerm, attr: FlatTerm, value: FlatTerm, preference: SoarPreference, referent: FlatTerm) -> Self {
        Self { referent, has_referent: true, ..Self::make(id, attr, value, preference) }
    }

    /// The referent of a `Make` action, if it has one.
    pub(super) fn referent(&self) -> Option<FlatTerm> {
        self.has_referent.then_some(self.referent)
    }
}

/// An action the agent can store.
pub(super) trait ToStored {
    /// The stored form; an action that calls a function is moved to `complex`.
    fn to_stored(&self, complex: &mut Vec<CompiledAction>) -> FlatAction;
}

impl ToStored for FlatAction {
    fn to_stored(&self, _complex: &mut Vec<CompiledAction>) -> FlatAction {
        *self
    }
}

impl ToStored for Action {
    fn to_stored(&self, complex: &mut Vec<CompiledAction>) -> FlatAction {
        match self {
            Action::Add { id, attr, val } => FlatAction::add(*id, *attr, *val),
            Action::Remove(key) => FlatAction::remove(*key),
            Action::Prefer(p) => FlatAction::prefer(*p),
            Action::Rhs(rhs) => {
                if let CompiledAction::Make { id, attr, value, preference, referent } = rhs {
                    let terms = (FlatTerm::from_rhs(id), FlatTerm::from_rhs(attr), FlatTerm::from_rhs(value));
                    if let (Some(id), Some(attr), Some(value)) = terms {
                        match referent.as_ref().map(FlatTerm::from_rhs) {
                            None => return FlatAction::make(id, attr, value, *preference),
                            Some(Some(referent)) => return FlatAction::make_binary(id, attr, value, *preference, referent),
                            Some(None) => {}
                        }
                    }
                }
                complex.push(rhs.clone());
                FlatAction { index: complex.len() - 1, ..FlatAction::of_kind(ActionKind::Complex) }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    start: usize,
    len: usize,
}

impl Range {
    fn indices(self) -> core::ops::Range<usize> {
        self.start..self.start + self.len
    }
}

#[derive(Debug, Clone, Copy)]
struct Rule {
    name: &'static str,
    actions: Range,
    letters: Range,
    support: SoarSupport,
}

/// A store that ran out of room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Full {
    Rules,
    Actions,
    NewIdentifierLetters,
}

impl Full {
    /// The variant's name, as `Debug` prints it.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Rules => "Rules",
            Self::Actions => "Actions",
            Self::NewIdentifierLetters => "NewIdentifierLetters",
        }
    }
}

/// Every production's actions, letters and support, indexed by [`ProductionId`].
#[derive(Debug)]
pub(super) struct Rules<S: Layout> {
    rules: BoundedVec<Rule, S::Productions>,
    actions: BoundedVec<FlatAction, S::Actions>,
    letters: BoundedVec<char, S::Actions>,
    complex: Vec<CompiledAction>,
}

impl<S: Layout> Rules<S> {
    pub(super) fn with_capacity(rules: usize, actions: usize, letters: usize) -> Self {
        Self {
            rules: BoundedVec::with_capacity(rules),
            actions: BoundedVec::with_capacity(actions),
            letters: BoundedVec::with_capacity(letters),
            complex: Vec::new(),
        }
    }

    /// Records the production the Rete numbered `production`, which must be the next one.
    pub(super) fn add(
        &mut self,
        production: ProductionId,
        name: &'static str,
        actions: &[impl ToStored],
        letters: &[char],
        support: SoarSupport,
    ) -> Result<(), Full> {
        debug_assert_eq!(production.0, self.rules.len(), "productions are recorded in Rete order");
        let action_range = Range { start: self.actions.len(), len: actions.len() };
        for action in actions {
            let stored = action.to_stored(&mut self.complex);
            self.actions.push(stored).map_err(|_| Full::Actions)?;
        }
        let letter_range = Range { start: self.letters.len(), len: letters.len() };
        for letter in letters {
            self.letters.push(*letter).map_err(|_| Full::NewIdentifierLetters)?;
        }
        let rule = Rule { name, actions: action_range, letters: letter_range, support };
        self.rules.push(rule).map_err(|_| Full::Rules)
    }

    /// Whether a production of this name has been recorded.
    pub(super) fn contains_name(&self, name: &str) -> bool {
        self.rules.iter().any(|rule| rule.name == name)
    }

    /// Number of actions of a production (zero if it is unknown).
    pub(super) fn action_count(&self, production: ProductionId) -> usize {
        self.rule(production).map_or(0, |rule| rule.actions.len)
    }

    /// A production's action at `index`.
    pub(super) fn action(&self, production: ProductionId, index: usize) -> FlatAction {
        let rule = self.rule(production).expect("a recorded production");
        self.actions[rule.actions.indices().nth(index).expect("an action of the production")]
    }

    /// A production's new-identifier letters.
    pub(super) fn letters(&self, production: ProductionId) -> Vec<char> {
        self.rule(production).map_or_else(Vec::new, |rule| rule.letters.indices().map(|i| self.letters[i]).collect())
    }

    /// A production's name.
    pub(super) fn name(&self, production: ProductionId) -> &'static str {
        self.rule(production).map_or("", |rule| rule.name)
    }

    /// A production's declared support.
    pub(super) fn support(&self, production: ProductionId) -> SoarSupport {
        self.rule(production).map_or_else(SoarSupport::default, |rule| rule.support)
    }

    /// An action stored as [`ActionKind::Complex`].
    pub(super) fn complex(&self, index: usize) -> &CompiledAction {
        &self.complex[index]
    }

    fn rule(&self, production: ProductionId) -> Option<Rule> {
        (production.0 < self.rules.len()).then(|| self.rules[production.0])
    }
}
