use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;
use rusty_soar_core::symbol::SymbolId;
use rusty_soar_core::tms::TruthMaintenanceSystem;
use rusty_soar_core::wm::WmeKey;

#[test]
fn test_wme_insert_and_retract() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let attr = agent.symbols.intern_str("type");
    let val = agent.symbols.intern_str("state");

    let key = agent.insert_wme(s1, attr, val);

    let retrieved = agent.wm.get(key).expect("WME should exist in arena");
    assert_eq!(retrieved.id, s1);
    assert_eq!(retrieved.attr, attr);
    assert_eq!(retrieved.val, val);

    agent.remove_wme(key);
    assert!(agent.wm.get(key).is_none());
}

#[test]
fn test_working_memory_is_a_set() {
    let mut agent = SoarAgent::new();
    let id = agent.symbols.intern_id('S', 1);
    let attr = agent.symbols.intern_str("status");
    let value = agent.symbols.intern_str("ready");

    let first = agent.insert_wme(id, attr, value);
    let second = agent.insert_wme(id, attr, value);

    assert_eq!(first, second);
    assert_eq!(agent.wm.len(), 1);
    assert_eq!(agent.rete.pending_assertions(), 0);
}

#[test]
fn test_preference_resolution() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let op_a = agent.symbols.intern_id('O', 1);
    let op_b = agent.symbols.intern_id('O', 2);

    agent.add_preference(Preference {
        state: s1,
        operator: op_a,
        preference_type: PreferenceType::Acceptable,
    });

    agent.add_preference(Preference {
        state: s1,
        operator: op_b,
        preference_type: PreferenceType::Acceptable,
    });

    agent.add_preference(Preference {
        state: s1,
        operator: op_b,
        preference_type: PreferenceType::Reject,
    });

    agent.run_decision_phase(s1);

    assert_eq!(
        agent.selected_operator,
        Some(op_a),
        "Operator B was rejected; Operator A must be selected"
    );
}

#[test]
fn test_rete_elaboration_and_proposal() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let op1 = agent.symbols.intern_id('O', 1);

    let attr_sensor = agent.symbols.intern_str("sensor");
    let val_offline = agent.symbols.intern_str("offline");

    agent.add_rule(
        "propose_reboot",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_sensor),
                val: Some(val_offline),
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: op1,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    agent.insert_wme(s1, attr_sensor, val_offline);
    agent.run_decision_cycle(s1);

    assert_eq!(
        agent.selected_operator,
        Some(op1),
        "Proposed reboot operator should be selected"
    );
}

#[test]
fn test_multi_condition_production() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let op_emergency = agent.symbols.intern_id('O', 99);

    let attr_temp = agent.symbols.intern_str("temperature");
    let val_high = agent.symbols.intern_str("high");

    let attr_pressure = agent.symbols.intern_str("pressure");
    let val_critical = agent.symbols.intern_str("critical");

    agent.add_rule(
        "propose_emergency_shutdown",
        vec![
            (
                AlphaTest {
                    id: Some(s1),
                    attr: Some(attr_temp),
                    val: Some(val_high),
                },
                vec![],
            ),
            (
                AlphaTest {
                    id: Some(s1),
                    attr: Some(attr_pressure),
                    val: Some(val_critical),
                },
                vec![],
            ),
        ],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: op_emergency,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    agent.insert_wme(s1, attr_temp, val_high);
    agent.insert_wme(s1, attr_pressure, val_critical);

    agent.run_decision_cycle(s1);

    assert_eq!(
        agent.selected_operator,
        Some(op_emergency),
        "Emergency operator should be selected when all conditions match"
    );
}

#[test]
fn test_elaboration_has_deterministic_cycle_budget() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let attr = agent.symbols.intern_str("step");

    // A chain step0 -> step1 -> ... -> step15 needs one elaboration wave per link, which
    // exceeds the budget. (A rule re-adding its own match would only fire once: refraction.)
    let steps: Vec<_> = (0..16)
        .map(|i| agent.symbols.intern_str(&format!("step{i}")))
        .collect();
    for (i, pair) in steps.windows(2).enumerate() {
        let (from, to) = (pair[0], pair[1]);
        agent.add_rule(
            Box::leak(format!("chain{i}").into_boxed_str()),
            vec![(
                AlphaTest {
                    id: Some(state),
                    attr: Some(attr),
                    val: Some(from),
                },
                vec![],
            )],
            vec![Action::Add {
                id: state,
                attr,
                val: to,
            }],
        );
    }
    agent.insert_wme(state, attr, steps[0]);
    agent.max_elaborations = 10;

    assert_eq!(agent.run_elaboration_phase(), 10);
}

