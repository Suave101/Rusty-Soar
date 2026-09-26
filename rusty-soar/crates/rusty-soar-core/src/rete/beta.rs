use alloc::vec::Vec;
use slotmap::{new_key_type, SlotMap};
use crate::wm::{Wme, WmeKey};

new_key_type! {
    /// Generational key referencing a Token in the Beta network.
    pub struct TokenKey;
}

/// Variable binding test performed across Join Nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinTest {
    /// Field position in the left parent WME/token
    pub left_field: FieldPosition,
    /// Field position in the right WME
    pub right_field: FieldPosition,
}

/// Identifies a specific slot within a WME triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPosition {
    /// The root Identifier slot of a WME (e.g., S1)
    Identifier,
    /// The Attribute edge slot of a WME (e.g., ^status)
    Attribute,
    /// The Value node slot of a WME (e.g., active)
    Value,
}

impl FieldPosition {
    /// Extracts the target SymbolId value from a given WME triple.
    pub fn extract(&self, wme: &Wme) -> crate::symbol::SymbolId {
        match self {
            FieldPosition::Identifier => wme.id,
            FieldPosition::Attribute => wme.attribute,
            FieldPosition::Value => wme.value,
        }
    }
}

/// A Token represents a chain of matched WMEs satisfying a partial production.
#[derive(Debug, Clone)]
pub struct Token {
    /// Key to parent token higher in the Beta tree
    pub parent: Option<TokenKey>,
    /// WME key joined at this node
    pub wme: WmeKey,
}

/// Storage container for valid partial matches in the Beta network.
#[derive(Debug, Default)]
pub struct BetaMemory {
    tokens: SlotMap<TokenKey, Token>,
}

impl BetaMemory {
    /// Creates a new, empty Beta Memory.
    pub fn new() -> Self {
        Self {
            tokens: SlotMap::with_key(),
        }
    }

    /// Inserts a new token entry into memory.
    pub fn insert(&mut self, parent: Option<TokenKey>, wme: WmeKey) -> TokenKey {
        self.tokens.insert(Token { parent, wme })
    }

    /// Removes a token entry upon retraction.
    pub fn remove(&mut self, key: TokenKey) -> Option<Token> {
        self.tokens.remove(key)
    }

    /// Immutable lookup for a token by its key.
    pub fn get(&self, key: TokenKey) -> Option<&Token> {
        self.tokens.get(key)
    }

    /// Returns all token keys currently in memory.
    pub fn keys(&self) -> Vec<TokenKey> {
        self.tokens.keys().collect()
    }
}

/// Join node testing inter-variable equality between Beta tokens and Alpha WMEs.
#[derive(Debug, Clone)]
pub struct JoinNode {
    /// Associated Alpha Memory index on the right side of the join
    pub alpha_memory_id: usize,
    /// Parent Beta Memory index providing input token chains
    pub parent_beta_memory: Option<usize>,
    /// Child Beta Memory index where passing token joins are pushed
    pub child_beta_memory: usize,
    /// Equality constraint tests to evaluate against incoming tokens and WMEs
    pub tests: Vec<JoinTest>,
}

impl JoinNode {
    /// Evaluates all variable equality conditions between an existing WME chain and a new WME.
    pub fn evaluate(&self, left_wme: &Wme, right_wme: &Wme) -> bool {
        for test in &self.tests {
            if test.left_field.extract(left_wme) != test.right_field.extract(right_wme) {
                return false;
            }
        }
        true
    }
}