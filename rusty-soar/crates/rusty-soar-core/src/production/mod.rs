use crate::agent::Action;
use crate::rete::{AlphaTest, VariableBinding};
use alloc::vec::Vec;

/// Representation of a Soar production rule containing conditions and actions.
#[derive(Debug, Clone)]
pub struct Production {
    /// Unique identifier label for the rule.
    pub name: &'static str,
    /// RETE conditions (alpha test filter + variable bindings per condition).
    pub conditions: Vec<(AlphaTest, Vec<VariableBinding>)>,
    /// Right-hand side actions executed upon instantiation.
    pub actions: Vec<Action>,
}

impl Production {
    /// Creates a new production rule instance.
    pub fn new(
        name: &'static str,
        conditions: Vec<(AlphaTest, Vec<VariableBinding>)>,
        actions: Vec<Action>,
    ) -> Self {
        Self {
            name,
            conditions,
            actions,
        }
    }
}