#[test]
fn test_tms_preserves_independently_supported_wme() {
    let mut tms = TruthMaintenanceSystem::new();
    let first_support = WmeKey(0);
    let second_support = WmeKey(1);
    let derived = WmeKey(2);

    tms.add_justification("first", vec![first_support], vec![derived]);
    tms.add_justification("second", vec![second_support], vec![derived]);

    assert!(tms.process_retraction(first_support).is_empty());
    assert_eq!(tms.process_retraction(second_support), vec![derived]);
}

#[test]
fn test_explicit_better_preference_selects_dominant_operator() {
    let state = SymbolId(1);
    let first = SymbolId(10);
    let second = SymbolId(20);
    let preferences = vec![
        Preference {
            state,
            operator: first,
            preference_type: PreferenceType::Acceptable,
        },
        Preference {
            state,
            operator: second,
            preference_type: PreferenceType::Acceptable,
        },
        Preference {
            state,
            operator: first,
            preference_type: PreferenceType::Better(second),
        },
    ];

    assert_eq!(
        rusty_soar_core::preference::resolve_preferences(state, &preferences),
        rusty_soar_core::preference::DecisionResult::Selected(first)
    );
}

#[test]
fn test_hard_and_best_preferences_are_deterministic() {
    let state = SymbolId(2);
    let required = SymbolId(30);
    let alternative = SymbolId(31);
    let preferences = vec![
        Preference {
            state,
            operator: required,
            preference_type: PreferenceType::Require,
        },
        Preference {
            state,
            operator: alternative,
            preference_type: PreferenceType::Acceptable,
        },
    ];

    assert_eq!(
        rusty_soar_core::preference::resolve_preferences(state, &preferences),
        DecisionResult::Selected(required)
    );

    let best = vec![
        Preference {
            state,
            operator: required,
            preference_type: PreferenceType::Acceptable,
        },
        Preference {
            state,
            operator: alternative,
            preference_type: PreferenceType::Acceptable,
        },
        Preference {
            state,
            operator: alternative,
            preference_type: PreferenceType::Best,
        },
    ];
    assert_eq!(
        rusty_soar_core::preference::resolve_preferences(state, &best),
        DecisionResult::Selected(alternative)
    );
}

#[test]
fn test_parsed_soar_script_installs_and_runs() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {propose-demo
            (state <s> ^sensor ready)
        -->
            (<s> ^operator <o> +)
            (<o> ^name demo)
        }
        "#,
    )
    .expect("script should parse");

    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    agent.install_soar_script(&script, state).expect("script should install");
    let attr = agent.symbols.intern_str("sensor");
    let value = agent.symbols.intern_str("ready");
    agent.insert_wme(state, attr, value);

    agent.run_elaboration_phase();
    let DecisionResult::Selected(operator) = agent.run_decision_phase(state) else {
        panic!("an operator should be selected");
    };
    assert_eq!(operator_name(&agent, operator).as_deref(), Some("demo"));
}

/// The `^name` of an operator identifier created by a RHS.
fn operator_name(agent: &SoarAgent, operator: rusty_soar_core::symbol::SymbolId) -> Option<String> {
    use rusty_soar_core::symbol::SymbolData;
    agent.wm.iter().find_map(|w| {
        let is_name = matches!(agent.symbols.resolve(w.attr), Some(SymbolData::String(s)) if s == "name");
        match agent.symbols.resolve(w.val) {
            Some(SymbolData::String(name)) if w.id == operator && is_name => Some(name.clone()),
            _ => None,
        }
    })
}

#[test]
fn test_parsed_soar_script_preserves_identifier_joins() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {propose-linked
            (state <s> ^child <c>)
            (<c> ^status ready)
        -->
            (<s> ^operator <o> +)
            (<o> ^name linked)
        }
        "#,
    )
    .expect("script should parse");

    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let child = agent.symbols.intern_id('C', 1);
    let child_attr = agent.symbols.intern_str("child");
    let status_attr = agent.symbols.intern_str("status");
    let ready = agent.symbols.intern_str("ready");
    agent.install_soar_script(&script, state).expect("script should install");
    agent.insert_wme(state, child_attr, child);
    agent.insert_wme(child, status_attr, ready);

    agent.run_elaboration_phase();
    let DecisionResult::Selected(operator) = agent.run_decision_phase(state) else {
        panic!("an operator should be selected");
    };
    assert_eq!(operator_name(&agent, operator).as_deref(), Some("linked"));
}

#[test]
fn test_generated_soar_rules_expand_foreach_templates() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        proc generate { types } {
            foreach type $types {
                sp "generated*${type}
                    (state <s> ^sensor-${type} ready)
                -->
                    (<s> ^operator <o> +)
                    (<o> ^name ${type})
                "
            }
        }
        generate { gps lidar }
        "#,
    )
    .expect("generated rules should parse");

    assert_eq!(script.productions.len(), 2);
    assert_eq!(script.productions[0].name, "generated*gps");
    assert_eq!(script.productions[1].name, "generated*lidar");
}

