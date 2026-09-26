use alloc::vec::Vec;
use crate::preference::{resolve_preferences, DecisionResult, Preference};
use crate::rete::{AlphaTest, ReteNetwork, VariableBinding};
use crate::symbol::{SymbolId, SymbolTable};
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

/// Production action specifying Working Memory or Preference modifications.
#[derive(Debug, Clone)]
pub enum Action {
    /// Asserts a new WME triple into Working Memory.
    Add {
        /// Identifier symbol.
        id: SymbolId,
        /// Attribute symbol.
        attr: SymbolId,
        /// Value symbol.
        val: SymbolId,
    },
    /// Retracts an existing WME by key handle.
    Remove(WmeKey),
    /// Emits an architectural operator preference.
    Prefer(Preference),
}

/// The main Soar Cognitive Agent instance coordinating memory, RETE, and decision processing.
pub struct SoarAgent {
    /// Global symbol interning table.
    pub symbols: SymbolTable,
    /// Working Memory arena storing current state assertions.
    pub wm: WmeArena,
    /// Index-based RETE match engine.
    pub rete: ReteNetwork,
    /// Active phase of the Soar decision cycle.
    pub current_phase: Phase,
    /// Active operator preference pool for the current cycle.
    pub preferences: Vec<Preference>,
    /// Currently selected active operator handle (if any).
    pub selected_operator: Option<SymbolId>,
    /// Action callbacks mapped to production rules by name.
    pub rule_actions: Vec<(&'static str, Vec<Action>)>,
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
            preferences: Vec::new(),
            selected_operator: None,
            rule_actions: Vec::new(),
        }
    }

    /// Registers a production rule with its RETE conditions and RHS execution actions.
    pub fn add_rule(
        &mut self,
        name: &'static str,
        conditions: Vec<(AlphaTest, Vec<VariableBinding>)>,
        actions: Vec<Action>,
    ) {
        self.rete.add_rule(name, conditions);
        self.rule_actions.push((name, actions));
    }

    /// Synchronizes Working Memory insertions into the RETE network.
    pub fn insert_wme(&mut self, s: SymbolId, a: SymbolId, v: SymbolId) -> WmeKey {
        let key = self.wm.insert(s, a, v, SupportType::ISupport);
        self.rete.add_wme(key, s, a, v);
        key
    }

    /// Synchronizes Working Memory retractions into the RETE network.
    pub fn remove_wme(&mut self, key: WmeKey) {
        self.wm.remove(key);
        self.rete.remove_wme(key);
    }

    /// Runs the Proposal Phase: Elaboration rules fire until RETE reaches quiescence.
    pub fn run_elaboration_phase(&mut self) -> usize {
        self.current_phase = Phase::Proposal;
        let mut total_fires = 0;

        loop {
            let activations = core::mem::take(&mut self.rete.activations);
            if activations.is_empty() {
                break;
            }

            total_fires += activations.len();

            for inst in activations {
                if let Some((_, actions)) =
                    self.rule_actions.iter().find(|(name, _)| *name == inst.rule_name)
                {
                    let actions_to_run = actions.clone();
                    for action in actions_to_run {
                        match action {
                            Action::Add { id, attr, val } => {
                                self.insert_wme(id, attr, val);
                            }
                            Action::Remove(key) => {
                                self.remove_wme(key);
                            }
                            Action::Prefer(pref) => {
                                self.preferences.push(pref);
                            }
                        }
                    }
                }
            }
        }

        total_fires
    }

    /// Runs the Decision Phase: Evaluates preferences to select an operator or handle an impasse.
    pub fn run_decision_phase(&mut self, state_id: SymbolId) -> DecisionResult {
        self.current_phase = Phase::Decision;
        let result = resolve_preferences(state_id, &self.preferences);

        if let DecisionResult::Selected(op_id) = result {
            self.selected_operator = Some(op_id);
        } else {
            self.selected_operator = None;
        }

        result
    }
}

impl Default for SoarAgent {
    fn default() -> Self {
        Self::new()
    }
}