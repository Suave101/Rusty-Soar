//! Settings commands in Soar scripts: carried out as upstream's CLI does, and any command this
//! port does not support is an error rather than silently skipped.

use rusty_soar_core::agent::SoarAgent;
use rusty_soar_core::preference::{ExplorationPolicy, NumericIndifferentMode};
use rusty_soar_core::rl::{DecayMode, LearningPolicy};
use rusty_soar_core::soar_parser::{ScriptCommand, SoarScript};

fn load(source: &str) -> SoarAgent {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(source).expect("parses"), state).expect("installs");
    agent
}

fn parse_error(source: &str) -> String {
    SoarScript::parse(source).expect_err("should not parse").to_string()
}

#[test]
fn rl_settings_are_applied() {
    let agent = load(
        "rl --set learning on\n\
         rl -s learning-policy q-learning\n\
         rl --set discount-rate 0.5\n\
         rl --set learning-rate 0.1\n\
         rl --set eligibility-trace-decay-rate 0.8\n\
         rl --set decay-mode delta-bar-delta\n\
         rl --set temporal-extension off\n",
    );
    let p = agent.rl.params;
    assert!(p.learning);
    assert_eq!(p.learning_policy, LearningPolicy::QLearning);
    assert_eq!((p.discount_rate, p.learning_rate, p.et_decay_rate), (0.5, 0.1, 0.8));
    assert_eq!(p.decay_mode, DecayMode::DeltaBarDelta);
    assert!(!p.temporal_extension);
}

#[test]
fn exploration_settings_and_aliases_are_applied() {
    let agent = load(
        "decide indifferent-selection --epsilon-greedy\n\
         indifferent-selection -e 0.25\n\
         inds --temperature 3\n\
         numeric-indifferent-mode --avg\n",
    );
    assert_eq!(agent.exploration.policy, ExplorationPolicy::EpsilonGreedy);
    assert_eq!((agent.exploration.epsilon, agent.exploration.temperature), (0.25, 3.0));
    assert_eq!(agent.exploration.numeric_indifferent_mode, NumericIndifferentMode::Avg);
}

#[test]
fn srand_seeds_as_upstream() {
    let script = SoarScript::parse("srand 42\ndecide set-random-seed 7").unwrap();
    let commands: Vec<_> = script.commands.iter().map(|(_, c)| c.clone()).collect();
    assert_eq!(commands, vec![ScriptCommand::SetRandomSeed(42), ScriptCommand::SetRandomSeed(7)]);
}

#[test]
fn commands_keep_their_place_among_productions() {
    let script = SoarScript::parse(
        "echo before\nsp {a (state <s> ^superstate nil) --> (<s> ^x 1)}\necho -n {after a}\nsp {b (state <s> ^superstate nil) --> (<s> ^y 1)}",
    )
    .unwrap();
    assert_eq!(script.commands[0].0, 0);
    assert_eq!(script.commands[1].0, 1);
    let agent = load("echo before\nsp {a (state <s> ^superstate nil) --> (<s> ^x 1)}\necho -n {after a}");
    assert!(agent.output.contains("before\nafter a"));
}

#[test]
fn unsupported_commands_are_errors() {
    for command in ["learn --on", "epmem --set learning on", "multi-attributes foo 3", "srand", "rl --set trace on"] {
        let error = parse_error(command);
        assert!(error.contains(command.split_whitespace().next().unwrap()), "{command}: {error}");
    }
    assert!(parse_error("rl --set learning-policy on-policy-gq-lambda").contains("not supported"));
}

#[test]
fn invalid_values_are_errors_as_upstream() {
    assert!(parse_error("rl --set learning yes").contains("on or off"));
    assert!(parse_error("rl --set discount-rate 1.5").contains("from 0 to 1"));
    assert!(parse_error("rl --set eligibility-trace-tolerance 0").contains("above 0"));
    assert!(parse_error("indifferent-selection --epsilon 2").contains("epsilon"));
    assert!(parse_error("indifferent-selection --temperature 0").contains("temperature"));
}

#[test]
fn learning_can_be_turned_off() {
    let mut agent = load("rl --set learning on");
    agent.set_rl(rusty_soar_core::rl::RlSetting::Learning(false));
    assert!(!agent.rl.params.learning);
}

#[test]
fn rule_flags_that_change_how_a_rule_runs_are_rejected() {
    for (flag, message) in [(":template", "template"), (":interrupt", "interrupt")] {
        let script = SoarScript::parse(&format!("sp {{p {flag} (state <s> ^superstate nil) --> (<s> ^x 1)}}")).unwrap();
        let mut agent = SoarAgent::new();
        let (state, _) = agent.create_top_state();
        let error = agent.install_soar_script(&script, state).expect_err("should not install");
        assert!(error.to_string().contains(message), "{flag}: {error}");
    }
}
