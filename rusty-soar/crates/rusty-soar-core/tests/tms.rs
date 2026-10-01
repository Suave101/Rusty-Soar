use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::rete::AlphaTest;
use rusty_soar_core::tms::TruthMaintenanceSystem;
use rusty_soar_core::wm::{SupportType, WmeKey};

#[test]
fn independent_justifications_retract_only_after_last_support() {
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
fn i_supported_wmes_retract_when_instantiation_breaks() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let trigger = agent.symbols.intern_str("trigger");
    let active = agent.symbols.intern_str("active");
    let result = agent.symbols.intern_str("result");
    let complete = agent.symbols.intern_str("complete");

    agent.add_rule(
        "i-support-rule",
        vec![(
            AlphaTest {
                id: Some(state),
                attr: Some(trigger),
                val: Some(active),
            },
            vec![],
        )],
        vec![Action::Add {
            id: state,
            attr: result,
            val: complete,
        }],
    );

    let source = agent.insert_wme(state, trigger, active);
    agent.run_elaboration_phase();
    let derived = agent.wm.find(state, result, complete).expect("derived WME");
    assert_eq!(agent.wm.get(derived).unwrap().support, SupportType::ISupport);

    agent.remove_wme(source);
    assert!(agent.wm.get(derived).is_none());
}

#[test]
fn o_supported_wmes_survive_instantiation_retraction() {
    let mut agent = SoarAgent::new();
    let state = agent.symbols.intern_id('S', 1);
    let trigger = agent.symbols.intern_str("trigger");
    let active = agent.symbols.intern_str("active");
    let result = agent.symbols.intern_str("result");
    let complete = agent.symbols.intern_str("complete");

    agent.add_rule(
        "o-support-rule",
        vec![(
            AlphaTest {
                id: Some(state),
                attr: Some(trigger),
                val: Some(active),
            },
            vec![],
        )],
        vec![Action::Add {
            id: state,
            attr: result,
            val: complete,
        }],
    );

    let source = agent.insert_wme(state, trigger, active);
    agent.selected_operator = Some(agent.symbols.intern_str("apply"));
    agent.run_application_phase();
    let derived = agent.wm.find(state, result, complete).expect("derived WME");
    assert_eq!(agent.wm.get(derived).unwrap().support, SupportType::OSupport);

    agent.remove_wme(source);
    assert!(agent.wm.get(derived).is_some());
}
