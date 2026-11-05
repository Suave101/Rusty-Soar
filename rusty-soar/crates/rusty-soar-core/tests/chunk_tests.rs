//! Chunking: a substate solves a problem, returns a result, and a rule is learned that makes the
//! result directly next time.

use rusty_soar_core::agent::{symbol_to_string, SoarAgent};
use rusty_soar_core::soar_parser::{SoarCondition, SoarScript};
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

/// The superstate has `^input 3`. In the state no-change, one rule doubles it locally and
/// another returns the answer, so the result depends on `^input` through a local WME.
const DOUBLER: &str = r#"
sp {start (state <s> ^superstate nil -^input -^answer) --> (<s> ^operator <o> + =) (<o> ^name start)}
sp {start*apply (state <s> ^operator.name start) --> (<s> ^input 3)}
sp {double (state <s> ^superstate <ss>) (<ss> ^input <i>) --> (<s> ^doubled (* <i> 2))}
sp {answer (state <s> ^superstate <ss> ^doubled <d>) --> (<ss> ^answer <d>)}
"#;

#[test]
fn nothing_is_learned_by_default() {
    let (mut agent, s) = agent_with(DOUBLER);
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["6"]);
    assert_eq!(agent.chunker.stats.chunks, 0);
}

/// Runs a problem in a state no-change: operators put each value of `inputs` on `^input` in
/// turn, and rules in `rules` solve it. After each value, `^solved` clears the way for the next.
fn run(rules: &str, inputs: &[&str]) -> (SoarAgent, SymbolId) {
    let mut source = String::from("chunk always\n");
    let mut previous = String::from("-^input");
    for (i, input) in inputs.iter().enumerate() {
        source.push_str(&format!("sp {{set*{i} (state <s> ^superstate nil {previous}) --> (<s> ^operator <o> + =) (<o> ^name set ^value {input})}}\n"));
        previous = format!("^answer.of {input}");
    }
    source.push_str(
        "sp {set*apply (state <s> ^operator <o>) (<o> ^name set ^value <v>) --> (<s> ^input <v>)}
sp {set*clear (state <s> ^operator <o> ^input <old>) (<o> ^name set ^value <> <old>) --> (<s> ^input <old> -)}
",
    );
    source.push_str(rules);
    let (mut agent, s) = agent_with(&source);
    for _ in 0..inputs.len() * 3 + 2 {
        agent.run_decision_cycle(s);
    }
    (agent, s)
}

fn rule_text(agent: &SoarAgent) -> String {
    agent.chunker.rules.iter().map(|r| format!("{:?} --> {:?}\n", r.conditions, r.actions)).collect()
}

const COPIER: &str = r#"
sp {copy (state <s> ^superstate <ss>) (<ss> ^input <i>) --> (<s> ^copy <i>)}
sp {answer (state <s> ^superstate <ss> ^copy <c>) --> (<ss> ^answer <a>) (<a> ^of <c>)}
"#;

#[test]
fn a_learned_rule_generalizes_to_new_values() {
    let (agent, s) = run(COPIER, &["1", "2", "3"]);
    assert_eq!(values(&agent, s, "input"), vec!["3"], "{}", agent.output);
    assert_eq!(agent.chunker.stats.chunks, 1, "the first problem teaches the rest: {}", rule_text(&agent));
    let rule = rule_text(&agent);
    assert!(!rule.contains("Int(1)"), "the input is a variable: {rule}");
    assert!(agent.chunker.rules[0].name.starts_with("chunk*answer*StateNoChange*t"), "{}", agent.chunker.rules[0].name);
}

#[test]
fn a_learned_rule_keeps_the_constants_rules_tested() {
    let rules = r#"
sp {copy (state <s> ^superstate <ss>) (<ss> ^input 1) --> (<s> ^copy yes)}
sp {answer (state <s> ^superstate <ss> ^copy yes) --> (<ss> ^answer <a>) (<a> ^of 1)}
"#;
    let (agent, _) = run(rules, &["1"]);
    assert_eq!(agent.chunker.stats.chunks, 1, "{:?}", agent.chunker.stats);
    let rule = rule_text(&agent);
    assert!(rule.contains("Int(1)"), "{rule}");
}

#[test]
fn math_arguments_become_constants() {
    let rules = r#"
sp {double (state <s> ^superstate.input <i>) --> (<s> ^doubled (* <i> 2))}
sp {answer (state <s> ^superstate <ss> ^doubled <d>) --> (<ss> ^answer <a>) (<a> ^of <d>)}
"#;
    let (agent, _) = run(rules, &["3"]);
    let rule = rule_text(&agent);
    assert!(rule.contains("Int(3)") && rule.contains("Int(6)"), "{rule}");
}

