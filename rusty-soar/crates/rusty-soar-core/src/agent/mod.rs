use alloc::{boxed::Box, format, vec::Vec};
use crate::epmem::{EpisodeId, EpisodicMemory};
use crate::impasse::SubstateRecord;
use crate::learning::ChunkBuilder;
use crate::preference::{DecisionResult, Exploration, Preference, PreferenceType};
use crate::production::compile::{self, CompileError, CompiledAction, RhsTerm};
use crate::rete::{AlphaTest, Condition, FlatCondition, Instantiation, ReteNetwork, VariableBinding};
use crate::soar_parser::{SoarPreference, SoarProduction, SoarScript, SoarSupport};
use crate::smem::{LtiId, SemanticMemory};
use crate::pool::{HeapSlots, Layout};
use crate::symbol::{SymbolId, SymbolTable};
use crate::tms::TruthMaintenanceSystem;
use crate::wm::{SupportType, WmeArena, WmeKey};
use alloc::string::String;

mod decide;
mod links;
mod io;
mod rhs;
mod rl;
mod rules;

pub use rules::{FlatAction, FlatTerm};
use rules::ActionKind;
mod trace;

pub use io::{IoWme, OutputEvent, OutputMode};

pub use trace::symbol_to_string;

pub use decide::PreferenceRecord;

/// Upstream's default `max-elaborations`.
pub const DEFAULT_MAX_ELABORATIONS: usize = 100;

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
    /// Makes an acceptable preference for the WME `(id ^attr val)`.
    Add {
        /// Identifier symbol.
        id: SymbolId,
        /// Attribute symbol.
        attr: SymbolId,
        /// Value symbol.
        val: SymbolId,
    },
    /// Removes a WME immediately (not a Soar action; kept for hand-built rules).
    Remove(WmeKey),
    /// Makes an operator preference.
    Prefer(Preference),
    /// A compiled `sp` action, evaluated against the match that fired it.
    Rhs(CompiledAction),
}

/// Sizes of an agent's fixed-capacity stores, set when it is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCapacity {
    /// The Rete's pools; [`crate::rete::ReteCapacity::productions`] also bounds the agent's
    /// own production records.
    pub rete: crate::rete::ReteCapacity,
    /// Working memory elements.
    pub wmes: usize,
    /// Actions across all productions.
    pub actions: usize,
    /// New-identifier letters across all productions.
    pub new_identifier_letters: usize,
}

impl Default for AgentCapacity {
    fn default() -> Self {
        Self {
            rete: crate::rete::ReteCapacity::default(),
            wmes: crate::wm::DEFAULT_WME_CAPACITY,
            actions: 8192,
            new_identifier_letters: 4096,
        }
    }
}

impl AgentCapacity {
    /// The smallest stores: four of everything, as [`crate::rete::ReteCapacity::four`].
    pub const fn four() -> Self {
        Self { rete: crate::rete::ReteCapacity::four(), wmes: 4, actions: 4, new_identifier_letters: 4 }
    }
}

/// The main Soar Cognitive Agent instance coordinating memory, RETE, decision processing, and learning.
///
/// `S` chooses where the Rete's pools and the agent's rule store live, as for
/// [`ReteNetwork`]: on the heap (the default), or inline for Kani proofs.
pub struct SoarAgent<S: Layout = HeapSlots> {
    /// Global symbol interning table.
    pub symbols: SymbolTable,
    /// Working Memory arena storing current state assertions.
    pub wm: WmeArena,
    /// Index-based RETE match engine.
    pub rete: ReteNetwork<S>,
    /// Long-term declarative Semantic Memory store.
    pub smem: SemanticMemory,
    /// Long-term autobiographical Episodic Memory store.
    pub epmem: EpisodicMemory,
    /// Standalone truth-maintenance tracker; the agent's own i-support comes from preferences.
    pub tms: TruthMaintenanceSystem,
    /// Q-table reinforcement learner; not consulted by the decision procedure.
    pub rl: crate::rl::Rl,
    /// Active phase of the Soar decision cycle.
    pub current_phase: Phase,
    /// Preference memory (upstream's slot preference lists), in assertion order.
    pub preferences: Vec<PreferenceRecord>,
    /// Operator currently selected for the top state.
    pub selected_operator: Option<SymbolId>,
    /// Substates on the goal stack below the top state, top-down.
    pub substates: Vec<SubstateRecord>,
    /// Number of substates created so far.
    pub state_counter: usize,
    /// Monotonically increasing decision cycle counter.
    pub cycle_counter: usize,
    /// Explanation-based learning chunk builder (learning is off, as upstream's default).
    pub chunk_builder: ChunkBuilder,
    /// Count of automatically synthesized chunk rules.
    pub chunks_learned: usize,
    rules: rules::Rules<S>,
    /// Text written by RHS functions (`write`, `crlf`) and warnings.
    pub output: String,
    /// Set by the `halt` RHS function.
    pub halted: bool,
    /// Writes upstream's context-decision trace (`watch 1`) to `output` after each decide phase.
    pub trace_decisions: bool,
    io: io::Io,
    /// Upstream `mcs_counter`, numbering the symbols `make-constant-symbol` creates.
    mcs_counter: u64,
    /// Exploration policy and random state for indifferent selection.
    pub exploration: Exploration,
    /// Elaboration cycles allowed per phase (upstream `max-elaborations`).
    pub max_elaborations: usize,
    decider: decide::Decider,
}

