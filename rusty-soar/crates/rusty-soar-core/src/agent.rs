//! Top-level Soar agent orchestration and decision cycle driver.

use alloc::vec::Vec;
use crate::decision::{Preference, PreferenceType};
use crate::rete::ReteNetwork;
use crate::symbol::{SymbolId, SymbolTable};
use crate::wm::{SupportType, WmeArena, WmeKey};

/// Execution state of the top-level decision loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Ingesting telemetry or external environment inputs
    Input,
    /// Parallel rule elaboration and operator proposal phase
    Elaboration,
    /// Evaluating preferences and selecting the active operator
    Decision,
    /// Executing operator application rules and state mutations
    Application,
    /// Dispatching output commands to external systems
    Output,
}

/// A standalone Soar cognitive agent instance managing WM, Rete matching, and decision steps.
pub struct Agent {
    /// String interning symbol table
    pub symbols: SymbolTable,
    /// Working Memory Element generational arena
    pub wm: WmeArena,
    /// Rete pattern matching network
    pub rete: ReteNetwork,
    /// Current decision cycle phase
    pub current_phase: Phase,
    /// Currently selected active operator symbol ID
    pub active_operator: Option<SymbolId>,
    /// Accumulated preferences during current Elaboration phase
    pub preference_buffer: Vec<Preference>,
}

impl Agent {
    /// Initializes a new `#![no_std]` Soar Agent instance.
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
            wm: WmeArena::new(),
            rete: ReteNetwork::new(),
            current_phase: Phase::Input,
            active_operator: None,
            preference_buffer: Vec::new(),
        }
    }

    /// Adds a WME to Working Memory and propagates it through the Rete network.
    pub fn add_wme(
        &mut self,
        id: SymbolId,
        attribute: SymbolId,
        value: SymbolId,
        support: SupportType,
    ) -> WmeKey {
        let key = self.wm.insert(id, attribute, value, support);
        if let Some(wme) = self.wm.get(key) {
            self.rete.add_wme(key, wme);
        }
        key
    }

    /// Retracts a WME from Working Memory and updates the Rete network.
    pub fn remove_wme(&mut self, key: WmeKey) -> Option<crate::wm::Wme> {
        self.rete.remove_wme(key);
        self.wm.remove(key)
    }

    /// Executes one full Soar Decision Cycle (Input -> Elaboration -> Decision -> Application -> Output).
    pub fn step(&mut self) {
        // 1. Input Phase
        self.current_phase = Phase::Input;

        // 2. Elaboration Phase: drain Rete instantiations and collect candidate preferences
        self.current_phase = Phase::Elaboration;
        for inst in self.rete.instantiations.drain(..) {
            self.preference_buffer.extend(inst.preferences);
        }

        // 3. Decision Phase (evaluate operator preferences)
        self.current_phase = Phase::Decision;
        self.resolve_decision();

        // 4. Application Phase (apply operator rules)
        self.current_phase = Phase::Application;

        // 5. Output Phase
        self.current_phase = Phase::Output;
    }

    /// Evaluates accumulated preferences to select or clear the active operator.
    fn resolve_decision(&mut self) {
        let mut acceptable = Vec::new();
        let mut rejected = Vec::new();

        for pref in &self.preference_buffer {
            match pref.pref_type {
                PreferenceType::Acceptable => acceptable.push(pref.candidate),
                PreferenceType::Reject => rejected.push(pref.candidate),
                _ => {}
            }
        }

        // Filter rejected candidates
        acceptable.retain(|candidate| !rejected.contains(candidate));

        // Deterministically select the first valid acceptable candidate
        self.active_operator = acceptable.first().copied();
        self.preference_buffer.clear();
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}