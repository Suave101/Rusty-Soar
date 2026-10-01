use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::epmem::EpisodicMemory;
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;
use rusty_soar_core::symbol::SymbolId;
use rusty_soar_core::tms::TruthMaintenanceSystem;
use rusty_soar_core::wm::{SupportType, WmeArena, WmeKey};

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
fn test_i_support_retracts_and_o_support_persists() {
    let mut proposal_agent = SoarAgent::new();
    let proposal_state = proposal_agent.symbols.intern_id('S', 1);
    let trigger = proposal_agent.symbols.intern_str("trigger");
    let active = proposal_agent.symbols.intern_str("active");
    let result = proposal_agent.symbols.intern_str("result");
    let complete = proposal_agent.symbols.intern_str("complete");
    proposal_agent.add_rule(
        "proposal-support",
        vec![(
            AlphaTest {
                id: Some(proposal_state),
                attr: Some(trigger),
                val: Some(active),
            },
            vec![],
        )],
        vec![Action::Add {
            id: proposal_state,
            attr: result,
            val: complete,
        }],
    );
    let proposal_trigger = proposal_agent.insert_wme(proposal_state, trigger, active);
    proposal_agent.run_elaboration_phase();
    let proposal_result = proposal_agent
        .wm
        .find(proposal_state, result, complete)
        .expect("proposal should derive a result");
    assert_eq!(
        proposal_agent.wm.get(proposal_result).unwrap().support,
        SupportType::ISupport
    );
    proposal_agent.remove_wme(proposal_trigger);
    assert!(proposal_agent.wm.get(proposal_result).is_none());

    let mut application_agent = SoarAgent::new();
    let application_state = application_agent.symbols.intern_id('S', 1);
    let trigger = application_agent.symbols.intern_str("trigger");
    let active = application_agent.symbols.intern_str("active");
    let result = application_agent.symbols.intern_str("result");
    let complete = application_agent.symbols.intern_str("complete");
    application_agent.add_rule(
        "application-support",
        vec![(
            AlphaTest {
                id: Some(application_state),
                attr: Some(trigger),
                val: Some(active),
            },
            vec![],
        )],
        vec![Action::Add {
            id: application_state,
            attr: result,
            val: complete,
        }],
    );
    let application_trigger = application_agent.insert_wme(application_state, trigger, active);
    application_agent.selected_operator = Some(application_agent.symbols.intern_str("apply"));
    application_agent.run_application_phase();
    let application_result = application_agent
        .wm
        .find(application_state, result, complete)
        .expect("application should derive a result");
    assert_eq!(
        application_agent.wm.get(application_result).unwrap().support,
        SupportType::OSupport
    );
    application_agent.remove_wme(application_trigger);
    assert!(application_agent.wm.get(application_result).is_some());
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
    assert_eq!(agent.rete.activations.len(), 0);
}

#[test]
fn test_preference_resolution() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let op_a = agent.symbols.intern_id('O', 1);
    let op_b = agent.symbols.intern_id('O', 2);

    agent.preferences.push(Preference {
        state: s1,
        operator: op_a,
        preference_type: PreferenceType::Acceptable,
    });

    agent.preferences.push(Preference {
        state: s1,
        operator: op_b,
        preference_type: PreferenceType::Acceptable,
    });

    agent.preferences.push(Preference {
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
fn test_selected_operator_is_architectural_wme() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let first = agent.symbols.intern_id('O', 1);
    let second = agent.symbols.intern_id('O', 2);
    let operator_attr = agent.symbols.intern_str("operator");

    agent.preferences.push(Preference {
        state,
        operator: first,
        preference_type: PreferenceType::Acceptable,
    });
    assert_eq!(agent.run_decision_phase(state), DecisionResult::Selected(first));
    let first_key = agent
        .wm
        .find(state, operator_attr, first)
        .expect("selected operator WME");
    assert_eq!(agent.wm.get(first_key).unwrap().support, SupportType::OSupport);

    agent.preferences.push(Preference {
        state,
        operator: second,
        preference_type: PreferenceType::Require,
    });
    assert_eq!(agent.run_decision_phase(state), DecisionResult::Selected(second));
    assert!(agent.wm.find(state, operator_attr, first).is_none());
    assert!(agent.wm.find(state, operator_attr, second).is_some());
}

#[test]
fn test_impasse_substate_has_architectural_fields() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let first = agent.symbols.intern_id('O', 1);
    let second = agent.symbols.intern_id('O', 2);
    agent.preferences.extend([
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
    ]);

    assert!(matches!(
        agent.run_decision_phase(state),
        DecisionResult::TieImpasse(_)
    ));
    let substate = agent.substates[0].substate_id;
    let type_attr = agent.symbols.intern_str("type");
    let choices_attr = agent.symbols.intern_str("choices");
    let attribute_attr = agent.symbols.intern_str("attribute");
    assert!(agent
        .wm
        .find(substate, type_attr, agent.symbols.intern_str("state"))
        .is_some());
    assert!(agent
        .wm
        .find(substate, choices_attr, agent.symbols.intern_str("multiple"))
        .is_some());
    assert!(agent
        .wm
        .find(substate, attribute_attr, agent.symbols.intern_str("operator"))
        .is_some());
}

