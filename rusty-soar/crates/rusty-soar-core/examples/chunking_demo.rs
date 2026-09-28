use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    // Intern Symbols
    let s1 = agent.symbols.intern_id('S', 1);
    let attr_sensor = agent.symbols.intern_str("sensor");
    let val_ambiguous = agent.symbols.intern_str("ambiguous-target");

    let o1_track = agent.symbols.intern_id('O', 1);
    let o2_classify = agent.symbols.intern_id('O', 2);

    let attr_impasse = agent.symbols.intern_str("impasse");
    let val_tie = agent.symbols.intern_str("tie");

    // Superstate Rule 1: Propose Track (O1)
    agent.add_rule(
        "propose-track",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_sensor),
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

    // Superstate Rule 2: Propose Classify (O2) -> Triggers Tie Impasse
    agent.add_rule(
        "propose-classify",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_sensor),
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

    // Substate Resolving Rule: Evaluates tie impasse and prefers Track (O1) over Classify (O2) on S1
    agent.add_rule(
        "resolve-tie-in-substate",
        vec![(
            AlphaTest {
                id: None,
                attr: Some(attr_impasse),
                val: Some(val_tie),
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: o1_track,
            preference_type: PreferenceType::Better(o2_classify),
        })],
    );

    println!("=== SOAR EXPLANATION-BASED LEARNING (CHUNKING) TEST ===");

    // Cycle 1: Seed WME causing Tie Impasse
    agent.insert_wme(s1, attr_sensor, val_ambiguous);
    let d1 = agent.run_decision_cycle(s1);

    match d1 {
        DecisionResult::TieImpasse(_) => {
            println!("Cycle 1 Decision: Tie Impasse created substate S2 successfully.");
        }
        _ => panic!("Expected Tie Impasse on Cycle 1"),
    }

    assert_eq!(agent.substates.len(), 1, "Substate S2 should exist");

    // Cycle 2: Substate rule fires, resolves tie for S1, and synthesizes CHUNK
    let d2 = agent.run_decision_cycle(s1);

    match d2 {
        DecisionResult::Selected(op) => {
            println!("Cycle 2 Decision: Selected Operator {:?}", op);
            assert_eq!(op, o1_track);
        }
        _ => panic!("Expected O1 selection on Cycle 2"),
    }

    println!("\nChunks Learned Count: {}", agent.chunks_learned);
    assert_eq!(
        agent.chunks_learned, 1,
        "Agent should have synthesized exactly 1 Chunk"
    );

    println!("\nExplanation-Based Learning (Chunking): VERIFIED SUCCESS!");
}
