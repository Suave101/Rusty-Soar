use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

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