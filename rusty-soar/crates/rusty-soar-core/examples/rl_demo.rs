use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let o1_left = agent.symbols.intern_id('O', 1);
    let o2_right = agent.symbols.intern_id('O', 2);

    let attr_state = agent.symbols.intern_str("type");
    let val_state = agent.symbols.intern_str("state");

    // Rules proposing operators
    agent.add_rule(
        "propose-left",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_state),
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
                attr: Some(attr_state),
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

    println!("=== SOAR REINFORCEMENT LEARNING (RL) DEMO ===");

    agent.insert_wme(s1, attr_state, val_state);

    // Initial Q-values are 0.0
    println!("Initial Q(S1, O_left)  = {}", agent.rl.get_q_value(s1, o1_left));
    println!("Initial Q(S1, O_right) = {}", agent.rl.get_q_value(s1, o2_right));

    // Agent executes O_left and receives a +10.0 environmental reward signal
    agent.rl.add_reward(10.0);
    let new_q = agent.rl.update_q_value(s1, o1_left, 0.0);
    println!("\nReceived +10.0 reward for O_left!");
    println!("Updated Q(S1, O_left) = {:.2}", new_q);

    // Agent executes O_right and receives a -5.0 penalty reward signal
    agent.rl.add_reward(-5.0);
    let new_q_right = agent.rl.update_q_value(s1, o2_right, 0.0);
    println!("\nReceived -5.0 reward for O_right!");
    println!("Updated Q(S1, O_right) = {:.2}", new_q_right);

    assert!(agent.rl.get_q_value(s1, o1_left) > agent.rl.get_q_value(s1, o2_right));

    println!("\nReinforcement Learning Q-Table Updates: VERIFIED SUCCESS!");
}