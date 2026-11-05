//! Semantic memory through its command interface: one operator stores a structure in long-term
//! memory, the next forgets it from working memory and retrieves it again.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;

const AGENT: &str = r#"
smem --set learning on

sp {propose*store (state <s> ^superstate nil -^rover) --> (<s> ^operator <o> +) (<o> ^name store)}
sp {apply*store
    (state <s> ^operator.name store ^smem.command <c>)
-->
    (<s> ^rover <r>)
    (<r> ^name curiosity ^category autonomous-vehicle ^destination mars-crater)
    (<c> ^store <r>)}

sp {propose*forget (state <s> ^rover <r> ^smem.result.success <r> -^forgotten) --> (<s> ^operator <o> +) (<o> ^name forget)}
sp {apply*forget
    (state <s> ^operator.name forget ^rover <r> ^smem.command <c>)
    (<r> ^name <n> ^category <k> ^destination <d>)
    (<c> ^store <r>)
-->
    (<s> ^forgotten yes)
    (<r> ^name <n> - ^category <k> - ^destination <d> -)
    (<c> ^store <r> - ^retrieve <r>)}
"#;

fn main() {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(AGENT).expect("the agent parses"), state).expect("the agent installs");

    let rover_attr = agent.symbols.lookup_str("rover").expect("used by the rules");
    let print_rover = |agent: &SoarAgent, when: &str| {
        let rover = agent.wm.iter().find(|w| w.id == state && w.attr == rover_attr).map(|w| w.val);
        let Some(rover) = rover else { return };
        let mut facts: Vec<String> = agent
            .wm
            .iter()
            .filter(|w| w.id == rover)
            .map(|w| format!("^{} {}", symbol_to_string(&agent.symbols, w.attr, false), symbol_to_string(&agent.symbols, w.val, false)))
            .collect();
        facts.sort();
        println!("{when}: ({} {})", symbol_to_string(&agent.symbols, rover, false), facts.join(" "));
    };

    agent.run_decision_cycle(state);
    agent.run_decision_cycle(state);
    print_rover(&agent, "stored");
    agent.run_decision_cycle(state);
    agent.run_decision_cycle(state);
    print_rover(&agent, "forgotten, then retrieved");
    println!("long-term identifiers: {}, stores: {}, retrievals: {}", agent.smem.len(), agent.smem.stats.stores, agent.smem.stats.retrievals);
}
