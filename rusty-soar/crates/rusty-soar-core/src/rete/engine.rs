//! Rete pattern matching network engine and propagation routines.

use alloc::vec::Vec;
use crate::rete::alpha::{AlphaMemory, AlphaNode};
use crate::rete::beta::{BetaMemory, JoinNode};
use crate::rete::production::{Instantiation, ProductionNode};
use crate::wm::{Wme, WmeKey};

/// Central execution graph for Rete pattern matching across Working Memory changes.
#[derive(Debug, Default)]
pub struct ReteNetwork {
    /// Collection of registered Alpha Memories holding WMEs
    pub alpha_memories: Vec<AlphaMemory>,
    /// Collection of Alpha constant condition test nodes
    pub alpha_nodes: Vec<AlphaNode>,
    /// Collection of Beta Memories holding active partial match tokens
    pub beta_memories: Vec<BetaMemory>,
    /// Collection of Join Nodes evaluating cross-element variable constraints
    pub join_nodes: Vec<JoinNode>,
    /// Collection of terminal Production Nodes triggering rule instantiations
    pub production_nodes: Vec<ProductionNode>,
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
            join_nodes: Vec::new(),
            production_nodes: Vec::new(),
            instantiations: Vec::new(),
        }
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
    }

    /// Retracts a removed WME from all Alpha Memories across the network.
    pub fn remove_wme(&mut self, key: WmeKey) {
        for alpha_mem in &mut self.alpha_memories {
            alpha_mem.remove(key);
        }
    }

    /// Clears all pending rule instantiations from the current cycle agenda.
    pub fn clear_instantiations(&mut self) {
        self.instantiations.clear();
    }
}