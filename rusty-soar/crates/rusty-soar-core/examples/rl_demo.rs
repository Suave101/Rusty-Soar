//! Reinforcement learning on a two-armed bandit: each round the agent chooses `left`
//! (reward +1) or `right` (reward -1) and learns the value of each choice.

use rusty_soar_core::agent::SoarAgent;
use rusty_soar_core::preference::ExplorationPolicy;
use rusty_soar_core::soar_parser::SoarScript;

const BANDIT: &str = r#"
sp {propose*init (state <s> ^superstate nil -^round) --> (<s> ^operator <o> +) (<o> ^name init)}
sp {apply*init (state <s> ^operator.name init) --> (<s> ^round 0)}

sp {propose*arm
   (state <s> ^superstate nil ^round <r>)
-->
   (<s> ^operator <l> + ^operator <rt> +)
   (<l> ^name left)
   (<rt> ^name right)}

sp {rl*left (state <s> ^operator <o> +) (<o> ^name left) --> (<s> ^operator <o> = 0)}
sp {rl*right (state <s> ^operator <o> +) (<o> ^name right) --> (<s> ^operator <o> = 0)}

sp {apply*arm
   (state <s> ^operator <o> ^round <r>)
   (<o> ^name { <arm> << left right >> })
-->
   (<s> ^round <r> - ^round (+ <r> 1))
   (<s> ^last-arm <arm>)}

sp {apply*arm*forget-last
   (state <s> ^operator.name <arm> ^last-arm { <old> <> <arm> })
-->
   (<s> ^last-arm <old> -)}

sp {reward*left (state <s> ^reward-link <rl> ^last-arm left) --> (<rl> ^reward <w>) (<w> ^value 1)}
sp {reward*right (state <s> ^reward-link <rl> ^last-arm right) --> (<rl> ^reward <w>) (<w> ^value -1)}
"#;

fn main() {
    let mut agent = SoarAgent::new();
    agent.rl.params.learning = true;
    agent.exploration.policy = ExplorationPolicy::EpsilonGreedy;
    let (state, _) = agent.create_top_state();
    let script = SoarScript::parse(BANDIT).expect("parses");
    agent.install_soar_script(&script, state).expect("installs");

    for _ in 0..100 {
        agent.run_decision_cycle(state);
    }
    let left = agent.rl.rule_value("rl*left").unwrap();
    let right = agent.rl.rule_value("rl*right").unwrap();
    println!("learned value of left:  {left:.4} ({} updates)", agent.rl.rule_update_count("rl*left").unwrap());
    println!("learned value of right: {right:.4} ({} updates)", agent.rl.rule_update_count("rl*right").unwrap());
    assert!(left > right, "the agent should learn that left is better");
}