#[test]
fn a_function_on_the_result_stays_in_the_rule() {
    let rules = "sp {answer (state <s> ^superstate <ss>) (<ss> ^input <i>) --> (<ss> ^answer <a>) (<a> ^of (+ <i> 1))}\n";
    let (agent, _) = run(rules, &["3"]);
    let rule = rule_text(&agent);
    assert!(rule.contains("FunctionCall") && !rule.contains("Int(3)"), "{rule}");
}

#[test]
fn a_test_on_a_local_copy_constrains_the_rule() {
    let rules = r#"
sp {copy (state <s> ^superstate.input <i>) --> (<s> ^copy <i>)}
sp {answer (state <s> ^superstate <ss> ^copy { <c> > 0 }) --> (<ss> ^answer <a>) (<a> ^of <c>)}
"#;
    let (agent, _) = run(rules, &["1"]);
    let rule = rule_text(&agent);
    assert!(rule.contains("Greater"), "the constraint moved to the ground condition: {rule}");
}

#[test]
fn a_negated_condition_on_the_superstate_is_kept() {
    let rules = "sp {answer (state <s> ^superstate <ss>) (<ss> ^input <i> -^blocked) --> (<ss> ^answer <a>) (<a> ^of <i>)}\n";
    let (agent, _) = run(rules, &["1"]);
    assert!(rule_text(&agent).contains("Negative"), "{}", rule_text(&agent));
}

#[test]
fn a_negated_conjunction_on_the_superstate_is_kept() {
    let rules = "sp {answer (state <s> ^superstate <ss>) (<ss> ^input <i>) -{ (<ss> ^x <y>) (<y> ^z <i>) } --> (<ss> ^answer <a>) (<a> ^of <i>)}\n";
    let (agent, s) = run(rules, &["1"]);
    assert_eq!(agent.chunker.stats.chunks, 1, "{:?}", agent.chunker.stats);
    let conditions = &agent.chunker.rules[0].conditions;
    let [SoarCondition::Positive(input), SoarCondition::ConjunctiveNegation(sub)] = conditions.as_slice() else { panic!("{:?}", conditions) };
    assert_eq!(sub.len(), 2, "{:?}", sub);
    // The negation's `<i>` is the rule's, and `<y>` is its own.
    let SoarCondition::Positive(second) = &sub[1] else { panic!() };
    assert_eq!(second.value, input.value, "{:?}", conditions);
    assert_eq!(values(&agent, s, "answer").len(), 1);
}

#[test]
fn a_negated_conjunction_on_local_structure_is_dropped() {
    let rules = "sp {answer (state <s> ^superstate <ss>) (<ss> ^input <i>) -{ (<s> ^x <y>) (<y> ^z 1) } --> (<ss> ^answer <a>) (<a> ^of <i>)}\n";
    let (agent, _) = run(rules, &["1"]);
    assert_eq!(agent.chunker.stats.chunks, 1);
    assert!(agent.chunker.rules[0].conditions.iter().all(|c| matches!(c, SoarCondition::Positive(_))), "{}", rule_text(&agent));
}

/// Two operators in a substate, and selection knowledge that depends on the superstate: the
/// operator chosen makes the result, so the rule learned depends on that knowledge too.
const SELECTED: &str = r#"
sp {start (state <s> ^superstate nil) --> (<s> ^prefer a)}
sp {propose (state <s> ^superstate <ss> ^attribute state) (<ss> ^prefer) --> (<s> ^operator <a> + , <b> +) (<a> ^name a) (<b> ^name b)}
sp {select (state <s> ^superstate <ss> ^operator <o> +) (<ss> ^prefer <p>) (<o> ^name <p>) --> (<s> ^operator <o> >)}
sp {apply (state <s> ^superstate <ss> ^operator <o>) (<o> ^name <n>) --> (<ss> ^answer done)}
"#;

fn run_selected(setting: &str) -> SoarAgent {
    let (mut agent, s) = agent_with(&format!("chunk always\n{}{}", setting, SELECTED));
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["done"], "{}", agent.output);
    agent
}

#[test]
fn a_rule_tests_what_selected_the_operator() {
    let agent = run_selected("");
    assert_eq!(agent.chunker.stats.chunks, 1, "{:?}", agent.chunker.stats);
    let text = format!("{:?}", agent.chunker.rules[0].conditions);
    assert!(text.contains("\"prefer\"") && text.contains("Str(\"a\")"), "the preference tested a's name: {}", rule_text(&agent));
}

