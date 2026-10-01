use alloc::{boxed::Box, format, string::{String, ToString}, vec, vec::Vec};
use crate::epmem::{EpisodeId, EpisodicMemory};
use crate::impasse::{ImpasseType, SubstateRecord};
use crate::learning::ChunkBuilder;
use crate::preference::{
    resolve_preferences_with_rl, DecisionResult, Preference, PreferenceType,
};
use crate::rete::{
    AlphaTest, Field, InstantiationId, Relation, ReteNetwork, VariableBinding,
};
use crate::rl::ReinforcementLearning;
use crate::soar_parser::{
    SoarCondition, SoarPreference, SoarRelation, SoarRhsFunction, SoarScript, SoarValue,
};
use crate::smem::{LtiId, SemanticMemory};
use crate::symbol::{SymbolData, SymbolId, SymbolTable};
use crate::tms::TruthMaintenanceSystem;
use crate::wm::{SupportType, WmeArena, WmeKey};

const MAX_ELABORATION_CYCLES: usize = 1024;

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
    /// Asserts a WME whose identifier and value are resolved from the matched token.
    AddWme {
        /// Identifier expression.
        id: WmeTerm,
        /// Attribute symbol.
        attr: SymbolId,
        /// Value expression.
        val: WmeTerm,
    },
    /// Retracts an existing WME by key handle.
    Remove(WmeKey),
    /// Retracts a WME whose identifier and value are resolved from the matched token.
    RemoveWme {
        /// Identifier expression.
        id: WmeTerm,
        /// Attribute symbol.
        attr: SymbolId,
        /// Value expression.
        val: WmeTerm,
    },
    /// Emits an architectural operator preference.
    Prefer(Preference),
    /// Stops the agent permanently.
    Halt,
    /// Stops the current elaboration phase.
    Interrupt,
    /// Appends deterministic text output.
    Write(String),
}

/// A symbol literal or a field selected from a matched WME token.
#[derive(Debug, Clone)]
pub enum WmeTerm {
    /// A fixed interned symbol.
    Literal(SymbolId),
    /// A variable bound to one field of one LHS WME.
    Binding { wme_index: usize, field: Field },
}

