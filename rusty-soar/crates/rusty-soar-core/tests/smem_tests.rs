//! Semantic memory through its command interface: rules write commands on `^smem.command`
//! and read results on `^smem.result`, as Soar agents do.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::smem::{LtiId, SmemValue};
use rusty_soar_core::soar_parser::SoarScript;
use rusty_soar_core::symbol::SymbolId;

fn agent_with(source: &str) -> (SoarAgent, SymbolId) {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&SoarScript::parse(source).unwrap(), state).unwrap();
    (agent, state)
}

/// The values of `(id ^attr *)` in working memory, printed.
fn values(agent: &SoarAgent, id: SymbolId, attr: &str) -> Vec<String> {
    let Some(attr) = agent.symbols.lookup_str(attr) else { return Vec::new() };
    let mut values: Vec<String> =
        agent.wm.iter().filter(|w| w.id == id && w.attr == attr && !w.acceptable).map(|w| symbol_to_string(&agent.symbols, w.val, false)).collect();
    values.sort();
    values
}

/// The single value of `(id ^attr *)`.
fn value(agent: &SoarAgent, id: SymbolId, attr: &str) -> SymbolId {
    let attr = agent.symbols.lookup_str(attr).unwrap();
    let found: Vec<SymbolId> = agent.wm.iter().filter(|w| w.id == id && w.attr == attr && !w.acceptable).map(|w| w.val).collect();
    assert_eq!(found.len(), 1, "one value of ^{}", symbol_to_string(&agent.symbols, attr, false));
    found[0]
}

const STORE: &str = r#"
smem --set learning on
sp {propose*store (state <s> ^superstate nil -^thing) --> (<s> ^operator <o> +) (<o> ^name store)}
sp {apply*store
    (state <s> ^operator.name store ^smem.command <c>)
-->
    (<s> ^thing <t>)
    (<t> ^name foo ^color red)
    (<c> ^store <t>)}
"#;

#[test]
fn store_command_stores_the_structure_and_reports_success() {
    let (mut agent, s) = agent_with(STORE);
    agent.run_decision_cycle(s);
    agent.run_decision_cycle(s);

    let thing = value(&agent, s, "thing");
    let smem = value(&agent, s, "smem");
    let result = value(&agent, smem, "result");
    assert_eq!(value(&agent, result, "success"), thing);
    let lti = agent.lti_of(thing).expect("the stored identifier is linked to long-term memory");
    assert_eq!(lti, LtiId(1));

    let edges: Vec<(String, String)> = agent
        .smem
        .edges(lti)
        .into_iter()
        .map(|(attr, value)| {
            let SmemValue::Constant(value) = value else { panic!("constant values") };
            (symbol_to_string(&agent.symbols, attr, false), symbol_to_string(&agent.symbols, value, false))
        })
        .collect();
    let mut sorted = edges.clone();
    sorted.sort();
    assert_eq!(sorted, vec![("color".into(), "red".into()), ("name".into(), "foo".into())]);
    assert_eq!(agent.smem.stats.stores, 1);
}

#[test]
fn retrieve_command_recalls_the_stored_structure() {
    let source = format!(
        "{STORE}
sp {{propose*forget (state <s> ^thing <t> ^smem.result.success <t> -^forgotten) --> (<s> ^operator <o> +) (<o> ^name forget)}}
sp {{apply*forget
    (state <s> ^operator.name forget ^thing <t> ^smem.command <c>)
    (<t> ^name <n> ^color <k>)
    (<c> ^store <t>)
-->
    (<s> ^forgotten yes)
    (<t> ^name <n> - ^color <k> -)
    (<c> ^store <t> - ^retrieve <t>)}}
"
    );
    let (mut agent, s) = agent_with(&source);
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    let thing = value(&agent, s, "thing");
    assert_eq!(values(&agent, s, "forgotten"), vec!["yes".to_string()]);
    let smem = value(&agent, s, "smem");
    let result = value(&agent, smem, "result");
    assert_eq!(value(&agent, result, "success"), thing, "{}", agent.output);
    assert_eq!(value(&agent, result, "retrieved"), thing);
    assert_eq!(values(&agent, thing, "name"), vec!["foo".to_string()]);
    assert_eq!(values(&agent, thing, "color"), vec!["red".to_string()]);
    assert_eq!(agent.smem.stats.retrievals, 1);
}