#[test]
fn add_osk_off_leaves_selection_out() {
    let agent = run_selected("chunk add-osk off\n");
    assert_eq!(agent.chunker.stats.chunks, 1);
    assert!(!format!("{:?}", agent.chunker.rules[0].conditions).contains("Str(\"a\")"), "{}", rule_text(&agent));
}

/// A result two states up: a rule is learned for the state in between, then one from it.
#[test]
fn a_result_two_levels_up_learns_a_rule_at_each() {
    let source = r#"
chunk always
sp {start (state <s> ^superstate nil) --> (<s> ^input 3)}
sp {answer (state <s> ^superstate.superstate <t>) (<t> ^superstate nil ^input <i>) --> (<t> ^answer <i>)}
"#;
    let (mut agent, s) = agent_with(source);
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["3"], "{}", agent.output);
    let names: Vec<&str> = agent.chunker.rules.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names.len(), 2, "{}", rule_text(&agent));
    assert!(names[0].starts_with("chunk*answer*StateNoChange"), "{:?}", names);
    assert!(names[1].starts_with("chunkx2*answer*StateNoChange"), "{:?}", names);
    assert_eq!(agent.chunker.rules[0].conditions.len(), 3, "{}", rule_text(&agent));
    assert_eq!(agent.chunker.rules[1].conditions.len(), 2, "{}", rule_text(&agent));
}

/// A result from a lower state is local in the state above, which uses it: the rule there goes
/// through the lower state's chunk to the top state.
#[test]
fn a_rule_traces_through_a_chunk_from_below() {
    let source = r#"
chunk always
chunk bottom-only off
sp {start (state <s> ^superstate nil) --> (<s> ^input 3)}
sp {copy (state <s> ^superstate <m>) (<m> ^superstate <t>) (<t> ^superstate nil ^input <i>) --> (<m> ^copied <i>)}
sp {answer (state <m> ^superstate <t> ^copied <c>) --> (<t> ^answer <c>)}
"#;
    let (mut agent, s) = agent_with(source);
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["3"], "{}", agent.output);
    let last = agent.chunker.rules.last().unwrap();
    let text = format!("{:?}", last.conditions);
    assert!(text.contains("input"), "{}", rule_text(&agent));
}

#[test]
fn firing_records_go_with_their_state() {
    let finish = "sp {finish (state <s> ^superstate nil ^answer) --> (<s> ^operator <o> +) (<o> ^name finish)}\n";
    let (mut agent, s) = agent_with(&format!("chunk always\n{}{}", SELECTED, finish));
    let mut most = 0;
    for _ in 0..6 {
        agent.run_decision_cycle(s);
        most = most.max(agent.chunker.firing_count());
    }
    assert!(most > 0);
    assert_eq!(agent.chunker.firing_count(), 0, "{}", agent.output);
}

#[test]
fn testing_quiescence_gets_no_rule() {
    let rules = "sp {answer (state <s> ^superstate <ss> ^quiescence t) (<ss> ^input <i>) --> (<ss> ^answer <a>) (<a> ^of <i>)}\n";
    let (agent, _) = run(rules, &["1"]);
    assert_eq!(agent.chunker.stats.chunks, 0, "{}", rule_text(&agent));
}

/// A tie between operators, resolved in the substate: the learned rule makes the preference.
#[test]
fn a_rule_is_learned_for_a_preference() {
    let source = r#"
chunk always
sp {a (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name a)}
sp {b (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name b)}
sp {pick (state <s> ^impasse tie ^superstate <ss> ^item <i>) (<i> ^name a) --> (<ss> ^operator <i> >)}
sp {done (state <s> ^operator <o>) (<o> ^name a) --> (<s> ^done yes)}
"#;
    let (mut agent, s) = agent_with(source);
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "done"), vec!["yes"], "{}", agent.output);
    assert_eq!(agent.chunker.stats.chunks, 1, "{:?}", agent.chunker.stats);
    let rule = &agent.chunker.rules[0];
    assert!(rule.name.contains("*Tie*"), "{}", rule.name);
    // Upstream's `^item` traces to the superstate's acceptable preference.
    assert!(rule.conditions.iter().any(|c| matches!(c, SoarCondition::Positive(t) if t.acceptable)), "{}", rule_text(&agent));
}