#[derive(Debug, Clone)]
struct NegativePattern {
    id: WmeTerm,
    attr: Option<SymbolId>,
    val: WmeTerm,
    relation: Relation,
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
    negative_patterns: Vec<(&'static str, Vec<NegativePattern>)>,
    negative_blocked: Vec<InstantiationId>,
    /// Goal Dependency Sets: substate IDs mapped to superstate WME dependencies.
    goal_dependencies: Vec<(SymbolId, Vec<WmeKey>)>,
    /// Whether a halt RHS function has permanently stopped execution.
    pub halted: bool,
    /// Whether an interrupt RHS function stopped the current phase.
    pub interrupted: bool,
    /// Deterministic output captured by write RHS functions.
    pub output: Vec<String>,
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
            negative_patterns: Vec::new(),
            negative_blocked: Vec::new(),
            goal_dependencies: Vec::new(),
            halted: false,
            interrupted: false,
            output: Vec::new(),
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
            let expanded_conditions: Vec<SoarCondition> = production
                .conditions
                .iter()
                .enumerate()
                .flat_map(|(index, condition)| Self::expand_attribute_path(condition, index))
                .collect();
            let mut variables: Vec<(String, usize, Field)> = Vec::new();
            let mut relation_tests = Vec::new();
            let mut negative_conditions = Vec::new();
            let mut positive_condition_index = 0;
            let conditions = expanded_conditions
                .iter()
                .filter_map(|condition| {
                    if condition.negated {
                        negative_conditions.push(condition);
                        return None;
                    }
                    let condition_index = positive_condition_index;
                    positive_condition_index += 1;
                    let attr = condition
                        .attribute_variable
                        .is_none()
                        .then(|| self.symbols.intern_str(&condition.attribute));
                    let value = match &condition.value {
                        SoarValue::Symbol(value) => value.clone(),
                        SoarValue::Int(value) => format!("{}", value),
                        SoarValue::Float(value) => format!("{}", value),
                        SoarValue::Bool(value) => format!("{}", value),
                        SoarValue::Disjunction(values) => values
                            .first()
                            .map(Self::soar_value_to_string)
                            .unwrap_or_default(),
                        SoarValue::Arithmetic { .. } => String::new(),
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
                    let relation = match condition.relation {
                        SoarRelation::Equal => Relation::Equal,
                        SoarRelation::NotEqual => Relation::NotEqual,
                        SoarRelation::Less => Relation::Less,
                        SoarRelation::LessOrEqual => Relation::LessOrEqual,
                        SoarRelation::Greater => Relation::Greater,
                        SoarRelation::GreaterOrEqual => Relation::GreaterOrEqual,
                    };
                    if let Some(number) = match &condition.value {
                        SoarValue::Int(value) => Some(*value as f64),
                        SoarValue::Float(value) => Some(*value as f64),
                        _ => None,
                    } {
                        if let Some(symbol) = val {
                            self.rete.register_numeric_value(symbol, number);
                        }
                    }
                    let alpha_test = AlphaTest {
                            id,
                            attr,
                            val,
                        };
                    if let SoarValue::Disjunction(values) = &condition.value {
                        let alternatives: Vec<SymbolId> = values
                            .iter()
                            .map(|value| self.symbols.intern_str(&Self::soar_value_to_string(value)))
                            .collect();
                        self.rete.register_alternatives(&alpha_test, alternatives);
                    }
                    relation_tests.push((alpha_test.clone(), relation));
                    Some((
                        alpha_test,
                        bindings,
                    ))
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
            let mut negative_patterns = Vec::new();
            for condition in negative_conditions {
                let Some(id) = Self::compile_wme_term(
                    &variables,
                    condition.identifier.as_deref().unwrap_or(""),
                    &mut self.symbols,
                ) else {
                    continue;
                };
                let Some(val) = Self::compile_value_term(
                    &variables,
                    &condition.value,
                    &mut self.symbols,
                ) else {
                    continue;
                };
                let attr = condition
                    .attribute_variable
                    .is_none()
                    .then(|| self.symbols.intern_str(&condition.attribute));
                let relation = match condition.relation {
                    SoarRelation::Equal => Relation::Equal,
                    SoarRelation::NotEqual => Relation::NotEqual,
                    SoarRelation::Less => Relation::Less,
                    SoarRelation::LessOrEqual => Relation::LessOrEqual,
                    SoarRelation::Greater => Relation::Greater,
                    SoarRelation::GreaterOrEqual => Relation::GreaterOrEqual,
                };
                negative_patterns.push(NegativePattern { id, attr, val, relation });
            }
            let mut rule_actions = vec![Action::Prefer(Preference {
                state: state_id,
                operator,
                preference_type,
            })];
            for function in &production.rhs_functions {
                rule_actions.push(match function {
                    SoarRhsFunction::Halt => Action::Halt,
                    SoarRhsFunction::Interrupt => Action::Interrupt,
                    SoarRhsFunction::Write(text) => Action::Write(text.clone()),
                });
            }
            for wme_action in &production.wme_actions {
                let Some(id) = Self::compile_wme_term(
                    &variables,
                    &wme_action.identifier,
                    &mut self.symbols,
                ) else {
                    continue;
                };
                let Some(val) = Self::compile_value_term(
                    &variables,
                    &wme_action.value,
                    &mut self.symbols,
                ) else {
                    continue;
                };
                let attr = self.symbols.intern_str(&wme_action.attribute);
                if wme_action.remove {
                    rule_actions.push(Action::RemoveWme { id, attr, val });
                } else {
                    rule_actions.push(Action::AddWme { id, attr, val });
                }
            }
            for (test, relation) in &relation_tests {
                self.rete.register_relation(test, *relation);
            }
            self.add_rule(
                rule_name,
                conditions,
                rule_actions,
            );
            self.negative_patterns.push((rule_name, negative_patterns));
        }
    }

    fn expand_attribute_path(condition: &SoarCondition, condition_index: usize) -> Vec<SoarCondition> {
        let segments: Vec<&str> = condition.attribute.split('.').collect();
        if segments.len() <= 1 {
            return vec![condition.clone()];
        }

        let mut expanded = Vec::new();
        let mut previous_identifier = condition.identifier.clone();
        for (segment_index, segment) in segments.iter().enumerate() {
            let last = segment_index + 1 == segments.len();
            let next_identifier = format!("<__path{}_{}>", condition_index, segment_index);
            expanded.push(SoarCondition {
                identifier: previous_identifier.clone(),
                attribute: (*segment).to_string(),
                attribute_variable: None,
                value: if last {
                    condition.value.clone()
                } else {
                    SoarValue::Symbol(next_identifier.clone())
                },
                value_variable: if last {
                    condition.value_variable.clone()
                } else {
                    Some(next_identifier.clone())
                },
                relation: if last {
                    condition.relation
                } else {
                    SoarRelation::Equal
                },
                negated: condition.negated,
            });
            previous_identifier = Some(next_identifier);
        }
        expanded
    }

    fn compile_wme_term(
        variables: &[(String, usize, Field)],
        expression: &str,
        symbols: &mut SymbolTable,
    ) -> Option<WmeTerm> {
        if let Some((_, wme_index, field)) = variables.iter().find(|(name, _, _)| name == expression) {
            return Some(WmeTerm::Binding {
                wme_index: *wme_index,
                field: *field,
            });
        }
        (!expression.starts_with('<')).then(|| WmeTerm::Literal(symbols.intern_str(expression)))
    }

    fn compile_value_term(
        variables: &[(String, usize, Field)],
        value: &SoarValue,
        symbols: &mut SymbolTable,
    ) -> Option<WmeTerm> {
        if let SoarValue::Symbol(expression) = value {
            if expression.starts_with('<') {
                return Self::compile_wme_term(variables, expression, symbols);
            }
        }
        let literal = match value {
            SoarValue::Symbol(value) => value.clone(),
            SoarValue::Int(value) => format!("{}", value),
            SoarValue::Float(value) => format!("{}", value),
            SoarValue::Bool(value) => format!("{}", value),
            SoarValue::Disjunction(_) => return None,
            SoarValue::Arithmetic { operator, operands } => {
                Self::evaluate_arithmetic(*operator, operands)?
            }
        };
        Some(WmeTerm::Literal(symbols.intern_str(&literal)))
    }

    fn soar_value_to_string(value: &SoarValue) -> String {
        match value {
            SoarValue::Symbol(value) => value.clone(),
            SoarValue::Int(value) => format!("{}", value),
            SoarValue::Float(value) => format!("{}", value),
            SoarValue::Bool(value) => format!("{}", value),
            SoarValue::Disjunction(_) => String::new(),
            SoarValue::Arithmetic { .. } => String::new(),
        }
    }

    fn evaluate_arithmetic(operator: char, operands: &[SoarValue]) -> Option<String> {
        let numbers: Vec<f64> = operands
            .iter()
            .map(|operand| match operand {
                SoarValue::Int(value) => Some(*value as f64),
                SoarValue::Float(value) => Some(*value as f64),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let first = *numbers.first()?;
        let result = numbers.iter().skip(1).fold(first, |total, value| match operator {
            '+' => total + value,
            '-' => total - value,
            '*' => total * value,
            '/' => total / value,
            _ => total,
        });
        Some(if result == result as i64 as f64 {
            format!("{}", result as i64)
        } else {
            format!("{}", result)
        })
    }

    fn resolve_wme_term(term: &WmeTerm, matched_wmes: &[crate::rete::WmeRecord]) -> Option<SymbolId> {
        match term {
            WmeTerm::Literal(symbol) => Some(*symbol),
            WmeTerm::Binding { wme_index, field } => matched_wmes
                .get(*wme_index)
                .map(|wme| field.extract(&wme.triple)),
        }
    }

    fn negative_patterns_match(
        &self,
        rule_name: &'static str,
        matched_wmes: &[crate::rete::WmeRecord],
    ) -> bool {
        let Some((_, patterns)) = self
            .negative_patterns
            .iter()
            .find(|(name, _)| *name == rule_name)
        else {
            return false;
        };
        patterns.iter().any(|pattern| {
            let (Some(id), Some(val)) = (
                Self::resolve_wme_term(&pattern.id, matched_wmes),
                Self::resolve_wme_term(&pattern.val, matched_wmes),
            ) else {
                return false;
            };
            self.wm.iter().any(|wme| {
                wme.id == id
                    && pattern.attr.is_none_or(|attr| attr == wme.attr)
                    && AlphaTest {
                        id: None,
                        attr: None,
                        val: Some(val),
                    }
                    .matches_with_numeric(
                        wme.id,
                        wme.attr,
                        wme.val,
                        &self.rete.numeric_values,
                        pattern.relation,
                        &[],
                    )
            })
        })
    }

    fn refresh_negative_instantiations(&mut self) {
        let instantiations = self.rete.active_instantiations.clone();
        for instantiation in instantiations {
            let matches = self.negative_patterns_match(
                instantiation.rule_name,
                &instantiation.matched_wmes,
            );
            let blocked = self.negative_blocked.contains(&instantiation.id);
            if matches && !blocked {
                self.negative_blocked.push(instantiation.id);
                let supporting_wmes: Vec<WmeKey> = instantiation
                    .matched_wmes
                    .iter()
                    .map(|wme| wme.key)
                    .collect();
                let derived_wmes = self
                    .tms
                    .retract_justification(instantiation.rule_name, &supporting_wmes);
                for derived in derived_wmes {
                    self.remove_wme(derived);
                }
            } else if !matches && blocked {
                self.negative_blocked
                    .retain(|id| *id != instantiation.id);
                if !self
                    .rete
                    .activations
                    .iter()
                    .any(|activation| activation.id == instantiation.id)
                {
                    self.rete.activations.push(instantiation);
                }
            }
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
        self.insert_wme_with_support(s, a, v, SupportType::ISupport)
    }

    /// Inserts a WME with an explicit architectural support classification.
    pub fn insert_wme_with_support(
        &mut self,
        s: SymbolId,
        a: SymbolId,
        v: SymbolId,
        support: SupportType,
    ) -> WmeKey {
        if let Some(existing) = self.wm.find(s, a, v) {
            if support == SupportType::OSupport {
                self.wm.set_support(existing, SupportType::OSupport);
            }
            return existing;
        }

        let key = self.wm.insert(s, a, v, support);
        for symbol in [s, a, v] {
            if let Some(SymbolData::String(value)) = self.symbols.resolve(symbol) {
                if let Ok(number) = value.parse::<f64>() {
                    self.rete.register_numeric_value(symbol, number);
                }
            }
        }
        self.rete.add_wme(key, s, a, v);
        self.refresh_negative_instantiations();
        key
    }

    /// Synchronizes Working Memory retractions and triggers cascading TMS retractions for unsupported I-supported WMEs.
    pub fn remove_wme(&mut self, key: WmeKey) {
        let impacted_substates: Vec<SymbolId> = self
            .goal_dependencies
            .iter()
            .filter(|(_, dependencies)| dependencies.contains(&key))
            .map(|(substate, _)| *substate)
            .collect();
        for substate_id in impacted_substates {
            self.remove_substate(substate_id);
        }
        self.wm.remove(key);
        self.rete.remove_wme(key);

        let dependent_wmes = self.tms.process_retraction(key);
        for dep_key in dependent_wmes {
            if self
                .wm
                .get(dep_key)
                .is_some_and(|wme| wme.support == SupportType::ISupport)
            {
                self.wm.remove(dep_key);
                self.rete.remove_wme(dep_key);
            }
        }
        self.refresh_negative_instantiations();
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
        self.run_phase(Phase::Proposal, SupportType::ISupport)
    }

    fn run_phase(&mut self, phase: Phase, support: SupportType) -> usize {
        if self.halted {
            return 0;
        }
        self.current_phase = phase;
        self.interrupted = false;
        let mut total_fires = 0;
        let mut cycles = 0;

        while cycles < MAX_ELABORATION_CYCLES {
            cycles += 1;
            let activations = core::mem::take(&mut self.rete.activations);
            if activations.is_empty() {
                break;
            }
            let mut activations = activations;
            activations.sort_by_key(|instantiation| self.substate_rank(instantiation));

            total_fires += activations.len();

            for inst in activations {
                if self.negative_patterns_match(inst.rule_name, &inst.matched_wmes) {
                    if !self.negative_blocked.contains(&inst.id) {
                        self.negative_blocked.push(inst.id);
                    }
                    continue;
                }
                if let Some((_, actions)) =
                    self.rule_actions.iter().find(|(name, _)| *name == inst.rule_name)
                {
                    let actions_to_run = actions.clone();
                    let mut derived_wmes = Vec::new();

                    for action in actions_to_run {
                        match &action {
                            Action::Add { id, attr, val } => {
                                let wme_key =
                                    self.insert_wme_with_support(*id, *attr, *val, support);
                                derived_wmes.push(wme_key);
                            }
                            Action::AddWme { id, attr, val } => {
                                let (Some(id), Some(val)) = (
                                    Self::resolve_wme_term(id, &inst.matched_wmes),
                                    Self::resolve_wme_term(val, &inst.matched_wmes),
                                ) else {
                                    continue;
                                };
                                let wme_key =
                                    self.insert_wme_with_support(id, *attr, val, support);
                                derived_wmes.push(wme_key);
                            }
                            Action::Remove(key) => {
                                self.remove_wme(*key);
                            }
                            Action::RemoveWme { id, attr, val } => {
                                let (Some(id), Some(val)) = (
                                    Self::resolve_wme_term(id, &inst.matched_wmes),
                                    Self::resolve_wme_term(val, &inst.matched_wmes),
                                ) else {
                                    continue;
                                };
                                if let Some(key) = self.wm.find(id, *attr, val) {
                                    self.remove_wme(key);
                                }
                            }
                            Action::Prefer(pref) => {
                                self.preferences.push(pref.clone());

                                if let Some(substate) = self.substates.last() {
                                    if pref.state == substate.superstate_id {
                                        self.learn_chunk_for_result(
                                            substate.superstate_id,
                                            Action::Prefer(pref.clone()),
                                        );
                                    }
                                }
                            }
                            Action::Halt => {
                                self.halted = true;
                            }
                            Action::Interrupt => {
                                self.interrupted = true;
                            }
                            Action::Write(text) => {
                                self.output.push(text.clone());
                            }
                        }
                        if self.halted || self.interrupted {
                            break;
                        }
                    }

                    if support == SupportType::ISupport && !derived_wmes.is_empty() {
                        let supporting_keys: Vec<WmeKey> =
                            inst.matched_wmes.iter().map(|w| w.key).collect();

                        self.tms.add_justification(
                            inst.rule_name,
                            supporting_keys,
                            derived_wmes,
                        );
                    } else if support == SupportType::OSupport && !derived_wmes.is_empty() {
                        self.record_goal_dependencies(&inst.matched_wmes);
                    }
                }
                if self.halted || self.interrupted {
                    break;
                }
            }
            if self.halted || self.interrupted {
                break;
            }
        }

        total_fires
    }

    fn substate_rank(&self, instantiation: &crate::rete::Instantiation) -> usize {
        self.substates
            .iter()
            .position(|substate| {
                instantiation
                    .matched_wmes
                    .iter()
                    .any(|wme| wme.triple.0 == substate.substate_id)
            })
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    fn record_goal_dependencies(&mut self, matched_wmes: &[crate::rete::WmeRecord]) {
        let Some(substate) = self.substates.last() else {
            return;
        };
        let dependencies: Vec<WmeKey> = matched_wmes
            .iter()
            .filter(|wme| wme.triple.0 != substate.substate_id)
            .map(|wme| wme.key)
            .collect();
        if let Some((_, existing)) = self
            .goal_dependencies
            .iter_mut()
            .find(|(substate_id, _)| *substate_id == substate.substate_id)
        {
            for dependency in dependencies {
                if !existing.contains(&dependency) {
                    existing.push(dependency);
                }
            }
        } else {
            self.goal_dependencies
                .push((substate.substate_id, dependencies));
        }
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
        let previous_operator = self.selected_operator;

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

        self.sync_selected_operator_wme(state_id, previous_operator);

        result
    }

    fn sync_selected_operator_wme(
        &mut self,
        state_id: SymbolId,
        previous_operator: Option<SymbolId>,
    ) {
        let operator_attr = self.symbols.intern_str("operator");
        if previous_operator != self.selected_operator {
            if let Some(previous_operator) = previous_operator {
                if let Some(key) = self.wm.find(state_id, operator_attr, previous_operator) {
                    self.remove_wme(key);
                }
            }
        }

        if let Some(operator) = self.selected_operator {
            self.insert_wme_with_support(
                state_id,
                operator_attr,
                operator,
                SupportType::OSupport,
            );
        }
    }

    /// Purges a substate and all associated Working Memory elements from the agent.
    pub fn purge_substates_for_superstate(&mut self, superstate_id: SymbolId) {
        let mut to_remove = Vec::new();

        for (idx, record) in self.substates.iter().enumerate() {
            if record.superstate_id == superstate_id {
                to_remove.push((idx, record.substate_id));
            }
        }

        for (_, substate_id) in to_remove.into_iter().rev() {
            self.remove_substate(substate_id);
        }
    }

    fn remove_substate(&mut self, substate_id: SymbolId) {
        self.goal_dependencies
            .retain(|(id, _)| *id != substate_id);
        let wme_keys = self.wm.wmes_by_id(substate_id);
        for key in wme_keys {
            self.wm.remove(key);
            self.rete.remove_wme(key);
        }
        if let Some(index) = self
            .substates
            .iter()
            .position(|record| record.substate_id == substate_id)
        {
            self.substates.remove(index);
        }
    }

    /// Runs Application Phase rules matching the currently selected operator until quiescence.
    pub fn run_application_phase(&mut self) -> usize {
        if self.selected_operator.is_none() {
            return 0;
        }

        self.run_phase(Phase::Application, SupportType::OSupport)
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
        let attr_choices = self.symbols.intern_str("choices");
        let attr_attribute = self.symbols.intern_str("attribute");

        let val_state = self.symbols.intern_str("state");
        let val_multiple = self.symbols.intern_str("multiple");
        let val_none = self.symbols.intern_str("none");
        let val_operator = self.symbols.intern_str("operator");
        let val_type_str = match impasse_type {
            ImpasseType::Tie => self.symbols.intern_str("tie"),
            ImpasseType::Conflict => self.symbols.intern_str("conflict"),
            ImpasseType::NoChange => self.symbols.intern_str("no-change"),
        };

        self.insert_wme(substate_id, attr_superstate, superstate_id);
        self.insert_wme(substate_id, attr_type, val_state);
        self.insert_wme(substate_id, attr_impasse, val_type_str);
        self.insert_wme(
            substate_id,
            attr_choices,
            if candidates.is_empty() { val_none } else { val_multiple },
        );
        self.insert_wme(substate_id, attr_attribute, val_operator);

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