//! Input and output links, against upstream `io_link.cpp` semantics.

use rusty_soar_core::agent::{OutputMode, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;
use rusty_soar_core::symbol::SymbolId;

fn agent_with(source: &str) -> (SoarAgent, SymbolId, SymbolId) {
    let mut agent = SoarAgent::new();
    let (state, input_link) = agent.create_top_state();
    let script = SoarScript::parse(source).expect("parses");
    agent.install_soar_script(&script, state).expect("installs");
    (agent, state, input_link)
}

const ECHO: &str = r#"
sp {echo
   (state <s> ^io <io>)
   (<io> ^input-link.speed <v> ^output-link <ol>)
-->
   (<ol> ^echo <v>)}
"#;

#[test]
fn creating_the_agent_reports_no_output() {
    let (mut agent, _, input_link) = agent_with(ECHO);
    assert_eq!(agent.input_link(), Some(input_link));
    assert!(agent.output_link().is_some());
    assert!(agent.take_output_events().is_empty());
}

#[test]
fn output_link_changes_are_reported_in_the_output_phase() {
    let (mut agent, state, input_link) = agent_with(ECHO);
    let speed = agent.symbols.intern_str("speed");
    let value = agent.symbols.intern_int(250);
    let input = agent.add_input_wme(input_link, speed, value);

    agent.run_decision_cycle(state);
    assert!(agent.output_link_changed());
    let events = agent.take_output_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].mode, OutputMode::Modified);
    let echo = agent.symbols.intern_str("echo");
    let output_link = agent.output_link().unwrap();
    assert!(events[0].wmes.iter().any(|w| w.id == output_link && w.attr == echo && w.value == value));
    // The link WME itself is reported last.
    let output_attr = agent.symbols.intern_str("output-link");
    let last = events[0].wmes.last().unwrap();
    assert_eq!((last.attr, last.value), (output_attr, output_link));

    agent.run_decision_cycle(state);
    assert!(agent.take_output_events().is_empty(), "no change, no report");
    assert!(!agent.output_link_changed());

    // Removing the input retracts the i-supported echo: reported, but a removal does not count
    // as the agent changing its output (upstream only sets the flag for additions).
    assert!(agent.remove_input_wme(input));
    agent.run_decision_cycle(state);
    let events = agent.take_output_events();
    assert_eq!(events.len(), 1);
    assert!(events[0].wmes.iter().all(|w| w.attr != echo));
    assert!(!agent.output_link_changed());
}

#[test]
fn removing_a_wme_that_is_not_an_input_wme_fails() {
    let (mut agent, state, _) = agent_with(ECHO);
    let attr = agent.symbols.intern_str("note");
    let value = agent.symbols.intern_str("x");
    let key = agent.insert_wme(state, attr, value);
    assert!(!agent.remove_input_wme(key));
    assert!(agent.output.contains("isn't one of the input wmes"));
    assert!(agent.wm.get(key).is_some());
}

#[test]
fn identical_input_wmes_are_distinct() {
    let (mut agent, _, input_link) = agent_with(ECHO);
    let attr = agent.symbols.intern_str("speed");
    let value = agent.symbols.intern_int(1);
    let first = agent.add_input_wme(input_link, attr, value);
    let second = agent.add_input_wme(input_link, attr, value);
    assert_ne!(first, second);
    assert!(agent.remove_input_wme(first));
    assert!(agent.wm.get(second).is_some());
}
