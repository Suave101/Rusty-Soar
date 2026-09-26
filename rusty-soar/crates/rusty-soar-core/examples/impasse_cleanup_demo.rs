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

    // Superstate Rule 2: Propose Classify (O2)
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

    // Substate Rule: Resolves tie in favor of Track (O1)
    agent.add_rule(
        "resolve-tie",
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

    println!("=== SOAR AUTOMATED SUBSTATE CLEANUP DEMO ===");

    // Cycle 1: Trigger impasse and substate S2 creation
    agent.insert_wme(s1, attr_sensor, val_ambiguous);
    let d1 = agent.run_decision_cycle(s1);

    assert!(matches!(d1, DecisionResult::TieImpasse(_)));
    assert_eq!(agent.substates.len(), 1);
    let initial_wm_count = agent.wm.len();
    println!("Cycle 1: Tie Impasse created substate. Total WMEs: {}", initial_wm_count);

    // Cycle 2: Substate resolves impasse -> S1 selects O1 -> Substate S2 is popped & cleaned up!
    let d2 = agent.run_decision_cycle(s1);

    match d2 {
        DecisionResult::Selected(op) => {
            println!("Cycle 2: Operator {:?} selected!", op);
            assert_eq!(op, o1_track);
        }
        _ => panic!("Expected operator selection on Cycle 2"),
    }

    println!("\nActive Substates Remaining: {}", agent.substates.len());
    assert_eq!(agent.substates.len(), 0, "Substate record list must be empty");

    let final_wm_count = agent.wm.len();
    println!("Working Memory WME count post-cleanup: {}", final_wm_count);
    assert!(
        final_wm_count < initial_wm_count,
        "Substate WMEs must be purged from Working Memory"
    );

    println!("\nSubstate Purging & Impasse Cleanup: VERIFIED SUCCESS!");
}