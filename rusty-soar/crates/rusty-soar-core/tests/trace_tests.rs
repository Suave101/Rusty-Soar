//! Upstream's default decision trace and symbol printing, which the differential test against
//! upstream Soar compares.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;

fn run_traced(source: &str, decisions: usize) -> String {
    let script = SoarScript::parse(source).expect("parses");
    let mut agent = SoarAgent::new();
    agent.trace_decisions = true;
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&script, state).expect("installs");
    for _ in 0..decisions {
        agent.run_decision_cycle(state);
    }
    agent.output
}

#[test]
fn trace_lines_follow_upstream_stack_trace_formats() {
    let output = run_traced(
        r#"
        sp {propose*a (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name a)}
        sp {propose*b (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name b)}
        sp {tie*prefer-b
           (state <s> ^impasse tie ^superstate <ss> ^item <o>) (<o> ^name b)
        -->
           (<ss> ^operator <o> >)}
        sp {apply*b (state <s> ^operator.name b) --> (<s> ^done yes) (write (crlf) |applied b|)}
        "#,
        4,
    );
    assert_eq!(
        output,
        "\n     0: ==>S: S1 \
         \n     1:    ==>S: S2 (operator tie)\
         \n     2:    O: O1 (b)\
         \napplied b\
         \n     3:    ==>S: S3 (state no-change)\
         \n     4:       ==>S: S4 (state no-change)"
    );
}

#[test]
fn symbols_print_as_upstream_to_string() {
    let mut agent = SoarAgent::new();
    let mut text = |s: &str| {
        let symbol = agent.symbols.intern_str(s);
        symbol_to_string(&agent.symbols, symbol, true)
    };
    assert_eq!(text("hello"), "hello");
    assert_eq!(text("a"), "a");
    assert_eq!(text("go-left"), "go-left");
    // Short non-alphanumeric strings could be special lexemes, so they are barred.
    assert_eq!(text("a-b"), "|a-b|");
    assert_eq!(text("*"), "*");
    assert_eq!(text("Hello world"), "|Hello world|");
    assert_eq!(text("<x>"), "|<x>|");
    assert_eq!(text("12"), "|12|");
    assert_eq!(text("S1"), "|S1|");
    assert_eq!(text("a|b"), "|a\\|b|");

    let half = agent.symbols.intern_float(1.5);
    assert_eq!(symbol_to_string(&agent.symbols, half, false), "1.500000");
    let s = agent.symbols.intern_str("Hello world");
    assert_eq!(symbol_to_string(&agent.symbols, s, false), "Hello world");
}