/// Alias for `SoarAgent` to satisfy public re-exports.
pub type Agent = SoarAgent;

impl SoarAgent {
    /// Creates a new `SoarAgent` instance with initialized memory components.
    pub fn new() -> Self {
        Self::with_capacity(AgentCapacity::default())
    }
}

impl<S: Layout> SoarAgent<S> {
    /// Creates an agent whose stores have the given fixed sizes.
    pub fn with_capacity(capacity: AgentCapacity) -> Self {
        Self {
            symbols: SymbolTable::new(),
            wm: WmeArena::with_capacity(capacity.wmes),
            rete: ReteNetwork::with_capacity(capacity.rete),
            smem: SemanticMemory::new(),
            epmem: EpisodicMemory::new(),
            tms: TruthMaintenanceSystem::new(),
            rl: crate::rl::Rl::default(),
            current_phase: Phase::Input,
            preferences: Vec::new(),
            selected_operator: None,
            substates: Vec::new(),
            state_counter: 1,
            cycle_counter: 0,
            chunk_builder: ChunkBuilder::new(),
            chunks_learned: 0,
            rules: rules::Rules::with_capacity(capacity.rete.productions, capacity.actions, capacity.new_identifier_letters),
            output: String::new(),
            halted: false,
            trace_decisions: false,
            io: io::Io::default(),
            mcs_counter: 1,
            exploration: Exploration::default(),
            max_elaborations: DEFAULT_MAX_ELABORATIONS,
            decider: decide::Decider::default(),
        }
    }

    /// Registers a production rule with its RETE conditions and RHS execution actions.
    pub fn add_rule(
        &mut self,
        name: &'static str,
        conditions: Vec<(AlphaTest, Vec<VariableBinding>)>,
        actions: Vec<Action>,
    ) {
        let result = self.rete.add_rule(name, conditions, &self.symbols);
        self.record_rule(result, name, &actions, &[], SoarSupport::Undeclared);
    }

    /// Registers a production whose conditions use the full Rete condition language
    /// (negations, conjunctive negations, relational and disjunction tests).
    pub fn add_production(&mut self, name: &'static str, conditions: Vec<Condition>, actions: Vec<Action>) {
        let result = self.rete.add_production(name, conditions, &self.symbols);
        self.record_rule(result, name, &actions, &[], SoarSupport::Undeclared);
    }

    /// [`Self::add_production`] for a flattened condition list and actions without function
    /// calls, with the letters of the new identifiers its [`FlatTerm::NewIdentifier`]s create.
    pub fn add_production_flat(
        &mut self,
        name: &'static str,
        conditions: &[FlatCondition],
        actions: &[FlatAction],
        new_identifier_letters: &[char],
    ) {
        let result = self.rete.add_production_flat(name, conditions, &self.symbols);
        self.record_rule(result, name, actions, new_identifier_letters, SoarSupport::Undeclared);
    }

    /// Records a production the Rete has added. Its matches are only queued until the next
    /// firing, so recording it after adding it is in time.
    fn record_rule(
        &mut self,
        added: Result<crate::rete::ProductionId, crate::rete::CapacityError>,
        name: &'static str,
        actions: &[impl rules::ToStored],
        letters: &[char],
        support: SoarSupport,
    ) {
        let production = match added {
            Ok(production) => production,
            Err(error) => return self.check_capacity(Err(error)),
        };
        if let Err(full) = self.rules.add(production, name, actions, letters, support) {
            self.halt_on_full(full);
        }
    }

