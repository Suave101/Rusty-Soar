use rusty_soar_core::agent::{Agent, Phase};
use rusty_soar_core::decision::{Preference, PreferenceType};
use rusty_soar_core::rete::ProductionId;
use rusty_soar_core::symbol::SymbolData;
use rusty_soar_core::wm::SupportType;

#[test]
fn test_symbol_interning() {
    let mut agent = Agent::new();
    let id1 = agent.symbols.intern_str("^sensor-status");
    let id2 = agent.symbols.intern_str("^sensor-status");
    let id3 = agent.symbols.intern_str("offline");

    assert_eq!(id1, id2, "Identical strings must return identical SymbolId handles");
    assert_ne!(id1, id3, "Distinct strings must return unique SymbolId handles");

    if let Some(SymbolData::String(s)) = agent.symbols.resolve(id1) {
        assert_eq!(s, "^sensor-status");
    } else {
        panic!("Failed to resolve interned string symbol");
    }
}

#[test]
fn test_wme_arena_lifecycle() {
    let mut agent = Agent::new();
    let s1 = agent.symbols.intern_id('S', 1);
    let attr = agent.symbols.intern_str("^status");
    let val = agent.symbols.intern_str("active");

    let key = agent.add_wme(s1, attr, val, SupportType::ISupport);
    assert_eq!(agent.wm.len(), 1);

    let retrieved = agent.wm.get(key).unwrap();
    assert_eq!(retrieved.id, s1);
    assert_eq!(retrieved.attribute, attr);
    assert_eq!(retrieved.value, val);

    let removed = agent.remove_wme(key);
    assert!(removed.is_some());
    assert_eq!(agent.wm.len(), 0);
    assert!(agent.wm.get(key).is_none(), "Generational key must be invalidated");
}

#[test]
fn test_decision_preference_resolution() {
    let mut agent = Agent::new();
    let s1 = agent.symbols.intern_id('S', 1);
    let attr_op = agent.symbols.intern_str("^operator");
    let op_a = agent.symbols.intern_id('O', 1);
    let op_b = agent.symbols.intern_id('O', 2);

    agent.preference_buffer.push(Preference {
        pref_type: PreferenceType::Acceptable,
        state_id: s1,
        attribute: attr_op,
        candidate: op_a,
        referent: None,
        source_wme: None,
    });
    agent.preference_buffer.push(Preference {
        pref_type: PreferenceType::Acceptable,
        state_id: s1,
        attribute: attr_op,
        candidate: op_b,
        referent: None,
        source_wme: None,
    });
    agent.preference_buffer.push(Preference {
        pref_type: PreferenceType::Reject,
        state_id: s1,
        attribute: attr_op,
        candidate: op_b,
        referent: None,
        source_wme: None,
    });

    agent.step();

    assert_eq!(agent.current_phase, Phase::Output);
    assert_eq!(agent.active_operator, Some(op_a), "Operator B was rejected; Operator A must be selected");
}

#[test]
fn test_end_to_end_rete_rule_trigger() {
    let mut agent = Agent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let attr_sensor = agent.symbols.intern_str("^sensor-status");
    let val_offline = agent.symbols.intern_str("offline");
    let attr_op = agent.symbols.intern_str("^operator");
    let op_failover = agent.symbols.intern_id('O', 99);

    // Register Rule: IF (^sensor-status offline) THEN propose operator O99 (+)
    let rule_pref = Preference {
        pref_type: PreferenceType::Acceptable,
        state_id: s1,
        attribute: attr_op,
        candidate: op_failover,
        referent: None,
        source_wme: None,
    };

    agent.rete.register_rule(
        ProductionId(1),
        attr_sensor,
        val_offline,
        vec![rule_pref],
    );

    // Assert telemetry WME: (S1 ^sensor-status offline)
    agent.add_wme(s1, attr_sensor, val_offline, SupportType::ISupport);

    // Run decision cycle
    agent.step();

    // Verify operator O99 was proposed by Rete and selected in Decision phase
    assert_eq!(
        agent.active_operator,
        Some(op_failover),
        "Rete rule should fire upon WME match and select failover operator O99"
    );
}
#[test]
fn test_multi_condition_rule_trigger() {
    let mut agent = Agent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let attr_temp = agent.symbols.intern_str("^temperature");
    let val_high = agent.symbols.intern_str("high");
    
    let attr_pressure = agent.symbols.intern_str("^pressure");
    let val_critical = agent.symbols.intern_str("critical");

    let attr_op = agent.symbols.intern_str("^operator");
    let op_vent = agent.symbols.intern_id('O', 10);

    // Rule 1: High Temperature -> Vent Operator
    agent.rete.register_rule(
        ProductionId(101),
        attr_temp,
        val_high,
        vec![Preference {
            pref_type: PreferenceType::Acceptable,
            state_id: s1,
            attribute: attr_op,
            candidate: op_vent,
            referent: None,
            source_wme: None,
        }],
    );

    // Rule 2: Critical Pressure -> Vent Operator
    agent.rete.register_rule(
        ProductionId(102),
        attr_pressure,
        val_critical,
        vec![Preference {
            pref_type: PreferenceType::Acceptable,
            state_id: s1,
            attribute: attr_op,
            candidate: op_vent,
            referent: None,
            source_wme: None,
        }],
    );

    // Assert temperature and pressure telemetry
    agent.add_wme(s1, attr_temp, val_high, SupportType::ISupport);
    agent.add_wme(s1, attr_pressure, val_critical, SupportType::ISupport);

    agent.step();

    assert_eq!(
        agent.active_operator,
        Some(op_vent),
        "Vent operator O10 must be selected upon telemetry condition match"
    );
}