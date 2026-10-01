use alloc::{boxed::Box, format, string::{String, ToString}, vec, vec::Vec};
use crate::epmem::{EpisodeId, EpisodicMemory};
use crate::impasse::{ImpasseType, SubstateRecord};
use crate::learning::ChunkBuilder;
use crate::preference::{
    resolve_preferences_with_rl, DecisionResult, Preference, PreferenceType,
};
use crate::rete::{AlphaTest, Field, ReteNetwork, VariableBinding};
use crate::rl::ReinforcementLearning;
use crate::soar_parser::{SoarPreference, SoarScript, SoarValue};
use crate::smem::{LtiId, SemanticMemory};
use crate::symbol::{SymbolId, SymbolTable};
use crate::tms::TruthMaintenanceSystem;
use crate::wm::{SupportType, WmeArena, WmeKey};

const MAX_ELABORATION_CYCLES: usize = 10;

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

/// The main Soar Cognitive Agent instance coordinating memory, RETE, decision processing, and learning.
pub struct SoarAgent {
    /// Global symbol interning table.
    pub symbols: SymbolTable,
    /// Working Memory arena storing current state assertions.
    pub wm: WmeArena,
    /// Index-based RETE match engine.
    pub rete: ReteNetwork,
    /// Long-term declarative Semantic Memory store.
    pub smem: SemanticMemory,
    /// Long-term autobiographical Episodic Memory store.
    pub epmem: EpisodicMemory,
    /// Dynamic Truth Maintenance System for I-support retractions.
    pub tms: TruthMaintenanceSystem,
    /// Reinforcement Learning engine managing numeric operator preference updates.
    pub rl: ReinforcementLearning,
    /// Active phase of the Soar decision cycle.
    pub current_phase: Phase,
    /// Active operator preference pool maintained across decision cycles.
    pub preferences: Vec<Preference>,
    /// Currently selected active operator handle (if any).
    pub selected_operator: Option<SymbolId>,
    /// Active architectural substates generated from impasses.
    pub substates: Vec<SubstateRecord>,
    /// Monotonically increasing state generation counter.
    pub state_counter: usize,
    /// Monotonically increasing decision cycle counter.
    pub cycle_counter: usize,
    /// Explanation-based learning chunk builder.
    pub chunk_builder: ChunkBuilder,
    /// Count of automatically synthesized chunk rules.
    pub chunks_learned: usize,
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
            smem: SemanticMemory::new(),
            epmem: EpisodicMemory::new(),
            tms: TruthMaintenanceSystem::new(),
            rl: ReinforcementLearning::default(),
            current_phase: Phase::Input,
            preferences: Vec::new(),
            selected_operator: None,
            substates: Vec::new(),
            state_counter: 1,
            cycle_counter: 0,
            chunk_builder: ChunkBuilder::new(),
            chunks_learned: 0,
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

    /// Installs parsed Soar productions using the current state as the flat WME identifier.
    ///
    /// This adapter intentionally supports the deterministic attribute/value and identifier-join
    /// subset represented by `SoarScript`. Relational tests and arbitrary RHS commands remain
    /// parser-level data until their richer AST representation is added.
    pub fn install_soar_script(&mut self, script: &SoarScript, state_id: SymbolId) {
        for production in &script.productions {
            let mut variables: Vec<(String, usize, Field)> = Vec::new();
            let conditions = production
                .conditions
                .iter()
                .enumerate()
                .map(|(condition_index, condition)| {
                    let attr = condition
                        .attribute_variable
                        .is_none()
                        .then(|| self.symbols.intern_str(&condition.attribute));
                    let value = match &condition.value {
                        SoarValue::Symbol(value) => value.clone(),
                        SoarValue::Int(value) => format!("{}", value),
                        SoarValue::Float(value) => format!("{}", value),
                        SoarValue::Bool(value) => format!("{}", value),
                    };
                    let val = condition
                        .value_variable
                        .is_none()
                        .then(|| self.symbols.intern_str(&value));
                    let id = if condition_index == 0
                        && condition.identifier.as_deref().is_some_and(|id| id == "state" || id == "<s>")
                    {
                        Some(state_id)
                    } else {
                        None
                    };
                    let mut bindings = Vec::new();
                    Self::add_variable_binding(
                        &mut variables,
                        &mut bindings,
                        condition.identifier.as_deref(),
                        condition_index,
                        Field::Id,
                    );
                    Self::add_variable_binding(
                        &mut variables,
                        &mut bindings,
                        condition.attribute_variable.as_deref(),
                        condition_index,
                        Field::Attr,
                    );
                    Self::add_variable_binding(
                        &mut variables,
                        &mut bindings,
                        condition.value_variable.as_deref(),
                        condition_index,
                        Field::Val,
                    );
                    (
                        AlphaTest {
                            id,
                            attr,
                            val,
                        },
                        bindings,
                    )
                })
                .collect();

            let Some(action) = production.actions.first() else {
                continue;
            };
            let operator = self.symbols.intern_str(&action.operator_name);
            let preference_type = match &action.preference {
                SoarPreference::Acceptable => PreferenceType::Acceptable,
                SoarPreference::Reject => PreferenceType::Reject,
                SoarPreference::Require => PreferenceType::Require,
                SoarPreference::Prohibit => PreferenceType::Prohibit,
                SoarPreference::Better(other) => PreferenceType::Better(
                    other
                        .as_deref()
                        .map(|value| self.symbols.intern_str(value))
                        .unwrap_or(operator),
                ),
                SoarPreference::Worse(other) => PreferenceType::Worse(
                    other
                        .as_deref()
                        .map(|value| self.symbols.intern_str(value))
                        .unwrap_or(operator),
                ),
                SoarPreference::Best => PreferenceType::Best,
                SoarPreference::Worst => PreferenceType::Worst,
                SoarPreference::Indifferent(_) => PreferenceType::Indifferent,
                SoarPreference::NumericIndifferent => PreferenceType::NumericIndifferent,
            };
            let rule_name: &'static str = Box::leak(production.name.clone().into_boxed_str());
            self.add_rule(
                rule_name,
                conditions,
                vec![Action::Prefer(Preference {
                    state: state_id,
                    operator,
                    preference_type,
                })],
            );
        }
    }

