//! The agent from the documentation's Getting Started guide: it reads an altitude from the
//! input link and writes a `climb` or `hold` command to the output link.
//!
//! The `--8<--` markers let the documentation include parts of this file, so the code shown
//! there is the code that compiles here.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;

// --8<-- [start:rules]
const RULES: &str = r#"
sp {propose*climb
   (state <s> ^io.input-link.altitude < 1000)
-->
   (<s> ^operator <o> + =)
   (<o> ^name climb)}

sp {propose*hold
   (state <s> ^io.input-link.altitude >= 1000)
-->
   (<s> ^operator <o> + =)
   (<o> ^name hold)}

sp {apply*command
   (state <s> ^operator.name <command> ^io.output-link <out>)
-->
   (<out> ^command <command>)}

sp {apply*command*remove-old
   (state <s> ^operator.name <command> ^io.output-link <out>)
   (<out> ^command { <old> <> <command> })
-->
   (<out> ^command <old> -)}
"#;
// --8<-- [end:rules]

fn main() {
    // --8<-- [start:create]
    let mut agent = SoarAgent::new();
    let (state, input_link) = agent.create_top_state();
    let script = SoarScript::parse(RULES).expect("the rules should parse");
    agent.install_soar_script(&script, state).expect("the rules should install");
    // --8<-- [end:create]

    // --8<-- [start:loop]
    let altitude = agent.symbols.intern_str("altitude");
    let command = agent.symbols.intern_str("command");
    let mut reading = None;

    for feet in [500, 800, 1200] {
        // Input: replace last cycle's altitude with the new one.
        if let Some(old) = reading.take() {
            agent.remove_input_wme(old);
        }
        let value = agent.symbols.intern_int(feet);
        reading = Some(agent.add_input_wme(input_link, altitude, value));

        // One decision cycle: input, propose, decide, apply, output.
        agent.run_decision_cycle(state);

        // Output: what the agent changed on its output link this cycle.
        for event in agent.take_output_events() {
            for wme in event.wmes.iter().filter(|w| w.attr == command) {
                println!("altitude {feet} -> {}", symbol_to_string(&agent.symbols, wme.value, false));
            }
        }
    }
    // --8<-- [end:loop]
}