#[test]
fn commands_do_nothing_while_learning_is_off() {
    let (mut agent, s) = agent_with(&STORE.replace("smem --set learning on", ""));
    agent.run_decision_cycle(s);
    agent.run_decision_cycle(s);
    let smem = value(&agent, s, "smem");
    let result = value(&agent, smem, "result");
    assert!(values(&agent, result, "success").is_empty());
    assert!(agent.smem.is_empty());
}

#[test]
fn an_unknown_command_is_a_bad_command() {
    let (mut agent, s) = agent_with(
        "smem --set learning on
sp {bad (state <s> ^smem.command <c>) --> (<c> ^frobnicate yes)}",
    );
    agent.run_decision_cycle(s);
    let smem = value(&agent, s, "smem");
    let (command, result) = (value(&agent, smem, "command"), value(&agent, smem, "result"));
    assert_eq!(value(&agent, result, "bad-cmd"), command);
}

/// Stores an apple and a banana with one command, then, in a later operator, issues the query
/// whose structure `ask` writes on `<c>` (with `<q>` the query identifier).
fn fruit_query(ask: &str) -> (SoarAgent, SymbolId) {
    let source = format!(
        r#"
smem --set learning on
sp {{propose*store (state <s> ^superstate nil -^stage) --> (<s> ^operator <o> +) (<o> ^name store)}}
sp {{apply*store
    (state <s> ^operator.name store ^smem.command <c>)
-->
    (<s> ^stage stored)
    (<a> ^name apple ^kind fruit ^color red ^weight 3)
    (<b> ^name banana ^kind fruit ^color yellow ^weight 5)
    (<c> ^store <a> ^store <b>)}}
sp {{propose*query (state <s> ^stage stored) --> (<s> ^operator <o> +) (<o> ^name query)}}
sp {{apply*query*clear
    (state <s> ^operator.name query ^smem.command <c>)
    (<c> ^store <x>)
-->
    (<c> ^store <x> -)}}
sp {{apply*query*ask
    (state <s> ^operator.name query ^smem.command <c>)
-->
    (<s> ^stage stored - ^stage queried)
    {ask}}}
"#
    );
    let (mut agent, s) = agent_with(&source);
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "stage"), vec!["queried".to_string()], "{}", agent.output);
    (agent, s)
}

/// The `^name` of what the last query retrieved, or `None` if it failed.
fn retrieved_name(agent: &SoarAgent, s: SymbolId) -> Option<String> {
    let smem = value(agent, s, "smem");
    let result = value(agent, smem, "result");
    let retrieved = values(agent, result, "retrieved");
    if retrieved.is_empty() {
        assert_eq!(values(agent, result, "failure").len(), 1, "a query either succeeds or fails");
        return None;
    }
    let lti = value(agent, result, "retrieved");
    values(agent, lti, "name").pop()
}

#[test]
fn query_retrieves_the_memory_matching_a_constant() {
    let (agent, s) = fruit_query("(<c> ^query <q>) (<q> ^color yellow)");
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("banana"));
    assert_eq!(agent.smem.stats.queries, 1);
}

#[test]
fn query_fails_when_nothing_matches() {
    let (agent, s) = fruit_query("(<c> ^query <q>) (<q> ^color blue)");
    assert_eq!(retrieved_name(&agent, s), None);
}

#[test]
fn neg_query_excludes_memories() {
    let (agent, s) = fruit_query("(<c> ^query <q> ^neg-query <n>) (<q> ^kind fruit) (<n> ^color yellow)");
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("apple"));
}

