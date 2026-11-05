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

/// Upstream `testTemplateVariableNameBug` (test1121): the template's rules fire in the cycle the
/// template does, so the agent halts after one decision and four elaboration cycles. Of its four
/// matches, two give the same rule as another once identifiers are variables again (the
/// operator's `^thing` either is the `^anything` matched or isn't), and upstream drops those as
/// duplicates.
#[test]
fn template_rules_fire_in_the_cycle_that_makes_them() {
    let script = rusty_soar_core::soar_parser::SoarScript::parse(include_str!("agents/rl-template-test1121.soar")).unwrap();
    let mut agent = rusty_soar_core::agent::SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&script, state).unwrap();
    for _ in 0..3 {
        agent.run_decision_cycle(state);
        if agent.halted {
            break;
        }
    }
    assert!(agent.halted, "{}", agent.output);
    assert_eq!((agent.decide_phases_count, agent.e_cycle_count), (1, 4));
    for n in 1..=2 {
        assert_eq!(agent.rl.rule_value(&format!("rl*c*{n}")), Some(0.0), "rl*c*{n}");
    }
    assert_eq!(agent.rl.rule_value("rl*c*3"), None);
}

/// The two-armed bandit of `examples/rl_demo.rs`, with a template instead of one rule per arm,
/// learns the same values.
#[test]
fn a_template_learns_as_its_rules_would() {
    let source = r#"
sp {propose*init (state <s> ^superstate nil -^round) --> (<s> ^operator <o> +) (<o> ^name init)}
sp {apply*init (state <s> ^operator.name init) --> (<s> ^round 0)}
sp {propose*arm (state <s> ^superstate nil ^round <r>) --> (<s> ^operator <l> + ^operator <rt> +) (<l> ^name left) (<rt> ^name right)}
sp {arm :template (state <s> ^operator <o> +) (<o> ^name { <arm> << left right >> }) --> (<s> ^operator <o> = 0)}
sp {apply*arm (state <s> ^operator <o> ^round <r>) (<o> ^name { <arm> << left right >> }) --> (<s> ^round <r> - ^round (+ <r> 1)) (<s> ^last-arm <arm>)}
sp {apply*arm*forget-last (state <s> ^operator.name <arm> ^last-arm { <old> <> <arm> }) --> (<s> ^last-arm <old> -)}
sp {reward*left (state <s> ^reward-link <rl> ^last-arm left) --> (<rl> ^reward <w>) (<w> ^value 1)}
sp {reward*right (state <s> ^reward-link <rl> ^last-arm right) --> (<rl> ^reward <w>) (<w> ^value -1)}
"#;
    let script = rusty_soar_core::soar_parser::SoarScript::parse(source).unwrap();
    let mut agent = rusty_soar_core::agent::SoarAgent::new();
    agent.rl.params.learning = true;
    agent.exploration.policy = rusty_soar_core::preference::ExplorationPolicy::EpsilonGreedy;
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&script, state).unwrap();
    for _ in 0..100 {
        agent.run_decision_cycle(state);
    }
    let mut learned: Vec<(String, f64)> = (1..=2)
        .map(|n| format!("rl*arm*{n}"))
        .map(|name| {
            let count = agent.rl.rule_update_count(&name).unwrap();
            (format!("{:.4} ({} updates)", agent.rl.rule_value(&name).unwrap(), count), count)
        })
        .collect();
    learned.sort_by(|a, b| b.1.total_cmp(&a.1));
    assert_eq!(learned.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["8.0533 (96 updates)", "1.2524 (2 updates)"]);
}

#[test]
fn gq_lambda_corrects_the_expected_future_reward() {
    for policy in ["on-policy-gq-lambda", "off-policy-gq-lambda"] {
        let source = format!("rl --set learning-policy {policy}\n{ONE_ACTION}");
        // Decision 3 is as SARSA's (ecr 0.3, efr 0), and sets the rule's gradient weight to
        // 0 + 1.0 * (0.3 + 0) = 0.3.
        let agent = run(&source, true, 3);
        assert!((agent.rl.rule_value("rl*left").unwrap() - 0.3).abs() < 1e-12, "{policy}");
        // Decision 4 updates as SARSA's (ecr 0.51, efr 0.081), then the operator now proposed
        // corrects its rule's efr by 0.3 * 0.9 * (1 - 0) * (0.3 * 1) = 0.081, so the value is 0.51.
        let agent = run(&source, true, 4);
        assert_eq!(agent.rl.rule_update_count("rl*left"), Some(2.0), "{policy}");
        assert!((agent.rl.rule_value("rl*left").unwrap() - 0.51).abs() < 1e-12, "{policy}: {:?}", agent.rl.rule_value("rl*left"));
    }
}