/// The substate asks semantic memory about what the superstate wants, and returns what it finds.
const RECALLED: &str = r#"
smem --set learning on
smem --add { (<a> ^name apple ^colour red) }
sp {want (state <s> ^superstate nil) --> (<s> ^want apple)}
sp {ask (state <s> ^superstate <ss> ^smem.command <c>) (<ss> ^want <w>) --> (<c> ^query <q>) (<q> ^name <w>)}
sp {found (state <s> ^superstate <ss> ^smem.result.retrieved <r>) (<r> ^colour <c>) --> (<ss> ^answer <c>)}
"#;

fn run_recalled(setting: &str) -> SoarAgent {
    let (mut agent, s) = agent_with(&format!("chunk always\n{}{}", setting, RECALLED));
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["red"], "{}", agent.output);
    agent
}

#[test]
fn a_rule_is_learned_from_a_retrieval() {
    let agent = run_recalled("");
    assert_eq!(agent.chunker.stats.chunks, 1, "{:?}", agent.chunker.stats);
    let text = rule_text(&agent);
    // What memory held isn't known to the rule: the cue and the answer are constants.
    assert!(text.contains("Str(\"want\")") && text.contains("Str(\"apple\")"), "{}", text);
    assert!(text.contains("Symbol(Str(\"red\"))"), "{}", text);
}

#[test]
fn allow_opaque_off_learns_no_rule() {
    let agent = run_recalled("chunk allow-opaque off\n");
    assert_eq!(agent.chunker.stats.chunks, 0, "{}", rule_text(&agent));
    assert_eq!(agent.chunker.stats.justifications, 1);
}

#[test]
fn a_rule_is_learned_from_an_episodic_retrieval() {
    let source = r#"
chunk always
epmem --set learning on
sp {want (state <s> ^superstate nil) --> (<s> ^want apple ^colour red)}
sp {ask (state <s> ^superstate <ss> ^epmem.command <c>) (<ss> ^want <w>) --> (<c> ^query <q>) (<q> ^want <w>)}
sp {found (state <s> ^superstate <ss> ^epmem.result.retrieved <r>) (<r> ^colour <c>) --> (<ss> ^answer <c>)}
"#;
    let (mut agent, s) = agent_with(source);
    for _ in 0..4 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["red"], "{}", agent.output);
    assert_eq!(agent.chunker.stats.chunks, 1, "{:?}", agent.chunker.stats);
    let text = rule_text(&agent);
    assert!(text.contains("Str(\"want\")") && text.contains("Symbol(Str(\"red\"))"), "{}", text);
}

/// Two rules test one WME, one for a value and one to return it.
const TWICE: &str = r#"
sp {start (state <s> ^superstate nil) --> (<s> ^colour red)}
sp {note (state <s> ^superstate <ss>) (<ss> ^colour <c>) --> (<s> ^noted <c>)}
sp {answer (state <s> ^superstate <ss> ^noted <n>) (<ss> ^colour <c>) --> (<ss> ^answer <c>) (<ss> ^also <n>)}
"#;

#[test]
fn conditions_on_a_singleton_are_one_condition() {
    let (mut plain, s) = agent_with(&format!("chunk always\nchunk automatically-create-singletons off\n{}", TWICE));
    let (mut single, t) = agent_with(&format!("chunk always\nchunk automatically-create-singletons off\nchunk singleton state colour constant\n{}", TWICE));
    for _ in 0..3 {
        plain.run_decision_cycle(s);
        single.run_decision_cycle(t);
    }
    let count = |agent: &SoarAgent| agent.chunker.rules[0].conditions.iter().filter(|c| format!("{:?}", c).contains("colour")).count();
    assert_eq!(count(&plain), 2, "{}", rule_text(&plain));
    assert_eq!(count(&single), 1, "{}", rule_text(&single));
}

const FLAGGED: &str = r#"
sp {start (state <s> ^superstate nil) --> (<s> ^input 1)}
sp {flag (state <s> ^superstate <ss>) --> (FLAG <s>) (<s> ^flagged yes)}
sp {answer (state <s> ^superstate <ss> ^flagged yes) (<ss> ^input <i>) --> (<ss> ^answer <i>)}
"#;

#[test]
fn only_learns_in_flagged_states() {
    let learned = |mode: &str, function: &str| {
        let (mut agent, s) = agent_with(&format!("chunk {}\n{}", mode, FLAGGED.replace("FLAG", function)));
        for _ in 0..3 {
            agent.run_decision_cycle(s);
        }
        agent.chunker.stats.chunks
    };
    assert_eq!(learned("only", "force-learn"), 1);
    assert_eq!(learned("only", "dont-learn"), 0);
    assert_eq!(learned("except", "dont-learn"), 0);
    assert_eq!(learned("except", "force-learn"), 1);
}