#[test]
fn test_soar_preference_markers_are_preserved() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {require-demo
            (state <s> ^ready yes)
        -->
            (<s> ^operator <o> !)
            (<o> ^name required)
        }
        "#,
    )
    .expect("script should parse");

    let rusty_soar_core::soar_parser::SoarAction::Make { preference, .. } = &script.productions[0].actions[0] else {
        panic!("expected a make action");
    };
    assert_eq!(*preference, rusty_soar_core::soar_parser::SoarPreference::Require);
}

#[test]
fn test_soar_rhs_wme_actions_are_preserved() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {apply-demo
            (state <s> ^operator <o>)
            (<o> ^name demo)
        -->
            (<s> ^result complete)
            (<s> ^old-value stale -)
        }
        "#,
    )
    .expect("script should parse");

    use rusty_soar_core::soar_parser::{SoarAction, SoarPreference, SoarRhsValue, SoarSymbol};
    let actions = &script.productions[0].actions;
    assert_eq!(actions.len(), 2);
    let SoarAction::Make { attr, preference, .. } = &actions[0] else { panic!("expected a make action") };
    assert_eq!(*attr, SoarRhsValue::Symbol(SoarSymbol::Str("result".into())));
    assert_eq!(*preference, SoarPreference::Acceptable);
    let SoarAction::Make { preference, .. } = &actions[1] else { panic!("expected a make action") };
    assert_eq!(*preference, SoarPreference::Reject, "`-` on a non-operator value removes the WME");
}
#[test]
fn halt_ends_the_cycle_before_output_and_stops_further_runs() {
    use rusty_soar_core::soar_parser::SoarScript;
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    let script = SoarScript::parse(
        "sp {propose (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name stop)}\n\
         sp {apply (state <s> ^operator.name stop ^io.output-link <out>) --> (<out> ^done yes) (halt)}",
    )
    .unwrap();
    agent.install_soar_script(&script, state).unwrap();

    assert!(matches!(agent.run_decision_cycle(state), DecisionResult::Selected(_)));
    assert!(agent.halted);
    assert!(agent.take_output_events().is_empty(), "upstream stops before the output phase");

    assert_eq!(agent.run_decision_cycle(state), DecisionResult::Halted);
    assert!(agent.output.ends_with("System halted.  Use (init-soar) before running Soar again."));
}

/// A production added from flat conditions and actions (the form Kani proofs use) runs exactly
/// as the same production added in the ordinary form.
#[test]
fn flat_production_runs_as_the_ordinary_form() {
    use rusty_soar_core::agent::{FlatAction, FlatTerm};
    use rusty_soar_core::rete::{Field, FlatCondition, FlatTests, VarLocation};
    use rusty_soar_core::soar_parser::SoarPreference;

    let run = |flat: bool| {
        let mut agent = SoarAgent::new();
        let s = agent.symbols.intern_id('S', 1);
        let a = agent.symbols.intern_str("a");
        let v = agent.symbols.intern_int(1);
        let operator = agent.symbols.predefined.operator;
        let alpha = AlphaTest { attr: Some(a), val: Some(v), ..AlphaTest::default() };
        if flat {
            agent.add_production_flat(
                "propose",
                &[FlatCondition::Positive(FlatTests { alpha, acceptable: false, tests: &[] })],
                &[FlatAction::make(
                    FlatTerm::Bound(VarLocation { depth: 1, field: Field::Id }),
                    FlatTerm::Constant(operator),
                    FlatTerm::NewIdentifier(0),
                    SoarPreference::Acceptable,
                )],
                &['O'],
            );
        } else {
            let script = rusty_soar_core::soar_parser::SoarScript::parse("sp {propose (state <s> ^a 1) --> (<s> ^operator <o> +)}").unwrap();
            agent.install_soar_script(&script, s).unwrap();
        }
        agent.insert_wme(s, a, v);
        let decision = agent.run_decision_cycle(s);
        (decision, agent.selected_operator.map(|o| rusty_soar_core::agent::symbol_to_string(&agent.symbols, o, false)))
    };
    let flat = run(true);
    assert!(matches!(flat.0, DecisionResult::Selected(_)), "{:?}", flat);
    assert_eq!(flat, run(false));
}