    fn add_variable_binding(
        variables: &mut Vec<(String, usize, Field)>,
        bindings: &mut Vec<VariableBinding>,
        variable: Option<&str>,
        condition_index: usize,
        field: Field,
    ) {
        let Some(variable) = variable.filter(|value| value.starts_with('<')) else {
            return;
        };
        if let Some((_, token_wme_index, token_field)) =
            variables.iter().find(|(name, _, _)| name == variable)
        {
            bindings.push(VariableBinding {
                token_wme_index: *token_wme_index,
                token_field: *token_field,
                wme_field: field,
            });
        } else {
            variables.push((variable.to_string(), condition_index, field));
        }
    }

    /// Synchronizes Working Memory insertions into the RETE network.
    pub fn insert_wme(&mut self, s: SymbolId, a: SymbolId, v: SymbolId) -> WmeKey {
        if let Some(existing) = self.wm.find(s, a, v) {
            return existing;
        }

        let key = self.wm.insert(s, a, v, SupportType::ISupport);
        self.rete.add_wme(key, s, a, v);
        key
    }

    /// Synchronizes Working Memory retractions and triggers cascading TMS retractions for unsupported I-supported WMEs.
    pub fn remove_wme(&mut self, key: WmeKey) {
        self.wm.remove(key);
        self.rete.remove_wme(key);

        let dependent_wmes = self.tms.process_retraction(key);
        for dep_key in dependent_wmes {
            self.wm.remove(dep_key);
            self.rete.remove_wme(dep_key);
        }
    }

    /// Records the current Working Memory snapshot as an episode in Episodic Memory.
    pub fn record_episode(&mut self) -> EpisodeId {
        self.epmem.record_episode(self.cycle_counter, &self.wm)
    }

    /// Queries Semantic Memory for an LTI matching an attribute-value cue and copies facts into Working Memory under `target_id`.
    pub fn retrieve_smem_to_wm(
        &mut self,
        cue_attr: SymbolId,
        cue_val: SymbolId,
        target_id: SymbolId,
    ) -> Option<LtiId> {
        if let Some(lti) = self.smem.query(cue_attr, cue_val) {
            if let Some(facts) = self.smem.retrieve(lti) {
                let facts_clone = facts.to_vec();
                for fact in facts_clone {
                    self.insert_wme(target_id, fact.attr, fact.val);
                }
            }
            Some(lti)
        } else {
            None
        }
    }

