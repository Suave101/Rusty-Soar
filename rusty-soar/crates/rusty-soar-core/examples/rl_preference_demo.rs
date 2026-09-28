use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{DecisionResult, Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let o1_left = agent.symbols.intern_id('O', 1);
    let o2_right = agent.symbols.intern_id('O', 2);

    let attr_type = agent.symbols.intern_str("type");
    let val_state = agent.symbols.intern_str("state");

    // Rules proposing both operators
    agent.add_rule(
        "propose-left",
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
            operator: o1_left,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    agent.add_rule(
        "propose-right",
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
            operator: o2_right,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    println!("=== RUSTY-SOAR V1.0 RL PREFERENCE INTEGRATION DEMO ===");

    agent.insert_wme(s1, attr_type, val_state);

    // Cycle 1: With zero Q-values, decision yields a TieImpasse
    let decision1 = agent.run_decision_cycle(s1);
    println!("Cycle 1 Decision Result (Initial): {:?}", decision1);
    assert!(matches!(decision1, DecisionResult::TieImpasse(_)));

    // Provide environmental reward to O1_left
    agent.rl.add_reward(10.0);
    agent.rl.update_q_value(s1, o1_left, 0.0);
    println!(
        "\nLearned Q(S1, O1_left) = {:.2}",
        agent.rl.get_q_value(s1, o1_left)
    );

    // Cycle 2: Higher Q-value breaks the tie automatically in favor of O1_left!
    let decision2 = agent.run_decision_cycle(s1);
    println!("Cycle 2 Decision Result (RL-Driven): {:?}", decision2);
    assert_eq!(decision2, DecisionResult::Selected(o1_left));

    println!("\nRL-assisted preference tie resolution: VERIFIED SUCCESS!");
}
