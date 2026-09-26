use alloc::vec::Vec;
use crate::symbol::SymbolId;
use crate::wm::WmeKey;

/// Represents a constant value test on a single WME field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstantTest {
    /// Test matches a specific Attribute symbol ID
    Attribute(SymbolId),
    /// Test matches a specific Value symbol ID
    Value(SymbolId),
}

/// An Alpha Node that filters incoming WMEs based on constant conditions.
#[derive(Debug, Clone)]
pub struct AlphaNode {
    /// Test condition to evaluate
    pub test: ConstantTest,
    /// Destination Alpha Memory containing WMEs that passed this test
    pub alpha_memory_id: usize,
}

impl AlphaNode {
    /// Evaluates whether a given WME field satisfies this node's constant test.
    pub fn matches(&self, attr: SymbolId, val: SymbolId) -> bool {
        match self.test {
            ConstantTest::Attribute(expected) => attr == expected,
            ConstantTest::Value(expected) => val == expected,
        }
    }
}

/// Storage for all active WMEs matching a specific single-element pattern.
#[derive(Debug, Default)]
pub struct AlphaMemory {
    /// WME keys stored in this memory
    wmes: Vec<WmeKey>,
}

impl AlphaMemory {
    /// Creates a new, empty Alpha Memory.
    pub fn new() -> Self {
        Self { wmes: Vec::new() }
    }

    /// Adds a WME key to this memory.
    pub fn insert(&mut self, wme_key: WmeKey) {
        if !self.wmes.contains(&wme_key) {
            self.wmes.push(wme_key);
        }
    }

    /// Removes a WME key from this memory upon retraction.
    pub fn remove(&mut self, wme_key: WmeKey) {
        self.wmes.retain(|&k| k != wme_key);
    }

    /// Returns a slice of all WME keys currently residing in this memory.
    pub fn wmes(&self) -> &[WmeKey] {
        &self.wmes
    }
}