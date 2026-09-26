//! Rete pattern matching network engine and propagation routines.

use alloc::vec::Vec;
use crate::decision::Preference;
use crate::rete::alpha::{AlphaMemory, AlphaNode, ConstantTest};
use crate::rete::beta::{BetaMemory, FieldPosition, TokenKey};
use crate::rete::production::{Instantiation, ProductionId, ProductionNode};
use crate::symbol::SymbolId;
use crate::wm::{Wme, WmeKey};

/// Registered rule definition inside the Rete network.
#[derive(Debug, Clone)]
pub struct RuleRule {
    /// Associated Production ID
    pub id: ProductionId,
    /// Attribute condition to match
    pub attribute: SymbolId,
    /// Value condition to match
    pub value: SymbolId,
    /// Right-hand side candidate preferences to assert when fired
    pub preferences: Vec<Preference>,
}

/// Central execution graph for Rete pattern matching across Working Memory changes.
#[derive(Debug, Default)]
pub struct ReteNetwork {
    /// Collection of registered Alpha Memories holding WMEs
    pub alpha_memories: Vec<AlphaMemory>,
    /// Collection of Alpha constant condition test nodes
    pub alpha_nodes: Vec<AlphaNode>,
    /// Collection of Beta Memories holding active partial match tokens
    pub beta_memories: Vec<BetaMemory>,
    /// Collection of terminal Production Nodes triggering rule instantiations
    pub production_nodes: Vec<ProductionNode>,
    /// Registered single-attribute rules
    pub rules: Vec<RuleRule>,
    /// Active agenda of production instantiations ready for Elaboration
    pub instantiations: Vec<Instantiation>,
}

impl ReteNetwork {
    /// Creates a new, empty Rete Network instance.
    pub fn new() -> Self {
        Self {
            alpha_memories: Vec::new(),
            alpha_nodes: Vec::new(),
            beta_memories: Vec::new(),
            production_nodes: Vec::new(),
            rules: Vec::new(),
            instantiations: Vec::new(),
        }
    }

    /// Registers a single-condition production rule into the Rete network.
    pub fn register_rule(
        &mut self,
        id: ProductionId,
        attribute: SymbolId,
        value: SymbolId,
        preferences: Vec<Preference>,
    ) {
        let alpha_mem_id = self.alpha_memories.len();
        self.alpha_memories.push(AlphaMemory::new());

        self.alpha_nodes.push(AlphaNode {
            test: ConstantTest::Attribute(attribute),
            alpha_memory_id: alpha_mem_id,
        });

        self.rules.push(RuleRule {
            id,
            attribute,
            value,
            preferences,
        });
    }

    /// Evaluates a newly added Working Memory Element through the Alpha network nodes.
    pub fn add_wme(&mut self, key: WmeKey, wme: &Wme) {
        for node in &self.alpha_nodes {
            if node.matches(wme.attribute, wme.value) {
                if let Some(alpha_mem) = self.alpha_memories.get_mut(node.alpha_memory_id) {
                    alpha_mem.insert(key);
                }
            }
        }

        // Check if WME satisfies any registered production rules
        for rule in &self.rules {
            if wme.attribute == rule.attribute && wme.value == rule.value {
                self.instantiations.push(Instantiation {
                    production_id: rule.id,
                    token: TokenKey::default(),
                    preferences: rule.preferences.clone(),
                });
            }
        }
    }

    /// Retracts a removed WME from all Alpha Memories across the network.
    pub fn remove_wme(&mut self, key: WmeKey) {
        for alpha_mem in &mut self.alpha_memories {
            alpha_mem.remove(key);
        }
    }

    /// Evaluates join constraints between a Token and a WME.
    pub fn evaluate_join(
        _parent_token: Option<TokenKey>,
        wme: &Wme,
        left_pos: FieldPosition,
        right_pos: FieldPosition,
    ) -> bool {
        let right_val = match right_pos {
            FieldPosition::Identifier => wme.id,
            FieldPosition::Attribute => wme.attribute,
            FieldPosition::Value => wme.value,
        };

        let left_val = match left_pos {
            FieldPosition::Identifier => wme.id,
            FieldPosition::Attribute => wme.attribute,
            FieldPosition::Value => wme.value,
        };

        left_val == right_val
    }

    /// Clears all pending rule instantiations from the current cycle agenda.
    pub fn clear_instantiations(&mut self) {
        self.instantiations.clear();
    }
}