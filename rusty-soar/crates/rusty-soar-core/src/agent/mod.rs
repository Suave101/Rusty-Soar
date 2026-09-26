use crate::rete::ReteNetwork;
use crate::symbol::SymbolTable;
use crate::wm::{SupportType, WmeArena, WmeKey};

/// Decision cycle execution phase for the Soar engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Ingest sensor inputs and external changes into Working Memory.
    #[default]
    Input,
    /// Elaborate state and fire rules proposing operator candidates.
    Proposal,
    /// Evaluate preferences to select an operator or trigger an impasse.
    Decision,
    /// Fire application rules corresponding to the active operator.
    Application,
    /// Dispatch output commands to actuators or external systems.
    Output,
}

/// The main Soar Cognitive Agent instance coordinating memory and RETE execution.
pub struct SoarAgent {
    /// Global symbol interning table.
    pub symbols: SymbolTable,
    /// Working Memory arena storing current state assertions.
    pub wm: WmeArena,
    /// Index-based RETE match engine.
    pub rete: ReteNetwork,
    /// Active phase of the Soar decision cycle.
    pub current_phase: Phase,
}

/// Alias for `SoarAgent` to satisfy public re-exports.
pub type Agent = SoarAgent;

impl SoarAgent {
    /// Creates a new `SoarAgent` instance with initialized memory components.
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
            wm: WmeArena::new(),
            rete: ReteNetwork::new(),
            current_phase: Phase::Input,
        }
    }

    /// Synchronizes Working Memory insertions into the RETE network.
    pub fn insert_wme(
        &mut self,
        s: crate::symbol::SymbolId,
        a: crate::symbol::SymbolId,
        v: crate::symbol::SymbolId,
    ) -> WmeKey {
        let key = self.wm.insert(s, a, v, SupportType::ISupport);
        self.rete.add_wme(key, s, a, v);
        key
    }

    /// Synchronizes Working Memory retractions into the RETE network.
    pub fn remove_wme(&mut self, key: WmeKey) {
        self.wm.remove(key);
        self.rete.remove_wme(key);
    }
}

impl Default for SoarAgent {
    fn default() -> Self {
        Self::new()
    }
}