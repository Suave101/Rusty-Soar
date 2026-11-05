//! Episodic memory through its command interface: an agent walks through three sectors, and a
//! query then asks when it was in the second one.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;

const AGENT: &str = r#"
epmem --set learning on

sp {propose*start (state <s> ^superstate nil -^sector) --> (<s> ^operator <o> + =) (<o> ^name move ^to alpha)}
sp {propose*beta (state <s> ^sector alpha) --> (<s> ^operator <o> + =) (<o> ^name move ^to beta)}
sp {propose*gamma (state <s> ^sector beta) --> (<s> ^operator <o> + =) (<o> ^name move ^to gamma)}
sp {apply*move (state <s> ^operator <o>) (<o> ^name move ^to <t>) --> (<s> ^sector <t>)}
sp {apply*move*leave (state <s> ^operator <o> ^sector <old>) (<o> ^name move ^to <> <old>) --> (<s> ^sector <old> -)}

sp {ask (state <s> ^sector gamma ^epmem.command <c>) --> (<c> ^query <q>) (<q> ^sector beta)}
"#;

fn main() {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(AGENT).expect("the agent parses"), state).expect("the agent installs");
    for _ in 0..3 {
        agent.run_decision_cycle(state);
    }

    let find = |agent: &SoarAgent, id, attr: &str| {
        let attr = agent.symbols.lookup_str(attr).expect("used above");
        agent.wm.iter().find(|w| w.id == id && w.attr == attr).map(|w| w.val).expect("in working memory")
    };
    let result = find(&agent, find(&agent, state, "epmem"), "result");
    let episode = find(&agent, result, "memory-id");
    let retrieved = find(&agent, result, "retrieved");
    let sector = find(&agent, retrieved, "sector");
    println!("episodes stored: {}", agent.epmem.len());
    println!("the agent was in {} in episode {}", symbol_to_string(&agent.symbols, sector, false), symbol_to_string(&agent.symbols, episode, false));
}
