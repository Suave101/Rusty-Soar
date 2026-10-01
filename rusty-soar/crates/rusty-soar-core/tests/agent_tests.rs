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