    /// Compiles and installs every production in a parsed script, using `state_id` as the
    /// top state. Like upstream `sp`, a production that fails to compile is reported and not
    /// added; the others still are.
    pub fn install_soar_script(&mut self, script: &SoarScript, state_id: SymbolId) -> Result<(), CompileError> {
        self.ensure_top_goal(state_id);
        let mut first_error = None;
        let mut commands = script.commands.iter().peekable();
        for (index, production) in script.productions.iter().enumerate() {
            while let Some((_, command)) = commands.next_if(|(before, _)| *before <= index) {
                self.run_script_command(command);
            }
            if let Err(error) = self.install_production(production) {
                first_error.get_or_insert(error);
            }
        }
        for (_, command) in commands {
            self.run_script_command(command);
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Carries out a script's settings command.
    pub fn run_script_command(&mut self, command: &crate::soar_parser::ScriptCommand) {
        use crate::soar_parser::ScriptCommand as C;
        match command {
            C::Rl(setting) => self.set_rl(*setting),
            C::ExplorationPolicy(policy) => self.exploration.policy = *policy,
            C::Epsilon(x) => self.exploration.epsilon = *x,
            C::Temperature(x) => self.exploration.temperature = *x,
            C::NumericIndifferentMode(mode) => self.exploration.numeric_indifferent_mode = *mode,
            C::SetRandomSeed(seed) => self.exploration.rng.seed(*seed),
            C::Trace(level) => self.trace_decisions = *level >= 1,
            C::Echo { text, newline } => {
                self.output.push_str(text);
                if *newline {
                    self.output.push('\n');
                }
            }
        }
    }

    /// Compiles and installs one parsed production.
    pub fn install_production(&mut self, production: &SoarProduction) -> Result<(), CompileError> {
        if self.rules.contains_name(&production.name) {
            return Err(CompileError(format!("production {} already exists", production.name)));
        }
        // Flags that change how a rule runs, which this port does not support yet.
        if production.production_type == crate::soar_parser::SoarProductionType::Template {
            return Err(CompileError(format!("production {}: :template (RL template) rules are not supported", production.name)));
        }
        if production.interrupt {
            return Err(CompileError(format!("production {}: :interrupt is not supported", production.name)));
        }
        let compiled = compile::compile(production, &mut self.symbols)?;
        for action in &compiled.actions {
            match action {
                CompiledAction::Make { id, attr, value, referent, .. } => {
                    for term in [Some(id), Some(attr), Some(value), referent.as_ref()].into_iter().flatten() {
                        rhs::check_term(term, true).map_err(|e| CompileError(format!("production {}: {}", compiled.name, e)))?;
                    }
                }
                CompiledAction::Call(term) => {
                    rhs::check_term(term, false).map_err(|e| CompileError(format!("production {}: {}", compiled.name, e)))?;
                }
            }
        }
        for warning in &compiled.warnings {
            self.output.push_str(warning);
            self.output.push('\n');
        }
        let name: &'static str = Box::leak(compiled.name.into_boxed_str());
        self.register_rl_rule(name, &compiled.actions);
        let actions: Vec<Action> = compiled.actions.into_iter().map(Action::Rhs).collect();
        let result = self.rete.add_production(name, compiled.conditions, &self.symbols);
        self.record_rule(result, name, &actions, &compiled.new_identifier_letters, production.support);
        Ok(())
    }

    /// Adds a WME from outside the rules (input, or test setup). Such WMEs are not in any
    /// slot and stay until removed with [`Self::remove_wme`].
    pub fn insert_wme(&mut self, s: SymbolId, a: SymbolId, v: SymbolId) -> WmeKey {
        self.insert_wme_with_flag(s, a, v, false, SupportType::OSupport)
    }

    fn insert_wme_with_flag(&mut self, s: SymbolId, a: SymbolId, v: SymbolId, acceptable: bool, support: SupportType) -> WmeKey {
        if let Some(existing) = self.wm.find_with_flag(s, a, v, acceptable) {
            return existing;
        }
        self.add_wme_to_wm(s, a, v, acceptable, support)
    }

    /// Upstream `add_wme_to_wm`: adds a WME even when one with the same triple exists.
    fn add_wme_to_wm(&mut self, s: SymbolId, a: SymbolId, v: SymbolId, acceptable: bool, support: SupportType) -> WmeKey {
        let Some(key) = self.wm.insert_distinct(s, a, v, support, acceptable) else {
            self.check_capacity(Err(crate::rete::CapacityError::Wmes));
            // A key that refers to no WME; the agent has halted.
            return WmeKey(usize::MAX);
        };
        let result = self.rete.add_wme(key, (s, a, v), acceptable, &self.symbols);
        self.check_capacity(result);
        self.post_wme_links(s, v, true);
        self.inform_output_module(io::IoWme { key, id: s, attr: a, value: v }, true);
        key
    }

    /// Removes a WME from working memory and the Rete. If the WME is in a substate's goal
    /// dependency set, that substate (and those below it) is removed first, as upstream does
    /// for input and slot WMEs.
    pub fn remove_wme(&mut self, key: WmeKey) {
        self.gds_invalid_so_remove_goal(key);
        self.remove_wme_quietly(key);
    }

    /// Removes a WME without checking goal dependency sets (architectural WMEs).
    pub(super) fn remove_wme_quietly(&mut self, key: WmeKey) {
        let Some(w) = self.wm.remove(key) else { return };
        self.rete.remove_wme(key, &self.symbols);
        self.decider.forget_wme(key);
        self.post_wme_links(w.id, w.val, false);
        self.inform_output_module(io::IoWme { key, id: w.id, attr: w.attr, value: w.val }, false);
    }

    /// Asserts an operator preference that no rule supports (it persists like an
    /// o-supported preference), as when a test sets up a slot by hand.
    pub fn add_preference(&mut self, preference: Preference) {
        self.ensure_top_goal(preference.state);
        let attr = self.symbols.predefined.operator;
        self.assert_preferences(alloc::vec![PreferenceRecord::architectural(&preference, attr, self.goal_of(preference.state))]);
    }

    /// Creates the top state as upstream `create_top_goal` and io initialization do:
    /// `(S1 ^type state ^superstate nil ^reward-link R1 ^epmem E1 ^smem L1 ^io I1)` with the
    /// epmem/smem link structures and `(I1 ^input-link I2 ^output-link I3)`.
    /// Returns the state and its input link.
    pub fn create_top_state(&mut self) -> (SymbolId, SymbolId) {
        let state = self.create_new_context(None, None);
        let input_link = self.create_io_links(state);
        self.do_buffered_link_changes();
        self.print_decision_trace();
        (state, input_link)
    }

    /// A full fixed-capacity pool stops the agent: it halts with an error rather than run on
    /// with an incomplete match network.
    fn check_capacity(&mut self, result: Result<(), crate::rete::CapacityError>) {
        if let Err(error) = result {
            if !self.halted {
                self.report_full("Rete", error.name());
            }
            self.halted = true;
        }
    }

    /// As [`Self::check_capacity`], for the agent's own stores.
    fn halt_on_full(&mut self, store: rules::Full) {
        if !self.halted {
            self.report_full("agent", store.name());
        }
        self.halted = true;
    }

    /// Reports a full store. Built from pieces rather than with `format!`, whose machinery Kani
    /// would otherwise unroll on every path where a store might be full.
    fn report_full(&mut self, owner: &str, store: &str) {
        for piece in ["Error: ", owner, " capacity exceeded (", store, "); agent halted.\n"] {
            self.output.push_str(piece);
        }
    }

    /// Creates a new identifier at a goal level (upstream `make_new_identifier(letter, level)`).
    pub(super) fn new_identifier(&mut self, letter: char, level: usize) -> SymbolId {
        let id = self.symbols.make_new_identifier(letter);
        self.decider.links.set_new_id_level(id, level);
        id
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

    /// Runs the propose phase (upstream `PROPOSE_PHASE`): i-supported rules and retractions
    /// fire in elaboration waves until quiescence or `max_elaborations`. Returns the number of
    /// rule firings.
    pub fn run_elaboration_phase(&mut self) -> usize {
        self.current_phase = Phase::Proposal;
        self.run_elaboration_waves(false)
    }

    /// Runs the decide phase (upstream `do_decide_phase`) for the goal stack whose top state is
    /// `state_id`, returning the decision made for the slot that was decided.
    pub fn run_decision_phase(&mut self, state_id: SymbolId) -> DecisionResult {
        self.ensure_top_goal(state_id);
        self.current_phase = Phase::Decision;
        let result = self.decide_context_slots();
        self.do_working_memory_phase();
        self.print_decision_trace();
        self.decider.e_cycles_this_d_cycle = 0;
        result
    }

    /// Runs the apply phase (upstream `APPLY_PHASE`): all rules fire in waterfall order until
    /// quiescence or `max_elaborations`. Returns the number of rule firings.
    pub fn run_application_phase(&mut self) -> usize {
        self.current_phase = Phase::Application;
        self.run_elaboration_waves(true)
    }

    /// Runs one decision cycle: input, propose, decide, apply, output.
    ///
    /// As upstream, a halt ends the cycle at the end of the phase it happens in, and a halted
    /// agent does not run again: this returns [`DecisionResult::Halted`] with upstream's message.
    pub fn run_decision_cycle(&mut self, state_id: SymbolId) -> DecisionResult {
        if self.halted {
            self.output.push_str("\nSystem halted.  Use (init-soar) before running Soar again.");
            return DecisionResult::Halted;
        }
        self.ensure_top_goal(state_id);
        self.cycle_counter += 1;

        self.current_phase = Phase::Input;
        self.decider.e_cycles_this_d_cycle = 0;
        self.do_input_cycle();

        self.run_elaboration_phase();
        if self.halted {
            return DecisionResult::Halted;
        }
        let decision = self.run_decision_phase(state_id);
        self.run_application_phase();
        if self.halted {
            return decision;
        }

        self.current_phase = Phase::Output;
        self.do_output_cycle();
        self.record_episode();

        decision
    }

    /// Removes every substate below the goal `superstate_id`.
    pub fn purge_substates_for_superstate(&mut self, superstate_id: SymbolId) {
        if let Some(level) = self.goal_level(superstate_id) {
            if level < self.decider.goals.len() {
                self.remove_existing_context_and_descendents(level);
            }
        }
    }

    /// The operator selected for a goal, if any.
    pub fn goal_operator(&self, goal: SymbolId) -> Option<SymbolId> {
        self.decider.goals.iter().find(|g| g.id == goal).and_then(|g| g.operator.map(|(op, _)| op))
    }

    /// The goal stack, top state first.
    pub fn goal_stack(&self) -> Vec<SymbolId> {
        self.decider.goals.iter().map(|g| g.id).collect()
    }

    // ------------------------------------------------------------------
    // Firing
    // ------------------------------------------------------------------

    /// Creates the instantiation for a match: allocates its new identifiers and evaluates its
    /// actions into preferences (upstream `create_instantiation`). Function calls such as
    /// `write` run now; preferences are asserted later in the wave.
    fn create_instantiation(&mut self, inst: &Instantiation) -> Vec<PreferenceRecord> {
        let match_goal = self.match_goal(&inst.matched_wmes);
        // New identifiers start at the match goal's level (upstream `new_id_level`).
        let new_id_level = match_goal.and_then(|g| self.goal_level(g)).map_or(links::LOWEST_POSSIBLE_GOAL_LEVEL, |g| g + 1);
        let letters = self.rules.letters(inst.production);
        let new_ids: Vec<SymbolId> = letters.into_iter().map(|letter| self.new_identifier(letter, new_id_level)).collect();
        let o_supported = self.calculate_support(inst.production, &inst.matched_wmes, &inst.levels, &new_ids, true);
        let operator = self.symbols.predefined.operator;

        let mut made = Vec::new();
        for index in 0..self.rules.action_count(inst.production) {
            let action = self.rules.action(inst.production, index);
            let made_pref = match action.kind {
                // An `Add` action's terms are constants.
                ActionKind::Add => match action.terms.map(|t| flat_value(t, &[], &[])) {
                    [Some(id), Some(attr), Some(val)] => Some((id, attr, val, PreferenceType::Acceptable)),
                    _ => None,
                },
                ActionKind::Prefer => Some((action.prefer.state, operator, action.prefer.operator, action.prefer.preference_type)),
                ActionKind::Remove => {
                    self.remove_wme(action.wme);
                    None
                }
                ActionKind::Make => self.execute_make(inst, action, &new_ids, operator),
                ActionKind::Complex => {
                    let compiled = self.rules.complex(action.index).clone();
                    self.execute_compiled(inst, &compiled, &new_ids, operator)
                }
            };
            if let Some((id, attr, value, preference_type)) = made_pref {
                made.push(PreferenceRecord {
                    id,
                    attr,
                    value,
                    preference_type,
                    o_supported,
                    instantiation: Some(inst.id),
                    production: Some(inst.rule_name),
                    match_goal,
                    justification: None,
                    seq: 0,
                });
            }
        }
        made
    }

    fn resolve_rhs(&mut self, term: &RhsTerm, levels: &[Option<(SymbolId, SymbolId, SymbolId)>], new_ids: &[SymbolId]) -> Option<SymbolId> {
        match term {
            RhsTerm::Constant(symbol) => Some(*symbol),
            RhsTerm::Bound(location) => flat_value(FlatTerm::Bound(*location), levels, new_ids),
            RhsTerm::NewIdentifier(index) => new_ids.get(*index).copied(),
            RhsTerm::Call { name, args } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.resolve_rhs(arg, levels, new_ids)?);
                }
                rhs::call(self, name, &values)
            }
        }
    }