    /// Runs the Proposal Phase: Elaboration rules fire until RETE reaches quiescence.
    pub fn run_elaboration_phase(&mut self) -> usize {
        self.current_phase = Phase::Proposal;
        let mut total_fires = 0;
        let mut cycles = 0;

        while cycles < MAX_ELABORATION_CYCLES {
            cycles += 1;
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
                    let mut derived_wmes = Vec::new();

                    for action in actions_to_run {
                        match &action {
                            Action::Add { id, attr, val } => {
                                let wme_key = self.insert_wme(*id, *attr, *val);
                                derived_wmes.push(wme_key);
                            }
                            Action::Remove(key) => {
                                self.remove_wme(*key);
                            }
                            Action::Prefer(pref) => {
                                if !self.preferences.contains(pref) {
                                    self.preferences.push(pref.clone());
                                }

                                if let Some(substate) = self.substates.last() {
                                    if pref.state == substate.superstate_id {
                                        self.learn_chunk_for_result(
                                            substate.superstate_id,
                                            Action::Prefer(pref.clone()),
                                        );
                                    }
                                }
                            }
                        }
                    }

                    if !derived_wmes.is_empty() {
                        let supporting_keys: Vec<WmeKey> =
                            inst.matched_wmes.iter().map(|w| w.key).collect();

                        self.tms.add_justification(
                            inst.rule_name,
                            supporting_keys,
                            derived_wmes,
                        );
                    }
                }
            }
        }

        total_fires
    }

    /// Resolves superstate conditions and automatically registers a learned Chunk rule into RETE.
    fn learn_chunk_for_result(&mut self, superstate_id: SymbolId, result_action: Action) {
        let superstate_conditions = vec![(
            AlphaTest {
                id: Some(superstate_id),
                attr: None,
                val: None,
            },
            vec![],
        )];

        let chunk = self.chunk_builder.build_chunk(superstate_conditions, result_action);
        self.chunks_learned += 1;

        self.add_rule(chunk.name, chunk.conditions, chunk.actions);
    }

    /// Runs the Decision Phase: Evaluates preferences with RL support to select an operator or create an impasse.
    pub fn run_decision_phase(&mut self, state_id: SymbolId) -> DecisionResult {
        self.current_phase = Phase::Decision;
        let result = resolve_preferences_with_rl(state_id, &self.preferences, &self.rl);

        match &result {
            DecisionResult::Selected(op_id) => {
                self.selected_operator = Some(*op_id);
                self.purge_substates_for_superstate(state_id);
            }
            DecisionResult::TieImpasse(candidates) => {
                self.selected_operator = None;
                self.create_impasse_substate(state_id, ImpasseType::Tie, candidates);
            }
            DecisionResult::ConflictImpasse(candidates) => {
                self.selected_operator = None;
                self.create_impasse_substate(state_id, ImpasseType::Conflict, candidates);
            }
            DecisionResult::NoChangeImpasse => {
                self.selected_operator = None;
                self.create_impasse_substate(state_id, ImpasseType::NoChange, &[]);
            }
        }

        result
    }

    /// Purges a substate and all associated Working Memory elements from the agent.
    pub fn purge_substates_for_superstate(&mut self, superstate_id: SymbolId) {
        let mut to_remove = Vec::new();

        for (idx, record) in self.substates.iter().enumerate() {
            if record.superstate_id == superstate_id {
                to_remove.push((idx, record.substate_id));
            }
        }

        for (idx, substate_id) in to_remove.into_iter().rev() {
            let wme_keys = self.wm.wmes_by_id(substate_id);
            for key in wme_keys {
                self.remove_wme(key);
            }

            self.substates.remove(idx);
        }
    }

    /// Runs Application Phase rules matching the currently selected operator until quiescence.
    pub fn run_application_phase(&mut self) -> usize {
        self.current_phase = Phase::Application;
        if self.selected_operator.is_none() {
            return 0;
        }

        self.run_elaboration_phase()
    }

    /// Executes one complete 5-phase Soar Decision Cycle and records an autobiographical episode.
    pub fn run_decision_cycle(&mut self, state_id: SymbolId) -> DecisionResult {
        self.cycle_counter += 1;

        // 1. Input Phase
        self.current_phase = Phase::Input;

        // 2. Proposal Phase
        self.run_elaboration_phase();

        // 3. Decision Phase
        let decision = self.run_decision_phase(state_id);

        // 4. Application Phase
        if self.selected_operator.is_some() {
            self.run_application_phase();
        }

        // 5. Output Phase
        self.current_phase = Phase::Output;

        self.record_episode();

        decision
    }

    /// Generates a substate WME hierarchy in Working Memory to represent an architectural impasse.
    fn create_impasse_substate(
        &mut self,
        superstate_id: SymbolId,
        impasse_type: ImpasseType,
        candidates: &[SymbolId],
    ) -> SymbolId {
        self.purge_substates_for_superstate(superstate_id);
        self.state_counter += 1;
        let substate_id = self.symbols.intern_id('S', self.state_counter as u64);

        let attr_superstate = self.symbols.intern_str("superstate");
        let attr_type = self.symbols.intern_str("type");
        let attr_impasse = self.symbols.intern_str("impasse");
        let attr_item = self.symbols.intern_str("item");

        let val_impasse = self.symbols.intern_str("impasse");
        let val_type_str = match impasse_type {
            ImpasseType::Tie => self.symbols.intern_str("tie"),
            ImpasseType::Conflict => self.symbols.intern_str("conflict"),
            ImpasseType::NoChange => self.symbols.intern_str("no-change"),
        };

        self.insert_wme(substate_id, attr_superstate, superstate_id);
        self.insert_wme(substate_id, attr_type, val_impasse);
        self.insert_wme(substate_id, attr_impasse, val_type_str);

        for &cand in candidates {
            self.insert_wme(substate_id, attr_item, cand);
        }

        self.substates.push(SubstateRecord {
            substate_id,
            superstate_id,
            impasse_type,
        });

        substate_id
    }
}

impl Default for SoarAgent {
    fn default() -> Self {
        Self::new()
    }
}