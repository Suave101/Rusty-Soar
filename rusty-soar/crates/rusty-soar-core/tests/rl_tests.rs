//! Reinforcement learning against upstream `reinforcement_learning.cpp`, with expected values
//! worked by hand from upstream's update.

use rusty_soar_core::agent::SoarAgent;
use rusty_soar_core::rl::LearningPolicy;
use rusty_soar_core::soar_parser::SoarScript;

/// One `left` operator per round, rewarded with 1 on the top state's reward link from the
/// round after it is first applied.
const ONE_ACTION: &str = r#"
sp {propose*init (state <s> ^superstate nil -^round) --> (<s> ^operator <o> +) (<o> ^name init)}
sp {apply*init (state <s> ^operator.name init) --> (<s> ^round 0)}
sp {propose*left (state <s> ^superstate nil ^round <r>) --> (<s> ^operator <o> +) (<o> ^name left)}
sp {rl*left (state <s> ^operator <o> +) (<o> ^name left) --> (<s> ^operator <o> = 0)}
sp {apply*left
   (state <s> ^operator.name left ^round <r>)
-->
   (<s> ^round <r> - ^round (+ <r> 1) ^last left)}
sp {reward*left (state <s> ^reward-link <rl> ^last left) --> (<rl> ^reward <w>) (<w> ^value 1)}
"#;

fn run(source: &str, learning: bool, decisions: usize) -> SoarAgent {
    let mut agent = SoarAgent::new();
    agent.rl.params.learning = learning;
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(source).unwrap(), state).unwrap();
    for _ in 0..decisions {
        agent.run_decision_cycle(state);
    }
    agent
}

#[test]
fn only_rules_whose_single_action_is_a_numeric_preference_are_rl_rules() {
    let agent = run(ONE_ACTION, false, 0);
    assert_eq!(agent.rl.rule_value("rl*left"), Some(0.0));
    assert_eq!(agent.rl.rule_value("propose*left"), None);
}

#[test]
fn learning_off_leaves_values_unchanged() {
    let agent = run(ONE_ACTION, false, 6);
    assert_eq!(agent.rl.rule_value("rl*left"), Some(0.0));
    assert_eq!(agent.rl.rule_update_count("rl*left"), Some(0.0));
}

#[test]
fn sarsa_updates_match_upstream_arithmetic() {
    // Decision 2 selects the first `left`; decision 3 credits it with reward 1 and the next
    // operator's value 0: ecr = 0.3 * (1 - 0) = 0.3, efr = 0.3 * (0.9 * 0 - 0) = 0.
    let agent = run(ONE_ACTION, true, 3);
    assert_eq!(agent.rl.rule_update_count("rl*left"), Some(1.0));
    assert!((agent.rl.rule_value("rl*left").unwrap() - 0.3).abs() < 1e-12);

    // Decision 4: reward 1, next value 0.3, old ecr 0.3 and efr 0:
    // ecr = 0.3 + 0.3 * (1 - 0.3) = 0.51, efr = 0 + 0.3 * (0.9 * 0.3 - 0) = 0.081.
    let agent = run(ONE_ACTION, true, 4);
    assert_eq!(agent.rl.rule_update_count("rl*left"), Some(2.0));
    assert!((agent.rl.rule_value("rl*left").unwrap() - 0.591).abs() < 1e-12);
}

#[test]
fn q_learning_matches_sarsa_with_a_single_candidate() {
    let sarsa = run(ONE_ACTION, true, 5);
    let mut agent = SoarAgent::new();
    agent.rl.params.learning = true;
    agent.rl.params.learning_policy = LearningPolicy::QLearning;
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(ONE_ACTION).unwrap(), state).unwrap();
    for _ in 0..5 {
        agent.run_decision_cycle(state);
    }
    assert_eq!(agent.rl.rule_value("rl*left"), sarsa.rl.rule_value("rl*left"));
}