    /// Evaluates one make action without function calls (upstream `execute_action`) into a
    /// preference.
    fn execute_make(
        &mut self,
        inst: &Instantiation,
        action: FlatAction,
        new_ids: &[SymbolId],
        operator: SymbolId,
    ) -> Option<(SymbolId, SymbolId, SymbolId, PreferenceType)> {
        let ([id, attr, value], preference, referent) = (action.terms, action.preference, action.referent());
        let id = flat_value(id, &inst.levels, new_ids)?;
        let attr = flat_value(attr, &inst.levels, new_ids)?;
        let value = flat_value(value, &inst.levels, new_ids)?;
        let referent = match referent {
            Some(term) => Some(flat_value(term, &inst.levels, new_ids)?),
            None => None,
        };
        self.make_preference(inst, [id, attr, value], preference, referent, operator)
    }

    /// Evaluates an action that calls a function (upstream `execute_action`): a call runs
    /// now, and a make action becomes a preference.
    fn execute_compiled(
        &mut self,
        inst: &Instantiation,
        action: &CompiledAction,
        new_ids: &[SymbolId],
        operator: SymbolId,
    ) -> Option<(SymbolId, SymbolId, SymbolId, PreferenceType)> {
        let (id, attr, value, preference, referent) = match action {
            CompiledAction::Call(call) => {
                self.resolve_rhs(call, &inst.levels, new_ids);
                return None;
            }
            CompiledAction::Make { id, attr, value, preference, referent } => (id, attr, value, *preference, referent),
        };
        let id = self.resolve_rhs(id, &inst.levels, new_ids)?;
        let attr = self.resolve_rhs(attr, &inst.levels, new_ids)?;
        let value = self.resolve_rhs(value, &inst.levels, new_ids)?;
        let referent = match referent {
            Some(term) => Some(self.resolve_rhs(term, &inst.levels, new_ids)?),
            None => None,
        };
        self.make_preference(inst, [id, attr, value], preference, referent, operator)
    }

