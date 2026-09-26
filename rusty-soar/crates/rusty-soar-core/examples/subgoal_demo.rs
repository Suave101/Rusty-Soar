use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    // Intern Symbols
    let s1 = agent.symbols.intern_id('S', 1);
    let attr_input = agent.symbols.intern_str("sensor");
    let val_ambiguous = agent.symbols.intern_str("ambiguous-target");

    let o1_track = agent.symbols.intern_id('O', 1);
    let o2_classify = agent.symbols.intern_id('O', 2);

    // Rule 1: Propose Track target (O1)
    agent.add_rule(
        "propose-track",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_input),
                val: Some(val_ambiguous),
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: o1_track,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    // Rule 2: Propose Classify target (O2)
    agent.add_rule(
        "propose-classify",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_input),
                val: Some(val_ambiguous),
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: o2_classify,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    println!("=== SOAR SUBGOALING & IMPASSE TEST ===");

    // Insert triggering state WME
    agent.insert_wme(s1, attr_input, val_ambiguous);

    // Run Full 5-Phase Decision Cycle
    let decision = agent.run_decision_cycle(s1);

    match decision {
        DecisionResult::TieImpasse(candidates) => {
            println!("Decision Result: Tie Impasse encountered!");
            println!("Tied Operators Count: {}", candidates.len());
            assert_eq!(candidates.len(), 2);

            // Verify architectural substate creation in Working Memory
            println!("\nActive Substates Created: {}", agent.substates.len());
            let substate = &agent.substates[0];
            println!("Created Substate Symbol: {:?}", substate.substate_id);
            println!("Superstate Parent Symbol: {:?}", substate.superstate_id);
            println!("Impasse Type: {:?}", substate.impasse_type);

            assert_eq!(substate.superstate_id, s1);
        }
        other => panic!("Expected TieImpasse, got: {:?}", other),
    }

    println!("\nWM Total Elements (including substate WMEs): {}", agent.wm.len());
    println!("Subgoaling and Impasse Engine: VERIFIED SUCCESS!");
}