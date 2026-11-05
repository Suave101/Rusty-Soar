#![cfg(kani)]
extern crate alloc;

use rusty_soar_core::{
    aadl_bridge::{AgreeAnnex, AgreeVal, SymbolEnvironment},
    agent::SoarAgent,
    soar_parser::SoarScript,
    symbol::{SymbolData, SymbolId},
};

/// Dynamic bridge mapping a Soar agent's Working Memory to AGREE contract specifications.
pub struct DynamicWmeEnv<'a> {
    pub agent: &'a SoarAgent,
    pub state_id: SymbolId,
}

impl<'a> SymbolEnvironment for DynamicWmeEnv<'a> {
    fn lookup(&self, var_name: &str) -> Option<AgreeVal> {
        // 1. Search WMEs attached directly to the current state identifier (<s> ^var_name val)
        for wme in self.agent.wm.iter() {
            if wme.id == self.state_id {
                if let Some(SymbolData::String(attr_str)) = self.agent.symbols.resolve(wme.attr) {
                    if attr_str == var_name {
                        return resolve_symbol_to_agree_val(self.agent, wme.val);
                    }
                }
            }
        }

        // 2. Search WMEs attached to the currently selected operator (<o> ^var_name val)
        if let Some(selected_op_id) = self.agent.selected_operator {
            for wme in self.agent.wm.iter() {
                if wme.id == selected_op_id {
                    if let Some(SymbolData::String(attr_str)) = self.agent.symbols.resolve(wme.attr) {
                        if attr_str == var_name {
                            return resolve_symbol_to_agree_val(self.agent, wme.val);
                        }
                    }
                }
            }

            // Direct resolution for selected operator handle
            if var_name == "operator_id" || var_name == "operator" {
                return resolve_symbol_to_agree_val(self.agent, selected_op_id);
            }
        }

        None
    }
}

/// Helper function converting a Soar SymbolId into an AGREE contract value type.
fn resolve_symbol_to_agree_val(agent: &SoarAgent, sym_id: SymbolId) -> Option<AgreeVal> {
    match agent.symbols.resolve(sym_id) {
        Some(SymbolData::Integer(val)) => Some(AgreeVal::Int(*val)),
        Some(SymbolData::Float(val)) => Some(AgreeVal::Float(*val as f32)),
        Some(SymbolData::Identifier { number, .. }) => Some(AgreeVal::Int(*number as i64)),
        Some(SymbolData::String(s)) => match s.as_str() {
            "true" => Some(AgreeVal::Bool(true)),
            "false" => Some(AgreeVal::Bool(false)),
            str_val => {
                if let Ok(i) = str_val.parse::<i64>() {
                    Some(AgreeVal::Int(i))
                } else if let Ok(f) = str_val.parse::<f32>() {
                    Some(AgreeVal::Float(f))
                } else {
                    None
                }
            }
        },
        None => None,
    }
}

#[kani::proof]
#[kani::unwind(10)]
pub fn verify_soar_model_against_agree_contract() {
    // Step 1: Load and parse the user's Soar production script
    let soar_src = include_str!("../../../../e2e_tests/model.soar");
    let soar_script = SoarScript::parse(soar_src).expect("Failed to parse Soar model");

    // Step 2: Load and parse the user's AADL/AGREE contract specification
    let aadl_src = include_str!("../../../../e2e_tests/contract.aadl");
    let contract = AgreeAnnex::parse_aadl_file(aadl_src).expect("Failed to parse AGREE contract");

    // Initialize the domain-agnostic Soar engine
    let mut agent = SoarAgent::new();
    let state_id = agent.symbols.intern_id('S', 1);
    agent.install_soar_script(&soar_script, state_id).expect("Failed to install Soar model");

    // Step 4a: Define symbolic inputs for state variables tested in assumptions
    let sym_sensor_valid: bool = kani::any();
    let sym_engine_status: i64 = kani::any();

    // Populate Working Memory with symbolic state triples
    let attr_sensor = agent.symbols.intern_str("sensor_valid");
    let val_sensor = agent.symbols.intern_str(if sym_sensor_valid { "true" } else { "false" });
    agent.insert_wme(state_id, attr_sensor, val_sensor);

    let attr_engine = agent.symbols.intern_str("engine_status");
    let val_engine = agent.symbols.intern_int(sym_engine_status);
    agent.insert_wme(state_id, attr_engine, val_engine);

    // Step 4b: Verify AGREE Assumptions (assume valid state spaces)
    let pre_env = DynamicWmeEnv {
        agent: &agent,
        state_id,
    };
    if contract.verify_assumes(&pre_env).is_err() {
        return;
    }

    // Step 4c: Run the Soar Decision Cycle
    agent.run_decision_cycle(state_id);

    // Step 4d: Verify AGREE Guarantees post-execution
    let post_env = DynamicWmeEnv {
        agent: &agent,
        state_id,
    };
    assert!(
        contract.verify_guarantees(&post_env).is_ok(),
        "AGREE Guarantee violated by Soar model execution"
    );
}