    /// The preference a make action's resolved values give. Operator attributes take any
    /// preference; other attributes only acceptable or reject.
    fn make_preference(
        &mut self,
        inst: &Instantiation,
        [id, attr, value]: [SymbolId; 3],
        preference: SoarPreference,
        referent: Option<SymbolId>,
        operator: SymbolId,
    ) -> Option<(SymbolId, SymbolId, SymbolId, PreferenceType)> {
        if !self.symbols.is_identifier(id) {
            self.output.push_str(&format!("Error: RHS makes a preference for non-identifier in {}\n", inst.rule_name));
            return None;
        }
        let preference_type = match preference {
            SoarPreference::Acceptable => PreferenceType::Acceptable,
            SoarPreference::Reject => PreferenceType::Reject,
            _ if attr != operator => return None,
            SoarPreference::Require => PreferenceType::Require,
            SoarPreference::Prohibit => PreferenceType::Prohibit,
            SoarPreference::Better => PreferenceType::Better(referent?),
            SoarPreference::Worse => PreferenceType::Worse(referent?),
            SoarPreference::Best => PreferenceType::Best,
            SoarPreference::Worst => PreferenceType::Worst,
            SoarPreference::UnaryIndifferent => PreferenceType::Indifferent,
            SoarPreference::BinaryIndifferent => PreferenceType::BinaryIndifferent(referent?),
            // An RL rule fires with its learned value (upstream replaces the action's referent).
            SoarPreference::NumericIndifferent => {
                PreferenceType::NumericIndifferent(self.rl_rule_value(inst.rule_name).or(referent)?)
            }
        };
        Some((id, attr, value, preference_type))
    }