/// The setup of the proposal Kani proofs (`formal-soar-verify`), run as a test: with four of
/// everything, the proposing rule's operator is selected when its WME is present; with room
/// for 32 WMEs, a state no-change impasse and its substate fit when it is not.
#[test]
fn proposal_proof_setup_fits_its_capacity() {
    use rusty_soar_core::agent::{AgentCapacity, FlatAction, FlatTerm};
    use rusty_soar_core::rete::{Field, FlatCondition, FlatTests, ReteCapacity, VarLocation};
    use rusty_soar_core::soar_parser::SoarPreference;

    let room_for_substate = AgentCapacity { rete: ReteCapacity { wmes: 32, ..ReteCapacity::four() }, wmes: 32, ..AgentCapacity::four() };
    for (capacity, present) in [(AgentCapacity::four(), true), (room_for_substate, true), (room_for_substate, false)] {
        let mut agent: SoarAgent = SoarAgent::with_capacity(capacity);
        let s = agent.symbols.intern_id('S', 1);
        let (attr, value) = (agent.symbols.predefined.name, agent.symbols.intern_int(1));
        let operator = agent.symbols.predefined.operator;
        agent.add_production_flat(
            "propose",
            &[FlatCondition::Positive(FlatTests { alpha: AlphaTest { attr: Some(attr), val: Some(value), ..AlphaTest::default() }, acceptable: false, tests: &[] })],
            &[FlatAction::make(
                FlatTerm::Bound(VarLocation { depth: 1, field: Field::Id }),
                FlatTerm::Constant(operator),
                FlatTerm::NewIdentifier(0),
                SoarPreference::Acceptable,
            )],
            &['O'],
        );
        if present {
            agent.insert_wme(s, attr, value);
        }
        let decision = agent.run_decision_cycle(s);
        assert!(!agent.halted, "{}", agent.output);
        if present {
            assert!(matches!(decision, DecisionResult::Selected(_)));
        } else {
            assert_eq!(decision, DecisionResult::NoChangeImpasse);
            assert_eq!(agent.goal_stack().len(), 2);
        }
    }
}

/// The setup of the indifferent-choice Kani proof (`formal-soar-verify`), run as a test: with
/// the proof's capacities, each combination of the two WMEs selects one of the operators whose
/// WME is present.
#[test]
fn indifferent_choice_proof_setup_fits_its_capacity() {
    use rusty_soar_core::agent::{AgentCapacity, FlatAction, FlatTerm};
    use rusty_soar_core::rete::{Field, FlatCondition, FlatTests, ReteCapacity, VarLocation};
    use rusty_soar_core::soar_parser::SoarPreference;

    for (has_first, has_second) in [(true, false), (false, true), (true, true)] {
        let capacity = AgentCapacity { rete: ReteCapacity { wmes: 8, nodes: 8, tokens: 8, ..ReteCapacity::four() }, wmes: 8, ..AgentCapacity::four() };
        let mut agent: SoarAgent = SoarAgent::with_capacity(capacity);
        let s = agent.symbols.intern_id('S', 1);
        let p = agent.symbols.predefined;
        let (first, second, one) = (p.name, p.value, agent.symbols.intern_int(1));
        for (name, attr) in [("propose-first", first), ("propose-second", second)] {
            let make = |preference| {
                FlatAction::make(FlatTerm::Bound(VarLocation { depth: 1, field: Field::Id }), FlatTerm::Constant(p.operator), FlatTerm::NewIdentifier(0), preference)
            };
            agent.add_production_flat(
                name,
                &[FlatCondition::Positive(FlatTests { alpha: AlphaTest { attr: Some(attr), val: Some(one), ..AlphaTest::default() }, acceptable: false, tests: &[] })],
                &[make(SoarPreference::Acceptable), make(SoarPreference::UnaryIndifferent)],
                &['O'],
            );
        }
        if has_first {
            agent.insert_wme(s, first, one);
        }
        if has_second {
            agent.insert_wme(s, second, one);
        }
        let decision = agent.run_decision_cycle(s);
        assert!(!agent.halted, "{}", agent.output);
        let DecisionResult::Selected(selected) = decision else { panic!("{:?}", decision) };
        let made_by = |rule: &str| agent.preferences.iter().find(|p| p.production == Some(rule)).map(|p| p.value);
        assert!(made_by("propose-first") == Some(selected) || made_by("propose-second") == Some(selected));
        assert_eq!((made_by("propose-first").is_some(), made_by("propose-second").is_some()), (has_first, has_second));
    }
}

/// A full Rete pool halts the agent with an error naming the pool.
#[test]
fn full_rete_pool_halts_the_agent() {
    use rusty_soar_core::agent::AgentCapacity;

    let mut agent: SoarAgent = SoarAgent::with_capacity(AgentCapacity::four());
    let s = agent.symbols.intern_id('S', 1);
    let attr = agent.symbols.predefined.name;
    for value in 0..5 {
        let value = agent.symbols.intern_int(value);
        agent.insert_wme(s, attr, value);
    }
    assert!(agent.halted);
    assert_eq!(agent.output, "Error: Rete capacity exceeded (Wmes); agent halted.\n");
}
