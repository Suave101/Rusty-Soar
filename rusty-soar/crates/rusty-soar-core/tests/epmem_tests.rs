//! Episodic memory through its command interface: episodes are stored each decision, and rules
//! write commands on `^epmem.command` and read results on `^epmem.result`.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;
use rusty_soar_core::symbol::SymbolId;

fn agent_with(source: &str) -> (SoarAgent, SymbolId) {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(source).unwrap(), state).unwrap();
    (agent, state)
}

fn values(agent: &SoarAgent, id: SymbolId, attr: &str) -> Vec<String> {
    let Some(attr) = agent.symbols.lookup_str(attr) else { return Vec::new() };
    let mut values: Vec<String> =
        agent.wm.iter().filter(|w| w.id == id && w.attr == attr && !w.acceptable).map(|w| symbol_to_string(&agent.symbols, w.val, false)).collect();
    values.sort();
    values
}

fn value(agent: &SoarAgent, id: SymbolId, attr: &str) -> SymbolId {
    let symbol = agent.symbols.lookup_str(attr).unwrap();
    let found: Vec<SymbolId> = agent.wm.iter().filter(|w| w.id == id && w.attr == symbol && !w.acceptable).map(|w| w.val).collect();
    assert_eq!(found.len(), 1, "one value of ^{attr}: {:?}", values(agent, id, attr));
    found[0]
}

fn result(agent: &SoarAgent, s: SymbolId) -> SymbolId {
    value(agent, value(agent, s, "epmem"), "result")
}

/// An agent that counts: `^step` goes through the values in `steps`, one per operator. The rules
/// in `extra` run alongside.
fn stepper(steps: &[&str], extra: &str) -> String {
    let mut source = String::from("epmem --set learning on\nsp {init (state <s> ^superstate nil -^step) --> (<s> ^operator <o> + =) (<o> ^name step ^to ");
    source.push_str(steps[0]);
    source.push_str(")}\n");
    for pair in steps.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        source.push_str(&format!("sp {{next*{a} (state <s> ^step {a}) --> (<s> ^operator <o> + =) (<o> ^name step ^to {b})}}\n"));
    }
    source.push_str(
        "sp {apply*step (state <s> ^operator <o>) (<o> ^name step ^to <t>) --> (<s> ^step <t>)}
sp {apply*step*clear (state <s> ^operator <o> ^step <old>) (<o> ^name step ^to <> <old>) --> (<s> ^step <old> -)}
",
    );
    source.push_str(extra);
    source
}

#[test]
fn an_episode_is_stored_each_decision_and_present_id_follows() {
    let (mut agent, s) = agent_with(&stepper(&["a", "b", "c"], ""));
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(agent.epmem.len(), 3);
    assert_eq!(values(&agent, value(&agent, s, "epmem"), "present-id"), vec!["4"]);
}

#[test]
fn nothing_is_stored_while_learning_is_off() {
    let (mut agent, s) = agent_with(&stepper(&["a", "b"], "").replace("epmem --set learning on", ""));
    agent.run_decision_cycle(s);
    agent.run_decision_cycle(s);
    assert_eq!(agent.epmem.len(), 0);
}

/// Runs `stepper(steps)`, asking `ask` once the last step is reached. `^step x` for the n-th value
/// is first stored in episode n, as an operator applies before the episode is stored.
fn query_after(steps: &[&str], ask: &str, extra: &str) -> (SoarAgent, SymbolId) {
    let last = steps[steps.len() - 1];
    let rules = format!("{extra}sp {{ask (state <s> ^step {last} ^epmem.command <c>) --> {ask}}}\n");
    let (mut agent, s) = agent_with(&stepper(steps, &rules));
    for _ in 0..steps.len() + 1 {
        agent.run_decision_cycle(s);
    }
    (agent, s)
}

#[test]
fn retrieve_recalls_an_episode() {
    let (agent, s) = query_after(&["a", "b", "c", "d"], "(<c> ^retrieve 2)", "");
    let r = result(&agent, s);
    assert_eq!(values(&agent, r, "memory-id"), vec!["2"], "{}", agent.output);
    assert_eq!(values(&agent, value(&agent, r, "retrieved"), "step"), vec!["b"]);
    assert_eq!(agent.epmem.stats.ncbr, 1);
}