/// `N` distinct symbols, made directly instead of interned. The Rete proofs below compare
/// symbols only for equality, which needs no table lookup, so this keeps the symbol table's
/// heap storage (which Kani tracks poorly) out of the proofs.
fn fixed_symbols<const N: usize>() -> [rusty_soar_core::symbol::SymbolId; N] {
    core::array::from_fn(|i| rusty_soar_core::symbol::SymbolId(i as u32))
}

/// For every combination of the three WMEs, including adding then removing the blocker, the
/// production `(<s> ^a <v>) -(<s> ^b <v>)` has exactly the matches its definition gives.
///
/// Run it path by path (`--cbmc-args --paths lifo`, about 6 minutes). Merged into one formula,
/// the three choices make the Rete's linked lists unboundable and the proof does not finish.
#[kani::proof]
#[kani::unwind(10)]
pub fn verify_rete_negated_condition() {
    use rusty_soar_core::rete::{
        AlphaTest, Field, FlatCondition, FlatTests, Referent, Relation, ReteNetwork, ReteTest, VarLocation,
    };
    use rusty_soar_core::wm::WmeKey;

    let symbols = rusty_soar_core::symbol::SymbolTable::new();
    let [s, a, b, v] = fixed_symbols();
    let same_id = ReteTest::Relational {
        field: Field::Id,
        relation: Relation::Equal,
        referent: Referent::Variable(VarLocation { depth: 1, field: Field::Id }),
    };

    let mut rete: ReteNetwork<rusty_soar_core::pool::InlineSlots<4>> = ReteNetwork::with_capacity(rusty_soar_core::rete::ReteCapacity::four());
    rete.add_production_flat(
        "p",
        &[
            FlatCondition::Positive(FlatTests {
                alpha: AlphaTest { attr: Some(a), ..AlphaTest::default() },
                acceptable: false,
                tests: &[],
            }),
            FlatCondition::Negative(FlatTests {
                alpha: AlphaTest { attr: Some(b), ..AlphaTest::default() },
                acceptable: false,
                tests: &[same_id],
            }),
        ],
        &symbols,
    )
    .expect("capacity");

    let has_a: bool = kani::any();
    let has_b: bool = kani::any();
    let then_remove_b: bool = kani::any();
    if has_a {
        rete.add_wme(WmeKey(0), (s, a, v), false, &symbols).expect("capacity");
    }
    if has_b {
        rete.add_wme(WmeKey(1), (s, b, v), false, &symbols).expect("capacity");
        if then_remove_b {
            rete.remove_wme(WmeKey(1), &symbols);
        }
    }

    let blocked = has_b && !then_remove_b;
    assert_eq!(rete.pending_assertions(), usize::from(has_a && !blocked));
}

/// Smallest Rete problem: one positive condition, one WME that may or may not be present.
#[kani::proof]
#[kani::unwind(10)]
pub fn verify_rete_single_condition_baseline() {
    use rusty_soar_core::rete::{AlphaTest, FlatCondition, FlatTests, ReteNetwork};
    use rusty_soar_core::wm::WmeKey;
    let symbols = rusty_soar_core::symbol::SymbolTable::new();
    let [s, a] = fixed_symbols();
    let mut rete: ReteNetwork<rusty_soar_core::pool::InlineSlots<4>> = ReteNetwork::with_capacity(rusty_soar_core::rete::ReteCapacity::four());
    rete.add_production_flat(
        "p",
        &[FlatCondition::Positive(FlatTests { alpha: AlphaTest { attr: Some(a), ..AlphaTest::default() }, acceptable: false, tests: &[] })],
        &symbols,
    )
    .expect("capacity");
    let present: bool = kani::any();
    if present {
        rete.add_wme(WmeKey(0), (s, a, s), false, &symbols).expect("capacity");
    }
    assert_eq!(rete.pending_assertions(), usize::from(present));
}

/// Stand-ins for the agent's random number generator. Each returns any value in its range, so
/// a proof using them covers every random choice the agent could make, not just one seed's.
#[allow(dead_code)] // Used only through `kani::stub`.
mod any_rand {
    use rusty_soar_core::soar_rand::MtRand;