#[test]
fn math_query_max_retrieves_the_largest_value() {
    let (agent, s) = fruit_query("(<c> ^query <q> ^math-query <m>) (<q> ^kind fruit) (<m> ^weight <w>) (<w> ^max true)");
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("banana"));
    let (agent, s) = fruit_query("(<c> ^query <q> ^math-query <m>) (<q> ^kind fruit) (<m> ^weight <w>) (<w> ^min true)");
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("apple"));
    let (agent, s) = fruit_query("(<c> ^query <q> ^math-query <m>) (<q> ^kind fruit) (<m> ^weight <w>) (<w> ^less 4)");
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("apple"));
}

/// Both memories were stored in one command without an access (`activate-on-add` is off), so
/// they have the same activation, and upstream's crawl returns the newest edge rows first. A
/// slot's WMEs are read newest first, so `^store <b>` is stored before `^store <a>`, and the
/// apple's edges are the newest. (This follows the port's model of upstream's slot order and
/// SQLite's choice of index; a differential run against upstream should confirm it.)
#[test]
fn equally_active_memories_are_tried_newest_edge_first() {
    let (agent, s) = fruit_query("(<c> ^query <q>) (<q> ^kind fruit)");
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("apple"));
    let (name, banana) = (agent.symbols.lookup_str("name").unwrap(), agent.symbols.lookup_str("banana").unwrap());
    assert!(agent.smem.edges(LtiId(1)).contains(&(name, SmemValue::Constant(banana))), "the banana is stored first");
}

/// The edges of `lti`, printed as `^attr value`, with identifier values as `@n`.
fn printed_edges(agent: &SoarAgent, lti: LtiId) -> Vec<String> {
    agent
        .smem
        .edges(lti)
        .into_iter()
        .map(|(attr, value)| {
            let value = match value {
                SmemValue::Constant(c) => symbol_to_string(&agent.symbols, c, false),
                SmemValue::Lti(l) => format!("@{}", l.0),
            };
            format!("^{} {}", symbol_to_string(&agent.symbols, attr, false), value)
        })
        .collect()
}

#[test]
fn smem_add_numbers_new_identifiers_in_order() {
    let (agent, _) = agent_with(
        "smem --add {
    (<a> ^name alice ^friend <b>)
    (<b> ^name bob ^age 30)
}",
    );
    assert_eq!(agent.smem.len(), 2);
    let mut a = printed_edges(&agent, LtiId(1));
    a.sort();
    assert_eq!(a, vec!["^friend @2", "^name alice"]);
    let mut b = printed_edges(&agent, LtiId(2));
    b.sort();
    assert_eq!(b, vec!["^age 30", "^name bob"]);
    // One `@` per identifier stored: <a> in the first clause, <b> in the second.
    assert_eq!(agent.output, "@@");
}

#[test]
fn smem_add_appends_to_numbered_identifiers() {
    let (agent, _) = agent_with(
        "smem --add {(@5 ^color red)}
smem --add {(@5 ^color blue ^shape round) (<x> ^of @5)}",
    );
    let mut edges = printed_edges(&agent, LtiId(5));
    edges.sort();
    assert_eq!(edges, vec!["^color blue", "^color red", "^shape round"]);
    // New identifiers are numbered from 1, skipping none (5 is not reached).
    assert_eq!(printed_edges(&agent, LtiId(1)), vec!["^of @5"]);
    assert_eq!(agent.smem.stats.edges, 4);
}

#[test]
fn smem_add_paths_make_intermediate_identifiers() {
    let (agent, _) = agent_with("smem --add {(<a> ^address.city paris ^address.zip 75001)}");
    // The root is made first, then one identifier per `.`.
    assert_eq!(printed_edges(&agent, LtiId(1)), vec!["^address @2", "^address @3"]);
    assert_eq!(printed_edges(&agent, LtiId(2)), vec!["^city paris"]);
    assert_eq!(printed_edges(&agent, LtiId(3)), vec!["^zip 75001"]);
}

