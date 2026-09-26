use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    // Symbols
    let s1 = agent.symbols.intern_id('S', 1);
    let attr_input = agent.symbols.intern_str("sensor-input");
    let val_obstacle = agent.symbols.intern_str("obstacle-detected");

    let o1_avoid = agent.symbols.intern_id('O', 1); // Avoid Obstacle
    let o2_cruise = agent.symbols.intern_id('O', 2); // Maintain Course

    // Rule 1: Propose Avoid Obstacle (O1) when obstacle detected
    agent.add_rule(
        "propose-avoid",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_input),
                val: Some(val_obstacle),
            },
            vec![],
        )],
        vec![
            Action::Prefer(Preference {
                state: s1,
                operator: o1_avoid,
                preference_type: PreferenceType::Acceptable,
            }),
            Action::Prefer(Preference {
                state: s1,
                operator: o1_avoid,
                preference_type: PreferenceType::Better(o2_cruise),
            }),
        ],
    );

    // Rule 2: Propose Maintain Course (O2)
    agent.add_rule(
        "propose-cruise",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_input),
                val: Some(val_obstacle),
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: o2_cruise,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    println!("=== SOAR PREFERENCE EVALUATION TEST ===");

    // Ingest state WME: (S1 ^sensor-input obstacle-detected)
    agent.insert_wme(s1, attr_input, val_obstacle);

    // 1. Run Proposal/Elaboration Phase
    let fired = agent.run_elaboration_phase();
    println!("Elaboration Complete. Rules Fired: {}", fired);
    println!("Total Preferences Asserted: {}", agent.preferences.len());

    // 2. Run Decision Phase
    let decision = agent.run_decision_phase(s1);

    match decision {
        DecisionResult::Selected(op_id) => {
            println!("Decision Result: Selected Operator {:?}", op_id);
            assert_eq!(op_id, o1_avoid, "O1 must be selected over O2 via Better preference");
        }
        other => panic!("Unexpected decision result: {:?}", other),
    }

    println!("\nPreference Resolution Engine: VERIFIED SUCCESS!");
}