    /// Upstream `calculate_support_for_instantiation_preferences`: declared support wins; an
    /// operator proposal is i-supported; otherwise a match that tests the selected operator of
    /// its lowest goal is o-supported, unless its actions only elaborate that operator.
    ///
    /// Also used at match time to sort matches into i- and o-assertions (upstream
    /// `p_node_left_addition`), with `warn` off and no new identifiers yet.
    pub(crate) fn calculate_support(
        &mut self,
        production: crate::rete::ProductionId,
        matched_wmes: &[crate::rete::WmeRecord],
        levels: &[Option<(SymbolId, SymbolId, SymbolId)>],
        new_ids: &[SymbolId],
        warn: bool,
    ) -> bool {
        match self.rules.support(production) {
            SoarSupport::OSupport => return true,
            SoarSupport::ISupport => return false,
            SoarSupport::Undeclared => {}
        }
        let rule_name = self.rules.name(production);
        let operator = self.symbols.predefined.operator;

        // Ids of the make actions, where they can be known without firing.
        let mut action_ids: Vec<(Option<SymbolId>, Option<SymbolId>, bool)> = Vec::new(); // (id, attr, acceptable)
        for index in 0..self.rules.action_count(production) {
            let action = self.rules.action(production, index);
            match action.kind {
                ActionKind::Add => action_ids.push((flat_value(action.terms[0], &[], &[]), flat_value(action.terms[1], &[], &[]), true)),
                ActionKind::Prefer => action_ids.push((
                    Some(action.prefer.state),
                    Some(operator),
                    action.prefer.preference_type == PreferenceType::Acceptable,
                )),
                ActionKind::Remove => {}
                ActionKind::Make => action_ids.push((
                    flat_value(action.terms[0], levels, new_ids),
                    flat_value(action.terms[1], levels, new_ids),
                    action.preference == SoarPreference::Acceptable,
                )),
                ActionKind::Complex => {
                    if let CompiledAction::Make { id, attr, preference, .. } = self.rules.complex(action.index) {
                        let static_term = |t: &RhsTerm| match t {
                            RhsTerm::Constant(s) => Some(*s),
                            RhsTerm::Bound(loc) => flat_value(FlatTerm::Bound(*loc), levels, new_ids),
                            RhsTerm::NewIdentifier(i) => new_ids.get(*i).copied(),
                            RhsTerm::Call { .. } => None,
                        };
                        action_ids.push((static_term(id), static_term(attr), *preference == SoarPreference::Acceptable));
                    }
                }
            }
        }

        let is_proposal = action_ids
            .iter()
            .any(|(id, attr, acceptable)| *acceptable && *attr == Some(operator) && id.is_some_and(|id| self.symbols.is_goal(id)));
        if is_proposal {
            return false;
        }

        let Some(lowest_goal) = self.match_goal(matched_wmes) else { return false };
        let mut o_support = false;
        let mut op_elab = false;
        for w in matched_wmes {
            let (id, attr, value) = w.triple;
            let acceptable = self.wm.get(w.key).is_some_and(|wme| wme.acceptable);
            if attr == operator && !acceptable && id == lowest_goal {
                for (action_id, _, _) in &action_ids {
                    if *action_id == Some(value) {
                        op_elab = true;
                    } else {
                        o_support = true;
                    }
                }
            }
        }
        if o_support && op_elab {
            if warn {
                self.output.push_str(&format!(
                    "WARNING:  Operator elaborations mixed with operator applications\nAssigning i_support to prod {}\n",
                    rule_name
                ));
            }
            return false;
        }
        o_support
    }
}

impl Default for SoarAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// The value of a RHS term without function calls, given the match's WME at each condition
/// level and the firing's new identifiers.
fn flat_value(term: FlatTerm, levels: &[Option<(SymbolId, SymbolId, SymbolId)>], new_ids: &[SymbolId]) -> Option<SymbolId> {
    match term {
        FlatTerm::Constant(symbol) => Some(symbol),
        FlatTerm::Bound(location) => {
            let triple = levels.get(location.depth.checked_sub(1)?)?.as_ref()?;
            Some(location.field.extract(triple))
        }
        FlatTerm::NewIdentifier(index) => new_ids.get(index).copied(),
    }
}