#[test]
fn test_duplicate_preferences_are_preserved() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let operator = agent.symbols.intern_id('O', 1);
    let attribute = agent.symbols.intern_str("sensor");
    let value = agent.symbols.intern_str("ready");
    let conditions = vec![(
        AlphaTest {
            id: Some(state),
            attr: Some(attribute),
            val: Some(value),
        },
        vec![],
    )];
    let preference = Preference {
        state,
        operator,
        preference_type: PreferenceType::Acceptable,
    };

    agent.add_rule(
        "first-proposal",
        conditions.clone(),
        vec![Action::Prefer(preference.clone())],
    );
    agent.add_rule(
        "second-proposal",
        conditions,
        vec![Action::Prefer(preference.clone())],
    );
    agent.insert_wme(state, attribute, value);

    assert_eq!(agent.run_elaboration_phase(), 2);
    assert_eq!(agent.preferences, vec![preference.clone(), preference]);
    assert_eq!(agent.run_decision_phase(state), DecisionResult::Selected(operator));
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
fn test_elaboration_fires_each_binding_once_until_retraction() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let attr = agent.symbols.intern_str("loop");
    let value = agent.symbols.intern_str("active");

    agent.add_rule(
        "self_retrigger",
        vec![(
            AlphaTest {
                id: Some(state),
                attr: Some(attr),
                val: Some(value),
            },
            vec![],
        )],
        vec![Action::Add {
            id: state,
            attr,
            val: value,
        }],
    );
    agent.insert_wme(state, attr, value);

    assert_eq!(agent.run_elaboration_phase(), 1);
    assert_eq!(agent.run_elaboration_phase(), 0);
        let first_instantiation = agent.rete.active_instantiations[0].id;

    let wme = agent.wm.find(state, attr, value).expect("source WME exists");
    agent.remove_wme(wme);
    agent.insert_wme(state, attr, value);
    assert_eq!(agent.run_elaboration_phase(), 1);
        let second_instantiation = agent.rete.active_instantiations[0].id;
        assert_ne!(first_instantiation, second_instantiation);
}

    #[test]
    fn test_elaboration_reaches_quiescence_past_legacy_cycle_limit() {
        let mut agent = SoarAgent::new();
        let state = agent.symbols.intern_id('S', 1);
        let active = agent.symbols.intern_str("active");
        let mut attributes = Vec::new();
        for index in 0..=12 {
            attributes.push(agent.symbols.intern_str(&format!("stage{index}")));
        }

        for index in 0..12 {
            let name: &'static str = Box::leak(format!("chain-{index}").into_boxed_str());
            agent.add_rule(
                name,
                vec![
                    (
                        AlphaTest {
                            id: Some(state),
                            attr: Some(attributes[index]),
                            val: Some(active),
                        },
                        vec![],
                    ),
                ],
                vec![Action::Add {
                    id: state,
                    attr: attributes[index + 1],
                    val: active,
                }],
            );
        }

        agent.insert_wme(state, attributes[0], active);
        assert_eq!(agent.run_elaboration_phase(), 12);
        assert!(agent.wm.find(state, attributes[12], active).is_some());
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
fn test_episodic_memory_captures_sparse_wme_arena() {
    let mut wm = WmeArena::new();
    let first = wm.insert(SymbolId(1), SymbolId(2), SymbolId(3), SupportType::ISupport);
    wm.insert(SymbolId(4), SymbolId(5), SymbolId(6), SupportType::ISupport);
    wm.insert(SymbolId(7), SymbolId(8), SymbolId(9), SupportType::ISupport);
    wm.remove(first);

    let mut epmem = EpisodicMemory::new();
    let episode = epmem.record_episode(1, &wm);
    let snapshot = epmem.retrieve(episode).expect("episode should exist");

    assert_eq!(snapshot.wmes.len(), 2);
    assert!(snapshot.wmes.contains(&(SymbolId(7), SymbolId(8), SymbolId(9))));
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
    agent.install_soar_script(&script, state);
    let attr = agent.symbols.intern_str("sensor");
    let value = agent.symbols.intern_str("ready");
    agent.insert_wme(state, attr, value);

    agent.run_elaboration_phase();
    assert_eq!(agent.run_decision_phase(state), DecisionResult::Selected(
        agent.symbols.intern_str("demo"),
    ));
}

#[test]
fn test_parsed_numeric_relation_matches_wme_value() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {high-altitude
            (state <s> ^altitude > 100)
        -->
            (<s> ^result high)
        }
        "#,
    )
    .expect("script should parse");

    assert_eq!(
        script.productions[0].conditions[0].relation,
        rusty_soar_core::soar_parser::SoarRelation::Greater
    );

    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    agent.install_soar_script(&script, state);
    let altitude = agent.symbols.intern_str("altitude");
    let value = agent.symbols.intern_str("150");
    agent.insert_wme(state, altitude, value);

    agent.run_elaboration_phase();
    assert!(agent
        .wm
        .find(state, agent.symbols.intern_str("result"), agent.symbols.intern_str("high"))
        .is_some());
}

