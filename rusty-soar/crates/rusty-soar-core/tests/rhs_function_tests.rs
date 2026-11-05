//! Built-in RHS functions, against upstream's `BuiltinRHSTests` and values worked out from
//! upstream's code.

use rusty_soar_core::agent::{symbol_to_string, Phase, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;
use rusty_soar_core::symbol::SymbolId;

fn agent_with(source: &str) -> (SoarAgent, SymbolId) {
    let script = SoarScript::parse(source).expect("parses");
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&script, state).expect("installs");
    (agent, state)
}

fn values(agent: &mut SoarAgent, id: SymbolId, attr: &str) -> Vec<String> {
    let attr = agent.symbols.intern_str(attr);
    let mut found: Vec<String> =
        agent.wm.iter().filter(|w| w.id == id && w.attr == attr).map(|w| symbol_to_string(&agent.symbols, w.val, false)).collect();
    found.sort();
    found
}

fn value(agent: &mut SoarAgent, id: SymbolId, attr: &str) -> String {
    let found = values(agent, id, attr);
    assert_eq!(found.len(), 1, "^{attr}: {found:?}\n{}", agent.output);
    found[0].clone()
}

/// Upstream `testString`: halts in the first propose phase, before any decision.
#[test]
fn string_round_trips_floats_and_ints() {
    let source = include_str!("agents/rhs-testString.soar").replace("(succeeded)", "(halt)");
    let (mut agent, state) = agent_with(&source);
    agent.run_decision_cycle(state);
    assert!(agent.halted && agent.current_phase == Phase::Proposal, "{}", agent.output);
    assert_eq!(value(&mut agent, state, "sup-string"), "nil");
}

#[test]
fn value_functions_give_upstreams_results() {
    let (mut agent, s) = agent_with(
        r#"
sp {make (state <s> ^superstate nil) -->
   (<s> ^concat (concat a 1 |b c|) ^min (min 3 2.5 4) ^max (max 1 2) ^sqrt (sqrt 16)
        ^int-string (int |3.7|) ^int-word (int |abc|) ^float-string (float |2.5x|)
        ^capitalized (capitalize-symbol foo) ^trimmed (trim |  hi |) ^strlen (strlen hello)
        ^round (round-off 23 5) ^heading-round (round-off-heading 359 10)
        ^range (compute-range 0 0 3 4) ^north (compute-heading 0 0 0 10) ^east (compute-heading 0 0 10 0)
        ^dice (compute-dice-probability 2 6 1 ge) ^abs (abs -3) ^cos (cos 0) ^atan2 (atan2 0 1)
        ^x (extrapolate-x-position 0 90 2 5) ^y (extrapolate-y-position 0 0 2 5))}
"#,
    );
    agent.run_decision_cycle(s);
    let expected = [
        ("concat", "a1b c"),
        ("min", "2.500000"),
        ("max", "2"),
        ("sqrt", "4.000000"),
        ("int-string", "3"),
        ("int-word", "0"),
        ("float-string", "2.500000"),
        ("capitalized", "Foo"),
        ("trimmed", "hi"),
        ("strlen", "5"),
        ("round", "25"),
        ("heading-round", "0"),
        ("range", "5"),
        ("north", "0"),
        ("east", "90"),
        ("dice", "0.305556"),
        ("abs", "3"),
        ("cos", "1.000000"),
        ("atan2", "0.000000"),
        ("x", "10"),
        ("y", "-10"),
    ];
    for (attr, want) in expected {
        assert_eq!(value(&mut agent, s, attr), want, "^{attr}");
    }
}

#[test]
fn set_functions_reduce_a_multi_valued_attribute() {
    let (mut agent, s) = agent_with(
        r#"
sp {init (state <s> ^superstate nil) --> (<s> ^set <set>) (<set> ^item 1 2 3 ^item word)}
sp {reduce (state <s> ^set <set>) (<set> ^item 3) -->
   (<s> ^count (set-count <set> item) ^sum (set-sum <set> item) ^mean (set-mean <set> item)
        ^range (set-range <set> item) ^min (set-min <set> item) ^max (set-max <set> item)
        ^stdev (set-stdev <set> item) ^product (set-multiply <set> item) ^size (size <set>)
        ^none (set-sum <set> missing) ^bad (set-count 3 item))}
"#,
    );
    agent.run_decision_cycle(s);
    let expected = [
        ("count", "4"),
        ("sum", "6.000000"),
        ("mean", "2.000000"),
        ("range", "2.000000"),
        ("min", "1.000000"),
        ("max", "3.000000"),
        ("stdev", "0.816497"),
        ("product", "6.000000"),
        ("size", "4"),
        ("none", "NaN"),
        ("bad", "|ERROR: First parameter needs to be an identifier.|"),
    ];
    for (attr, want) in expected {
        assert_eq!(value(&mut agent, s, attr), want, "^{attr}");
    }
}

#[test]
fn errors_are_written_and_the_action_skipped() {
    let (mut agent, s) = agent_with(
        "sp {bad (state <s> ^superstate nil) --> (<s> ^a (+ 1 x) ^b (div 1 0) ^c (sin x) ^d (trim 3))}",
    );
    agent.run_decision_cycle(s);
    for attr in ["a", "b", "c", "d"] {
        assert!(values(&mut agent, s, attr).is_empty(), "^{attr}");
    }
    for message in [
        "Error: non-number (x) passed to + function\n",
        "Error: attempt to divide ('div') by zero.\n",
        "Error: 'sin' function called with non-numeric argument x\n",
        "Error: non-symbol (3) passed to 'trim' function.\n",
    ] {
        assert!(agent.output.contains(message), "{message:?} in {:?}", agent.output);
    }
}

#[test]
fn dc_counts_decisions_and_trace_follows_the_trace_level() {
    let (mut agent, s) = agent_with(
        r#"
sp {propose (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name go)}
sp {apply (state <s> ^operator.name go) --> (<s> ^done (dc)) (trace 1 |one|) (trace 2 |two|) (log 1 |log|)}
"#,
    );
    agent.trace_decisions = true;
    agent.run_decision_cycle(s);
    assert_eq!(value(&mut agent, s, "done"), "1");
    assert!(agent.output.contains("one") && !agent.output.contains("two") && agent.output.contains("log"), "{}", agent.output);
}

#[test]
fn rand_int_stays_in_range_and_follows_the_seed() {
    let source = "srand 3\nsp {r (state <s> ^superstate nil) --> (<s> ^i (rand-int 10) ^f (rand-float 2.0))}";
    let (mut a, s) = agent_with(source);
    a.run_decision_cycle(s);
    let i: i64 = value(&mut a, s, "i").parse().unwrap();
    let f: f64 = value(&mut a, s, "f").parse().unwrap();
    assert!((0..=10).contains(&i) && (0.0..=2.0).contains(&f));
    let (mut b, s2) = agent_with(source);
    b.run_decision_cycle(s2);
    assert_eq!(value(&mut b, s2, "i"), i.to_string());
}
