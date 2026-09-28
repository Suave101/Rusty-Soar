//! Semantic Memory (SMem) long-term declarative store for `rusty-soar`.

use crate::symbol::SymbolId;
use alloc::vec::Vec;

/// Long-Term Identifier (LTI) key referencing a declarative concept in SMem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LtiId(pub usize);

/// Attribute-Value relationship stored on a Long-Term Identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFact {
    /// Attribute symbol.
    pub attr: SymbolId,
    /// Value symbol.
    pub val: SymbolId,
}

/// Long-Term Declarative Semantic Memory store.
#[derive(Debug, Default)]
pub struct SemanticMemory {
    /// Fact storage per LTI index.
    facts: Vec<Vec<SemanticFact>>,
}

impl SemanticMemory {
    /// Creates a new empty `SemanticMemory` instance.
    pub fn new() -> Self {
        Self { facts: Vec::new() }
    }

    /// Allocates a new Long-Term Identifier (LTI) handle in SMem.
    pub fn create_lti(&mut self) -> LtiId {
        let lti = LtiId(self.facts.len());
        self.facts.push(Vec::new());
        lti
    }

    /// Stores an attribute-value fact associated with an LTI in Semantic Memory.
    pub fn store_fact(&mut self, lti: LtiId, attr: SymbolId, val: SymbolId) {
        if lti.0 < self.facts.len() {
            let fact = SemanticFact { attr, val };
            if !self.facts[lti.0].contains(&fact) {
                self.facts[lti.0].push(fact);
            }
        }
    }

    /// Queries Semantic Memory for an LTI containing a matching (attribute, value) pair.
    pub fn query(&self, attr: SymbolId, val: SymbolId) -> Option<LtiId> {
        for (idx, facts) in self.facts.iter().enumerate() {
            if facts.iter().any(|f| f.attr == attr && f.val == val) {
                return Some(LtiId(idx));
            }
        }
        None
    }

    /// Retrieves all declarative facts associated with a given LTI.
    pub fn retrieve(&self, lti: LtiId) -> Option<&[SemanticFact]> {
        self.facts.get(lti.0).map(|v| v.as_slice())
    }

    /// Returns the total number of LTIs in Semantic Memory.
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Returns `true` if Semantic Memory contains no LTIs.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}