#[test]
fn test_parsed_disjunction_matches_any_alternative() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {color-match
            (state <s> ^color << red blue >>)
        -->
            (<s> ^result colorful)
        }
        "#,
    )
    .expect("script should parse");

    assert!(matches!(
        script.productions[0].conditions[0].value,
        rusty_soar_core::soar_parser::SoarValue::Disjunction(_)
    ));

    for color in ["red", "blue"] {
        let mut agent = SoarAgent::new();
        let state = agent.symbols.intern_id('S', 1);
        agent.install_soar_script(&script, state);
        let color_attr = agent.symbols.intern_str("color");
        let color_value = agent.symbols.intern_str(color);
        agent.insert_wme(state, color_attr, color_value);
        agent.run_elaboration_phase();
        assert!(agent
            .wm
            .find(state, agent.symbols.intern_str("result"), agent.symbols.intern_str("colorful"))
            .is_some());
    }
}

#[test]
fn test_parsed_attribute_path_matches_linked_wmes() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {location-match
            (state <s> ^location.name home)
        -->
            (<s> ^result located)
        }
        "#,
    )
    .expect("script should parse");

    assert_eq!(script.productions[0].conditions[0].attribute, "location.name");

    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let location = agent.symbols.intern_id('L', 1);
    agent.install_soar_script(&script, state);
    let location_attr = agent.symbols.intern_str("location");
    let name_attr = agent.symbols.intern_str("name");
    let home = agent.symbols.intern_str("home");
    agent.insert_wme(state, location_attr, location);
    agent.insert_wme(location, name_attr, home);
    agent.run_elaboration_phase();
    assert!(agent
        .wm
        .find(state, agent.symbols.intern_str("result"), agent.symbols.intern_str("located"))
        .is_some());
}

#[test]
fn test_basic_rhs_control_functions_execute_deterministically() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {write-rule
            (state <s> ^ready yes)
        -->
            (write hello)
        }
        sp {interrupt-rule
            (state <s> ^ready yes)
        -->
            (interrupt)
        }
        "#,
    )
    .expect("script should parse");

    assert_eq!(script.productions[0].rhs_functions.len(), 1);
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    agent.install_soar_script(&script, state);
    let ready = agent.symbols.intern_str("ready");
    let yes = agent.symbols.intern_str("yes");
    agent.insert_wme(state, ready, yes);

    assert_eq!(agent.run_elaboration_phase(), 2);
    assert!(agent.interrupted);
    assert_eq!(agent.output, vec!["hello".to_string()]);

    let halt_script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {halt-rule
            (state <s> ^ready yes)
        -->
            (halt)
        }
        "#,
    )
    .expect("halt script should parse");
    let mut halted_agent = SoarAgent::new();
    let state = halted_agent.symbols.intern_id('S', 1);
    halted_agent.install_soar_script(&halt_script, state);
    let ready = halted_agent.symbols.intern_str("ready");
    let yes = halted_agent.symbols.intern_str("yes");
    halted_agent.insert_wme(state, ready, yes);
    assert_eq!(halted_agent.run_elaboration_phase(), 1);
    assert!(halted_agent.halted);
    assert_eq!(halted_agent.run_elaboration_phase(), 0);
}