#[test]
fn next_and_previous_move_from_the_last_retrieval() {
    let rules = r#"
sp {ask (state <s> ^step d -^moved ^epmem.command <c>) --> (<c> ^retrieve 2)}
sp {propose*move (state <s> ^epmem.result.memory-id 2 -^moved) --> (<s> ^operator <o> + >) (<o> ^name move)}
sp {apply*move (state <s> ^operator.name move) --> (<s> ^moved yes)}
"#;
    let run = |direction: &str| {
        let rule = format!("sp {{then (state <s> ^moved yes ^epmem.command <c>) --> (<c> ^{direction} <n>)}}\n");
        let (mut agent, s) = agent_with(&stepper(&["a", "b", "c", "d"], &format!("{rules}{rule}")));
        for _ in 0..7 {
            agent.run_decision_cycle(s);
        }
        let memory = values(&agent, result(&agent, s), "memory-id");
        assert_eq!(values(&agent, result(&agent, s), "success").len(), 1, "{}", agent.output);
        memory
    };
    assert_eq!(run("next"), vec!["3"]);
    assert_eq!(run("previous"), vec!["1"]);
}

#[test]
fn retrieving_an_episode_that_does_not_exist_says_no_memory() {
    let (agent, s) = query_after(&["a", "b"], "(<c> ^retrieve 99)", "");
    assert_eq!(values(&agent, result(&agent, s), "retrieved"), vec!["no-memory"]);
}

#[test]
fn an_unknown_command_is_a_bad_command() {
    let (agent, s) = query_after(&["a"], "(<c> ^frobnicate yes)", "");
    assert_eq!(values(&agent, result(&agent, s), "status"), vec!["bad-cmd"]);
}

#[test]
fn query_finds_the_matching_episode() {
    let (agent, s) = query_after(&["a", "b", "c", "d"], "(<c> ^query <q>) (<q> ^step b)", "");
    let r = result(&agent, s);
    assert_eq!(values(&agent, value(&agent, r, "retrieved"), "step"), vec!["b"], "{}", agent.output);
    assert_eq!(values(&agent, r, "memory-id"), vec!["2"]);
    assert_eq!(values(&agent, r, "cue-size"), vec!["1"]);
    assert_eq!(values(&agent, r, "match-cardinality"), vec!["1"]);
    assert_eq!(values(&agent, r, "graph-match"), vec!["1"]);
    assert_eq!(agent.epmem.stats.cbr, 1);
}

/// `^seen yes` is set by every step after the first, so it spans a range: the most recent episode
/// wins, and `^before` and `^prohibit` move the answer back.
#[test]
fn query_prefers_the_newest_episode_and_honours_before_and_prohibit() {
    let seen = "sp {seen (state <s> ^operator.name step ^step <any>) --> (<s> ^seen yes)}\n";
    let (agent, s) = query_after(&["a", "b", "c", "d", "e"], "(<c> ^query <q>) (<q> ^seen yes)", seen);
    assert_eq!(values(&agent, result(&agent, s), "memory-id"), vec!["5"], "{}", agent.output);

    let (agent, s) = query_after(&["a", "b", "c", "d", "e"], "(<c> ^query <q> ^before 5 ^prohibit 4) (<q> ^seen yes)", seen);
    assert_eq!(values(&agent, result(&agent, s), "memory-id"), vec!["3"], "{}", agent.output);
}

#[test]
fn query_with_no_match_fails() {
    let (agent, s) = query_after(&["a", "b"], "(<c> ^query <q>) (<q> ^step z)", "");
    let r = result(&agent, s);
    assert_eq!(values(&agent, r, "failure").len(), 1, "{}", agent.output);
    assert!(values(&agent, r, "retrieved").is_empty());
}

#[test]
fn a_partial_match_gives_the_best_score() {
    let (agent, s) = query_after(&["a", "b", "c"], "(<c> ^query <q>) (<q> ^step b ^nothing here)", "");
    let r = result(&agent, s);
    assert_eq!(values(&agent, r, "memory-id"), vec!["2"], "{}", agent.output);
    assert_eq!(values(&agent, r, "cue-size"), vec!["2"]);
    assert_eq!(values(&agent, r, "match-cardinality"), vec!["1"]);
    assert_eq!(values(&agent, r, "graph-match"), vec!["0"]);
}