/// No rule is learned in the lowest state, but the rule in the state above traces through the
/// justification to the top state, keeping the variable.
#[test]
fn a_rule_traces_through_a_justification() {
    let source = r#"
chunk except
chunk bottom-only off
sp {start (state <s> ^superstate nil) --> (<s> ^input 3)}
sp {flag (state <s> ^superstate.superstate.superstate nil) --> (dont-learn <s>) (<s> ^flagged yes)}
sp {copy (state <s> ^flagged yes ^superstate <m>) (<m> ^superstate <t>) (<t> ^superstate nil ^input <i>) --> (<m> ^copied <i>)}
sp {answer (state <m> ^superstate <t> ^copied <c>) (<t> ^superstate nil) --> (<t> ^answer <c>)}
"#;
    let (mut agent, s) = agent_with(source);
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["3"], "{}", agent.output);
    assert_eq!(agent.chunker.stats.justifications, 1, "{}", rule_text(&agent));
    assert_eq!(agent.chunker.rules.len(), 1, "{}", rule_text(&agent));
    let rule = &agent.chunker.rules[0];
    assert!(rule.name.starts_with("chunk*answer"), "{}", rule.name);
    let text = rule_text(&agent);
    assert!(text.contains("\"input\""), "{}", text);
    assert!(!text.contains("Int(3)"), "{}", text);
}

/// A justification keeps what stopped its rule being learned, so nothing above learns from it.
#[test]
fn a_rule_through_a_filtered_justification_is_not_learned() {
    let source = r#"
chunk always
chunk bottom-only off
sp {start (state <s> ^superstate nil) --> (<s> ^input 3)}
sp {copy (state <s> ^quiescence t ^superstate <m>) (<m> ^superstate <t>) (<t> ^superstate nil ^input <i>) --> (<m> ^copied <i>)}
sp {answer (state <m> ^superstate <t> ^copied <c>) (<t> ^superstate nil) --> (<t> ^answer <c>)}
"#;
    let (mut agent, s) = agent_with(source);
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "answer"), vec!["3"], "{}", agent.output);
    assert_eq!(agent.chunker.stats.chunks, 0, "{}", rule_text(&agent));
}

/// `$<i>` keeps the value it matched in the learned rule.
#[test]
fn a_constant_match_test_keeps_the_value() {
    let (agent, _) = run(
        r#"
sp {copy (state <s> ^superstate <ss>) (<ss> ^input $<i>) --> (<s> ^copy <i>)}
sp {answer (state <s> ^superstate <ss> ^copy <c>) --> (<ss> ^answer <a>) (<a> ^of <c>)}
"#,
        &["3"],
    );
    assert_eq!(agent.chunker.rules.len(), 1);
    assert!(rule_text(&agent).contains("Int(3)"), "{}", rule_text(&agent));
}

/// Two rules test one WME under different variables, and only the first compares them. By default
/// any string attribute is a singleton, so the conditions are one; off, they stay two.
#[test]
fn string_attributes_are_singletons_by_default() {
    let count = |setting: &str| {
        let source = format!(
            r#"
chunk always
{setting}
sp {{start (state <s> ^superstate nil) --> (<s> ^input 3)}}
sp {{one (state <s> ^superstate <ss>) (<ss> ^input <i>) --> (<s> ^one <i>)}}
sp {{two (state <s> ^superstate <ss> ^one) (<ss> ^input <j>) --> (<ss> ^answer <j>)}}
"#
        );
        let (mut agent, s) = agent_with(&source);
        for _ in 0..3 {
            agent.run_decision_cycle(s);
        }
        assert_eq!(agent.chunker.rules.len(), 1, "{}", rule_text(&agent));
        agent.chunker.rules[0].conditions.len()
    };
    assert_eq!(count(""), 1);
    assert_eq!(count("chunk automatically-create-singletons off"), 2);
}

#[test]
fn the_architecture_keeps_its_own_singletons() {
    let (agent, _) = agent_with("chunk singleton state operator any\nchunk singleton -r any superstate any\nchunk singleton identifier color constant\n");
    assert!(agent.output.contains("cannot override"), "{}", agent.output);
    assert!(agent.output.contains("cannot remove"), "{}", agent.output);
    assert!(agent.output.contains("Will unify"), "{}", agent.output);
}