#[test]
fn test_constant_rhs_arithmetic_creates_typed_result_symbol() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {calculate
            (state <s> ^ready yes)
        -->
            (<s> ^sum (+ 1 2))
        }
        "#,
    )
    .expect("arithmetic script should parse");

    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    agent.install_soar_script(&script, state);
    let ready = agent.symbols.intern_str("ready");
    let yes = agent.symbols.intern_str("yes");
    agent.insert_wme(state, ready, yes);
    agent.run_elaboration_phase();

    let sum = agent.symbols.intern_str("sum");
    let three = agent.symbols.intern_str("3");
    assert!(agent.wm.find(state, sum, three).is_some());
}

#[test]
fn test_negated_condition_is_preserved_in_ast() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {without-block
            (state <s> ^ready yes)
            -(<s> ^blocked yes)
        -->
            (<s> ^result clear)
        }
        "#,
    )
    .expect("script should parse");

    assert!(script.productions[0].conditions[1].negated);

    let mut clear_agent = SoarAgent::new();
    let state = clear_agent.symbols.intern_id('S', 1);
    clear_agent.install_soar_script(&script, state);
    let ready = clear_agent.symbols.intern_str("ready");
    let yes = clear_agent.symbols.intern_str("yes");
    clear_agent.insert_wme(state, ready, yes);
    clear_agent.run_elaboration_phase();
    assert!(clear_agent
        .wm
        .find(state, clear_agent.symbols.intern_str("result"), clear_agent.symbols.intern_str("clear"))
        .is_some());
    let blocked = clear_agent.symbols.intern_str("blocked");
    let yes = clear_agent.symbols.intern_str("yes");
    let blocker = clear_agent.insert_wme(state, blocked, yes);
    assert!(clear_agent
        .wm
        .find(state, clear_agent.symbols.intern_str("result"), clear_agent.symbols.intern_str("clear"))
        .is_none());
    clear_agent.remove_wme(blocker);
    clear_agent.run_elaboration_phase();
    assert!(clear_agent
        .wm
        .find(state, clear_agent.symbols.intern_str("result"), clear_agent.symbols.intern_str("clear"))
        .is_some());

    let mut blocked_agent = SoarAgent::new();
    let state = blocked_agent.symbols.intern_id('S', 1);
    blocked_agent.install_soar_script(&script, state);
    let blocked = blocked_agent.symbols.intern_str("blocked");
    let ready = blocked_agent.symbols.intern_str("ready");
    let yes = blocked_agent.symbols.intern_str("yes");
    blocked_agent.insert_wme(state, blocked, yes);
    blocked_agent.insert_wme(state, ready, yes);
    blocked_agent.run_elaboration_phase();
    assert!(blocked_agent
        .wm
        .find(state, blocked_agent.symbols.intern_str("result"), blocked_agent.symbols.intern_str("clear"))
        .is_none());
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
    agent.install_soar_script(&script, state);
    let child_attr = agent.symbols.intern_str("child");
    agent.insert_wme(state, child_attr, child);
    let status_attr = agent.symbols.intern_str("status");
    let ready_val = agent.symbols.intern_str("ready");
    agent.insert_wme(child, status_attr, ready_val);

    agent.run_elaboration_phase();
    assert_eq!(
        agent.run_decision_phase(state),
        DecisionResult::Selected(agent.symbols.intern_str("linked"))
    );
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

    assert_eq!(
        script.productions[0].actions[0].preference,
        rusty_soar_core::soar_parser::SoarPreference::Require
    );
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

    assert_eq!(script.productions[0].wme_actions.len(), 2);
    assert_eq!(script.productions[0].wme_actions[0].attribute, "result");
    assert!(script.productions[0].wme_actions[1].remove);
}

#[test]
fn test_parsed_soar_rhs_wme_actions_execute_with_bindings() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(
        r#"
        sp {add-result
            (state <s> ^sensor ready)
        -->
            (<s> ^result complete)
        }
        sp {consume-result
            (state <s> ^result complete)
        -->
            (<s> ^observed matched)
        }
        sp {remove-result
            (state <s> ^observed matched)
        -->
            (<s> ^result complete -)
        }
        "#,
    )
    .expect("script should parse");

    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    agent.install_soar_script(&script, state);
    let sensor = agent.symbols.intern_str("sensor");
    let ready = agent.symbols.intern_str("ready");
    agent.insert_wme(state, sensor, ready);

    agent.run_elaboration_phase();

    let result = agent.symbols.intern_str("result");
    let complete = agent.symbols.intern_str("complete");
    let observed = agent.symbols.intern_str("observed");
    let matched = agent.symbols.intern_str("matched");
    assert!(agent.wm.find(state, result, complete).is_none());
    assert!(agent.wm.find(state, observed, matched).is_none());
}