#[test]
fn neg_query_rules_out_an_episode() {
    let seen = "sp {seen (state <s> ^operator.name step ^step <any>) --> (<s> ^seen yes)}\n";
    let (agent, s) = query_after(&["a", "b", "c"], "(<c> ^query <q> ^neg-query <n> ^before 4) (<q> ^seen yes) (<n> ^step c)", seen);
    assert_eq!(values(&agent, result(&agent, s), "memory-id"), vec!["2"], "{}", agent.output);
}

/// Adds structure when the first step applies.
const STRUCTURE: &str = "sp {build (state <s> ^operator <o>) (<o> ^name step ^to a) --> (<s> ^a <x> ^b <x> ^c <y>) (<x> ^p 1) (<y> ^q 2)}\n";

#[test]
fn a_shared_identifier_comes_back_as_one_identifier() {
    let (agent, s) = query_after(&["a", "b"], "(<c> ^retrieve 1)", STRUCTURE);
    let retrieved = value(&agent, result(&agent, s), "retrieved");
    assert_eq!(value(&agent, retrieved, "a"), value(&agent, retrieved, "b"), "{}", agent.output);
    assert_eq!(values(&agent, value(&agent, retrieved, "a"), "p"), vec!["1"]);
    assert_ne!(value(&agent, retrieved, "a"), value(&agent, retrieved, "c"));
}

/// All the cue's values are in the episode, so it scores perfectly, but `^p` and `^q` are on
/// different identifiers: the graph match fails.
#[test]
fn graph_match_requires_the_cue_structure() {
    let structure = "sp {build (state <s> ^operator <o>) (<o> ^name step ^to a) --> (<s> ^a <x> ^a <y>) (<x> ^p 1) (<y> ^q 2)}\n";
    let (agent, s) = query_after(&["a", "b"], "(<c> ^query <q>) (<q> ^a <i>) (<i> ^p 1 ^q 2)", structure);
    let r = result(&agent, s);
    assert_eq!(values(&agent, r, "match-cardinality"), vec!["2"], "{}", agent.output);
    assert_eq!(values(&agent, r, "graph-match"), vec!["0"]);

    let structure = "sp {build (state <s> ^operator <o>) (<o> ^name step ^to a) --> (<s> ^a <x>) (<x> ^p 1 ^q 2)}\n";
    let (agent, s) = query_after(&["a", "b"], "(<c> ^query <q>) (<q> ^a <i>) (<i> ^p 1 ^q 2)", structure);
    let r = result(&agent, s);
    assert_eq!(values(&agent, r, "graph-match"), vec!["1"], "{}", agent.output);
    let mapping = value(&agent, r, "mapping");
    let node = value(&agent, mapping, "node");
    let a = value(&agent, value(&agent, r, "retrieved"), "a");
    assert_eq!(value(&agent, node, "retrieved"), a);
}

#[test]
fn exclusions_are_not_recorded() {
    let build = "epmem --set exclusions secret\nsp {build (state <s> ^operator <o>) (<o> ^name step ^to a) --> (<s> ^secret 1 ^open 2)}\n";
    let (agent, s) = query_after(&["a", "b"], "(<c> ^retrieve 1)", build);
    let retrieved = value(&agent, result(&agent, s), "retrieved");
    assert_eq!(values(&agent, retrieved, "open"), vec!["2"], "{}", agent.output);
    assert!(values(&agent, retrieved, "secret").is_empty());
}

/// A structure that leaves and comes back with the same shape gets the same rows, so a query
/// sees one identity over time.
#[test]
fn a_structure_keeps_its_rows_when_it_returns() {
    let rules = r#"
sp {has (state <s> ^operator <o>) (<o> ^name step ^to << a c >>) --> (<s> ^thing <t>) (<t> ^p 1)}
sp {hasnt (state <s> ^operator <o> ^thing <t>) (<o> ^name step ^to b) --> (<s> ^thing <t> -)}
"#;
    let (agent, s) = query_after(&["a", "b", "c", "d"], "(<c> ^query <q>) (<q> ^thing.p 1)", rules);
    // Episodes 1 and 3 and 4 have the thing; there's one identifier row for ^thing.
    assert_eq!(values(&agent, result(&agent, s), "memory-id"), vec!["4"], "{}", agent.output);
}