#[test]
fn smem_add_rejects_a_clause_without_attributes() {
    let error = SoarScript::parse("smem --add {(<a>)}").unwrap_err();
    assert!(error.to_string().contains("Error parsing clause #0"), "{error}");
}

#[test]
fn added_memories_can_be_queried() {
    let (mut agent, s) = agent_with(
        r#"
smem --set learning on
smem --add {(<a> ^name apple ^kind fruit) (<b> ^name brick ^kind stone)}
sp {query (state <s> ^smem.command <c>) --> (<c> ^query <q>) (<q> ^kind stone)}
"#,
    );
    agent.run_decision_cycle(s);
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("brick"));
}

/// Under base-level activation, a memory retrieved before outranks one never accessed (whose
/// edges stay at the lowest activation, though they are newer).
#[test]
fn base_level_activation_ranks_a_retrieved_memory_first() {
    let (mut agent, s) = agent_with(
        r#"
smem --set learning on
smem --set activation-mode base-level
smem --add {(<a> ^name apple ^kind fruit) (<b> ^name banana ^kind fruit)}
sp {propose*recall (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name recall)}
sp {apply*recall
    (state <s> ^operator.name recall ^smem.command <c>)
-->
    (<s> ^done yes)
    (<c> ^query <q>)
    (<q> ^name apple)}
sp {propose*query (state <s> ^done yes -^asked) --> (<s> ^operator <o> +) (<o> ^name query)}
sp {apply*query
    (state <s> ^operator.name query ^smem.command <c>)
    (<c> ^query <old>)
-->
    (<s> ^asked yes)
    (<c> ^query <old> - ^query <q>)
    (<q> ^kind fruit)}
"#,
    );
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "asked"), vec!["yes".to_string()], "{}", agent.output);
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("apple"));
}

/// `@+`, `@-`, `@ <n>`, `@ <x>`, `!@ <x>` and `(@ <x>)` see the long-term identifier a
/// retrieved identifier is linked to.
#[test]
fn rules_test_long_term_identifier_links() {
    let (mut agent, s) = agent_with(
        r#"
smem --set learning on
smem --add {(<a> ^name apple) (<b> ^name banana)}
sp {query (state <s> ^smem.command <c>) --> (<c> ^query <q>) (<q> ^name banana)}
sp {linked (state <s> ^smem.result.retrieved {<r> @+}) --> (<s> ^linked yes)}
sp {unlinked (state <s> ^smem {<m> @-}) --> (<s> ^unlinked yes)}
sp {number (state <s> ^smem.result.retrieved {<r> @ 2}) --> (<s> ^is-two yes)}
sp {not-number (state <s> ^smem.result.retrieved {<r> @ 1}) --> (<s> ^is-one yes)}
sp {same (state <s> ^smem.result.retrieved <r> ^smem.result.retrieved {<q> @ <r>}) --> (<s> ^same yes)}
sp {differ (state <s> ^smem.result.retrieved <r> ^smem.command {<c> !@ <r>}) --> (<s> ^differ yes)}
sp {value (state <s> ^smem.result.retrieved <r>) --> (<s> ^lti (@ <r>))}
"#,
    );
    // The query is answered at the end of the first cycle; the rules fire in the second.
    agent.run_decision_cycle(s);
    agent.run_decision_cycle(s);
    assert_eq!(retrieved_name(&agent, s).as_deref(), Some("banana"));
    for (attr, expected) in [("linked", &["yes"][..]), ("unlinked", &["yes"]), ("is-two", &["yes"]), ("is-one", &[]), ("same", &["yes"]), ("differ", &["yes"]), ("lti", &["2"])] {
        assert_eq!(values(&agent, s, attr), expected, "^{attr}");
    }
}