    pub fn new(_seed: u32) -> MtRand {
        // Never read: every method that would read it is stubbed too.
        unsafe { core::mem::zeroed() }
    }

    pub fn seed(_rng: &mut MtRand, _seed: u32) {}

    pub fn rand_int(_rng: &mut MtRand) -> u32 {
        kani::any()
    }

    pub fn rand_int_max(_rng: &mut MtRand, n: u32) -> u32 {
        let i: u32 = kani::any();
        kani::assume(i <= n);
        i
    }

    pub fn rand(_rng: &mut MtRand) -> f64 {
        let x: f64 = kani::any();
        kani::assume((0.0..=1.0).contains(&x));
        x
    }

    pub fn rand_max(_rng: &mut MtRand, n: f64) -> f64 {
        rand(_rng) * n
    }
}

/// Proofs about whole decision cycles of a small agent.
///
/// Run them without assertion reachability checks (`--no-assertion-reach-checks`): Kani
/// otherwise makes one solver call per assertion, which takes about an hour here. Each proof
/// ends with a `kani::cover!` instead, which shows the end of the cycle is reached.
mod decision_cycle {
    use rusty_soar_core::agent::{AgentCapacity, FlatAction, FlatTerm, SoarAgent};
    use rusty_soar_core::pool::{InlineSlots, Layout};
    use rusty_soar_core::preference::DecisionResult;
    use rusty_soar_core::rete::{AlphaTest, Field, FlatCondition, FlatTests, ReteCapacity, VarLocation};
    use rusty_soar_core::soar_parser::SoarPreference;
    use rusty_soar_core::symbol::SymbolId;

