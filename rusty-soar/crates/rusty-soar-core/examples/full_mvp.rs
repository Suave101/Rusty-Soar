use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let o1 = agent.symbols.intern_id('O', 1);

    let attr_type = agent.symbols.intern_str("type");
    let val_state = agent.symbols.intern_str("state");

    agent.add_rule(
        "propose-op1",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_type),
                val: Some(val_state),
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: o1,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    println!("=== RUSTY-SOAR V1.0 FULL MVP DEMO ===");

    agent.insert_wme(s1, attr_type, val_state);
    let total_fired = agent.run_elaboration_phase();
    println!("Elaboration rules fired: {}", total_fired);

    let decision = agent.run_decision_phase(s1);
    println!("Decision phase result: {:?}", decision);

    assert_eq!(decision, DecisionResult::Selected(o1));
    println!("Full MVP execution: SUCCESS!");
}