    /// A decision-cycle proof: loops over the symbol table's predefined symbols need a bound
    /// above 35, and random choices may come out any way.
    macro_rules! agent_proof {
        ($(#[$doc:meta])* fn $name:ident() $body:block) => {
            $(#[$doc])*
            #[kani::proof]
            #[kani::unwind(64)]
            #[kani::stub(rusty_soar_core::soar_rand::MtRand::new, crate::any_rand::new)]
            #[kani::stub(rusty_soar_core::soar_rand::MtRand::seed, crate::any_rand::seed)]
            #[kani::stub(rusty_soar_core::soar_rand::MtRand::rand_int, crate::any_rand::rand_int)]
            #[kani::stub(rusty_soar_core::soar_rand::MtRand::rand_int_max, crate::any_rand::rand_int_max)]
            #[kani::stub(rusty_soar_core::soar_rand::MtRand::rand, crate::any_rand::rand)]
            #[kani::stub(rusty_soar_core::soar_rand::MtRand::rand_max, crate::any_rand::rand_max)]
            fn $name() $body
        };
    }

    /// The rule `(<s> ^attr value) --> (<s> ^operator <o> + =)`: an acceptable and, when
    /// `indifferent`, an indifferent preference for a new operator.
    fn add_proposal<S: Layout>(agent: &mut SoarAgent<S>, name: &'static str, attr: SymbolId, value: SymbolId, indifferent: bool) {
        let operator = agent.symbols.predefined.operator;
        let make = |preference| {
            FlatAction::make(
                FlatTerm::Bound(VarLocation { depth: 1, field: Field::Id }),
                FlatTerm::Constant(operator),
                FlatTerm::NewIdentifier(0),
                preference,
            )
        };
        let actions = [make(SoarPreference::Acceptable), make(SoarPreference::UnaryIndifferent)];
        agent.add_production_flat(
            name,
            &[FlatCondition::Positive(FlatTests {
                alpha: AlphaTest { attr: Some(attr), val: Some(value), ..AlphaTest::default() },
                acceptable: false,
                tests: &[],
            })],
            if indifferent { &actions } else { &actions[..1] },
            &['O'],
        );
    }

    agent_proof! {
        /// One rule proposes an operator and its WME is present: one decision cycle selects it.
        fn verify_decision_cycle_selects_proposal() {
            let mut agent: SoarAgent<InlineSlots<4>> = SoarAgent::with_capacity(AgentCapacity::four());
            let s = agent.symbols.intern_id('S', 1);
            let (attr, value) = (agent.symbols.predefined.name, agent.symbols.intern_int(1));
            add_proposal(&mut agent, "propose", attr, value, false);
            agent.insert_wme(s, attr, value);
            let decision = agent.run_decision_cycle(s);
            assert!(!agent.halted);
            assert!(matches!(decision, DecisionResult::Selected(_)));
            assert!(agent.selected_operator.is_some());
            kani::cover!(true, "the decision cycle completes");
            // Freeing the symbol table's strings is a loop Kani cannot bound, and proves nothing.
            core::mem::forget(agent);
        }
    }

    agent_proof! {
        /// Two rules each propose an indifferent operator when their own WME is present, and at
        /// least one is. However the random choice between indifferent operators comes out, one
        /// decision cycle selects an operator, and only one whose WME is present.
        fn verify_decision_cycle_indifferent_choice() {
            let capacity = AgentCapacity { rete: ReteCapacity { wmes: 8, nodes: 8, tokens: 8, ..ReteCapacity::four() }, wmes: 8, ..AgentCapacity::four() };
            let mut agent: SoarAgent<InlineSlots<8>> = SoarAgent::with_capacity(capacity);
            let s = agent.symbols.intern_id('S', 1);
            let p = agent.symbols.predefined;
            let (first, second, one) = (p.name, p.value, agent.symbols.intern_int(1));
            add_proposal(&mut agent, "propose-first", first, one, true);
            add_proposal(&mut agent, "propose-second", second, one, true);
            let (has_first, has_second): (bool, bool) = (kani::any(), kani::any());
            kani::assume(has_first || has_second);
            if has_first {
                agent.insert_wme(s, first, one);
            }
            if has_second {
                agent.insert_wme(s, second, one);
            }
            let decision = agent.run_decision_cycle(s);
            assert!(!agent.halted);
            let DecisionResult::Selected(selected) = decision else { panic!("an operator is selected") };
            assert_eq!(agent.selected_operator, Some(selected));
            // The operator each rule made is the value of the acceptable preference it asserted.
            let made_by = |rule: &str| agent.preferences.iter().find(|p| p.production == Some(rule)).map(|p| p.value);
            assert!(made_by("propose-first") == Some(selected) || made_by("propose-second") == Some(selected));
            assert_eq!(made_by("propose-first").is_some(), has_first);
            assert_eq!(made_by("propose-second").is_some(), has_second);
            kani::cover!(has_first && has_second, "both operators are proposed");
            // Freeing the symbol table's strings is a loop Kani cannot bound, and proves nothing.
            core::mem::forget(agent);
        }
    }

    /// Room for a substate: an impasse adds about 14 WMEs, all of which enter the Rete's WME
    /// pool. The other pools keep four slots, which keeps the agent small enough to build.
    struct RoomForSubstate;

    impl Layout for RoomForSubstate {
        type ReteWmes = InlineSlots<32>;
        type AlphaMemories = InlineSlots<4>;
        type AlphaEntries = InlineSlots<4>;
        type Nodes = InlineSlots<4>;
        type Tests = InlineSlots<4>;
        type DisjunctionValues = InlineSlots<4>;
        type Tokens = InlineSlots<4>;
        type Blocks = InlineSlots<4>;
        type Productions = InlineSlots<4>;
        type Matches = InlineSlots<4>;
        type Actions = InlineSlots<4>;
    }

    agent_proof! {
        /// With the WME that matches the proposal rule present or not, one decision cycle
        /// selects the proposed operator when it is present, and otherwise reaches a state
        /// no-change impasse and creates a substate.
        fn verify_decision_cycle_proposal_or_impasse() {
            let capacity = AgentCapacity { rete: ReteCapacity { wmes: 32, ..ReteCapacity::four() }, wmes: 32, ..AgentCapacity::four() };
            let mut agent: SoarAgent<RoomForSubstate> = SoarAgent::with_capacity(capacity);
            let s = agent.symbols.intern_id('S', 1);
            let (attr, value) = (agent.symbols.predefined.name, agent.symbols.intern_int(1));
            add_proposal(&mut agent, "propose", attr, value, false);
            let present: bool = kani::any();
            if present {
                agent.insert_wme(s, attr, value);
            }
            let decision = agent.run_decision_cycle(s);
            assert!(!agent.halted);
            if present {
                assert!(matches!(decision, DecisionResult::Selected(_)));
                assert!(agent.selected_operator.is_some());
            } else {
                assert_eq!(decision, DecisionResult::NoChangeImpasse);
                assert_eq!(agent.selected_operator, None);
                assert_eq!(agent.goal_stack().len(), 2);
            }
            kani::cover!(!present, "the impasse is reached");
            // Freeing the symbol table's strings is a loop Kani cannot bound, and proves nothing.
            core::mem::forget(agent);
        }
    }
}
