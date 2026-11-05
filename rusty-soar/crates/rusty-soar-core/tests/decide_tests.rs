//! Decision procedure behaviour checked against upstream `decide.cpp`, `consistency.cpp`
//! and `run_soar.cpp`, using small Soar programs.

use rusty_soar_core::agent::SoarAgent;
use rusty_soar_core::preference::{DecisionResult, ExplorationPolicy};
use rusty_soar_core::soar_parser::SoarScript;
use rusty_soar_core::symbol::{SymbolData, SymbolId};

fn agent_with(rules: &str) -> (SoarAgent, SymbolId) {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    let script = SoarScript::parse(rules).unwrap_or_else(|e| panic!("{e}"));
    agent.install_soar_script(&script, state).unwrap_or_else(|e| panic!("{e}"));
    (agent, state)
}

fn values(agent: &SoarAgent, id: SymbolId, attr: &str) -> Vec<SymbolData> {
    agent
        .wm
        .iter()
        .filter(|w| !w.acceptable && w.id == id && matches!(agent.symbols.resolve(w.attr), Some(SymbolData::String(s)) if s == attr))
        .filter_map(|w| agent.symbols.resolve(w.val).cloned())
        .collect()
}

fn str_(s: &str) -> SymbolData {
    SymbolData::String(s.into())
}

fn name_of(agent: &SoarAgent, operator: SymbolId) -> SymbolData {
    values(agent, operator, "name").pop().expect("operator should have a name")
}

const TWO_PROPOSALS: &str = r#"
    sp {propose*a (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name a)}
    sp {propose*b (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name b)}
"#;

/// The classic counter: an o-supported apply rule changes state, proposals retract and
/// re-propose, and the consistency check deselects the stale operator.
#[test]
fn operator_cycle_counts_to_three() {
    let (mut agent, s) = agent_with(
        r#"
        sp {init :o-support (state <s> ^superstate nil -^count) --> (<s> ^count 0)}
        sp {propose*inc (state <s> ^count {<c> < 3}) --> (<s> ^operator <o> +) (<o> ^name inc)}
        sp {apply*inc (state <s> ^operator <o> ^count <c>) (<o> ^name inc) --> (<s> ^count <c> - (+ <c> 1))}
        "#,
    );
    for _ in 0..5 {
        agent.run_decision_cycle(s);
    }
    assert_eq!(values(&agent, s, "count"), vec![SymbolData::Integer(3)]);
    // Nothing is proposed any more: a state no-change substate.
    assert_eq!(agent.goal_stack().len(), 2);
    assert!(agent.selected_operator.is_none());
}

/// The selected operator gets a `(S1 ^operator O1)` WME; proposals also make
/// acceptable-preference WMEs `(S1 ^operator O1 +)`.
#[test]
fn selection_creates_operator_wme() {
    let (mut agent, s) = agent_with("sp {propose (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name only)}");
    let DecisionResult::Selected(op) = agent.run_decision_cycle(s) else { panic!("expected a selection") };
    let operator = agent.symbols.intern_str("operator");
    assert!(agent.wm.find(s, operator, op).is_some());
    assert!(agent.wm.find_with_flag(s, operator, op, true).is_some());
    assert_eq!(agent.selected_operator, Some(op));
}

/// Upstream `create_new_impasse` + `create_new_context` + `update_impasse_items`.
#[test]
fn tie_impasse_substate_structure() {
    let (mut agent, s) = agent_with(TWO_PROPOSALS);
    let DecisionResult::TieImpasse(items) = agent.run_decision_cycle(s) else { panic!("expected a tie") };
    assert_eq!(items.len(), 2);
    let stack = agent.goal_stack();
    assert_eq!(stack.len(), 2);
    let s2 = stack[1];
    assert_eq!(agent.symbols.resolve(s2), Some(&SymbolData::Identifier { prefix: 'S', number: 2 }));
    assert_eq!(values(&agent, s2, "type"), vec![str_("state")]);
    assert_eq!(values(&agent, s2, "impasse"), vec![str_("tie")]);
    assert_eq!(values(&agent, s2, "choices"), vec![str_("multiple")]);
    assert_eq!(values(&agent, s2, "attribute"), vec![str_("operator")]);
    assert_eq!(values(&agent, s2, "quiescence"), vec![str_("t")]);
    assert_eq!(values(&agent, s2, "superstate").len(), 1);
    assert_eq!(values(&agent, s2, "item").len(), 2);
    assert_eq!(values(&agent, s2, "item-count"), vec![SymbolData::Integer(2)]);
    assert_eq!(values(&agent, s2, "non-numeric").len(), 2);
    assert_eq!(values(&agent, s2, "non-numeric-count"), vec![SymbolData::Integer(2)]);
    for link in ["reward-link", "epmem", "smem"] {
        assert_eq!(values(&agent, s2, link).len(), 1, "{link}");
    }
}

/// A rule in the tie substate returns a better preference to the superstate. The result is
/// justified by the superstate's acceptable preferences, so it survives the substate.
#[test]
fn tie_resolved_by_substate_result() {
    let rules = format!(
        "{TWO_PROPOSALS}
        sp {{tie*prefer-a (state <s> ^impasse tie ^superstate <ss> ^item <a> ^item {{<b> <> <a>}}) (<a> ^name a)
            --> (<ss> ^operator <a> > <b>)}}"
    );
    let (mut agent, s) = agent_with(&rules);
    assert!(matches!(agent.run_decision_cycle(s), DecisionResult::TieImpasse(_)));
    let DecisionResult::Selected(op) = agent.run_decision_cycle(s) else { panic!("the tie should be resolved") };
    assert_eq!(name_of(&agent, op), str_("a"));
    assert_eq!(agent.goal_stack().len(), 1, "the tie substate is gone");
}

#[test]
fn operator_no_change() {
    let (mut agent, s) = agent_with("sp {propose (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name idle)}");
    assert!(matches!(agent.run_decision_cycle(s), DecisionResult::Selected(_)));
    assert_eq!(agent.run_decision_cycle(s), DecisionResult::NoChangeImpasse);
    let s2 = agent.goal_stack()[1];
    assert_eq!(values(&agent, s2, "impasse"), vec![str_("no-change")]);
    assert_eq!(values(&agent, s2, "attribute"), vec![str_("operator")]);
    assert_eq!(values(&agent, s2, "choices"), vec![str_("none")]);
    assert!(agent.selected_operator.is_some(), "the operator stays selected above the impasse");

    // With nothing proposed in S2, the next decision is a state no-change below it, as
    // upstream subgoals until something changes.
    agent.run_decision_cycle(s);
    let stack = agent.goal_stack();
    assert_eq!(stack[..2], [s, s2]);
    assert_eq!(values(&agent, stack[2], "attribute"), vec![str_("state")]);
}

#[test]
fn state_no_change() {
    let (mut agent, s) = agent_with("sp {noop (state <s> ^superstate nil) --> (<s> ^seen yes)}");
    assert_eq!(agent.run_decision_cycle(s), DecisionResult::NoChangeImpasse);
    let s2 = agent.goal_stack()[1];
    assert_eq!(values(&agent, s2, "attribute"), vec![str_("state")]);
    assert_eq!(values(&agent, s2, "impasse"), vec![str_("no-change")]);
}

#[test]
fn constraint_failure_and_conflict() {
    let (mut agent, s) = agent_with(
        r#"
        sp {a (state <s> ^superstate nil) --> (<s> ^operator <o> !) (<o> ^name a)}
        sp {b (state <s> ^superstate nil) --> (<s> ^operator <o> !) (<o> ^name b)}
        "#,
    );
    assert!(matches!(agent.run_decision_cycle(s), DecisionResult::ConstraintFailureImpasse(c) if c.len() == 2));
    let s2 = agent.goal_stack()[1];
    assert_eq!(values(&agent, s2, "impasse"), vec![str_("constraint-failure")]);
    assert_eq!(values(&agent, s2, "choices"), vec![str_("none")]);

    let (mut agent, s) = agent_with(
        r#"
        sp {a (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name a)}
        sp {b (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name b)}
        sp {a>b (state <s> ^operator <x> + ^operator <y> +) (<x> ^name a) (<y> ^name b) --> (<s> ^operator <x> > <y>)}
        sp {b>a (state <s> ^operator <x> + ^operator <y> +) (<x> ^name b) (<y> ^name a) --> (<s> ^operator <x> > <y>)}
        "#,
    );
    assert!(matches!(agent.run_decision_cycle(s), DecisionResult::ConflictImpasse(c) if c.len() == 2));
    assert_eq!(values(&agent, agent.goal_stack()[1], "impasse"), vec![str_("conflict")]);
}

/// Mutually indifferent candidates are chosen by the exploration policy, reproducibly for a
/// given seed.
#[test]
fn indifferent_selection_is_reproducible() {
    let rules = r#"
        sp {a (state <s> ^superstate nil) --> (<s> ^operator <o> + =) (<o> ^name a)}
        sp {b (state <s> ^superstate nil) --> (<s> ^operator <o> + =) (<o> ^name b)}
        sp {c (state <s> ^superstate nil) --> (<s> ^operator <o> + =) (<o> ^name c)}
    "#;
    let choose = |seed: u32| {
        let (mut agent, s) = agent_with(rules);
        agent.exploration.rng.seed(seed);
        let DecisionResult::Selected(op) = agent.run_decision_cycle(s) else { panic!("expected a selection") };
        name_of(&agent, op)
    };
    let picks: Vec<_> = (0..12).map(choose).collect();
    assert_eq!(picks, (0..12).map(choose).collect::<Vec<_>>());
    assert!(picks.iter().any(|p| *p != picks[0]), "different seeds should not always agree");
}

/// Softmax weights candidates by their numeric-indifferent values.
#[test]
fn numeric_indifferent_with_softmax_and_first() {
    let rules = r#"
        sp {a (state <s> ^superstate nil) --> (<s> ^operator <o> + = 0) (<o> ^name a)}
        sp {b (state <s> ^superstate nil) --> (<s> ^operator <o> + = 5) (<o> ^name b)}
    "#;
    for seed in 0..10 {
        let (mut agent, s) = agent_with(rules);
        agent.exploration.rng.seed(seed);
        let DecisionResult::Selected(op) = agent.run_decision_cycle(s) else { panic!() };
        assert_eq!(name_of(&agent, op), str_("b"), "a zero value is never chosen while another is positive");
    }

    let (mut agent, s) = agent_with(TWO_PROPOSALS.replace("+)", "+ =)").as_str());
    agent.exploration.policy = ExplorationPolicy::First;
    assert!(matches!(agent.run_decision_cycle(s), DecisionResult::Selected(_)));
}

/// O-supported results of operator application persist after the operator goes; i-supported
/// elaborations retract with their match.
#[test]
fn o_support_persists_and_i_support_retracts() {
    let (mut agent, s) = agent_with(
        r#"
        sp {propose (state <s> ^superstate nil -^done) --> (<s> ^operator <o> +) (<o> ^name finish)}
        sp {apply (state <s> ^operator <o>) (<o> ^name finish) --> (<s> ^done yes)}
        sp {elaborate (state <s> ^operator <o>) (<o> ^name finish) --> (<o> ^note selected)}
        "#,
    );
    let DecisionResult::Selected(op) = agent.run_decision_cycle(s) else { panic!() };
    assert_eq!(values(&agent, s, "done"), vec![str_("yes")]);
    // The proposal retracted (it tests -^done), so the operator was deselected and its
    // i-supported elaboration retracted with it.
    agent.run_decision_cycle(s);
    assert_eq!(values(&agent, s, "done"), vec![str_("yes")]);
    assert!(values(&agent, op, "note").is_empty());
    assert_ne!(agent.selected_operator, Some(op));
}

/// Rules fire in the propose phase only when i-supported: an o-supported rule waits for
/// the apply phase.
#[test]
fn propose_phase_fires_only_i_supported_rules() {
    let (mut agent, s) = agent_with(
        r#"
        sp {elab (state <s> ^superstate nil) --> (<s> ^i yes)}
        sp {persist :o-support (state <s> ^superstate nil) --> (<s> ^o yes)}
        "#,
    );
    agent.run_elaboration_phase();
    assert_eq!(values(&agent, s, "i"), vec![str_("yes")]);
    assert!(values(&agent, s, "o").is_empty());
    agent.run_application_phase();
    assert_eq!(values(&agent, s, "o"), vec![str_("yes")]);
}

/// An operator no-change substate that applies its own operator, recording an o-supported
/// WME that depends on the superstate's `^input`.
const SUBSTATE_MEMO: &str = r#"
    sp {propose*top (state <s> ^superstate nil ^task go) --> (<s> ^operator <o> +) (<o> ^name top)}
    sp {propose*sub (state <s> ^superstate <ss>) (<ss> ^operator <op>) (<op> ^name top)
        --> (<s> ^operator <o> +) (<o> ^name sub)}
    sp {apply*sub (state <s> ^operator <o> ^superstate <ss>) (<o> ^name sub) (<ss> ^input <v>)
        --> (<s> ^memo <v>)}
"#;

/// Runs the memo agent until the substate has applied its operator. Returns the agent, the
/// top state, the substate and the key of the `^input` WME.
fn memo_agent() -> (SoarAgent, SymbolId, SymbolId, rusty_soar_core::wm::WmeKey) {
    let (mut agent, s) = agent_with(SUBSTATE_MEMO);
    let (task, go, input, value) = (
        agent.symbols.intern_str("task"),
        agent.symbols.intern_str("go"),
        agent.symbols.intern_str("input"),
        agent.symbols.intern_str("seen"),
    );
    agent.insert_wme(s, task, go);
    let input_key = agent.insert_wme(s, input, value);
    for _ in 0..3 {
        agent.run_decision_cycle(s);
    }
    let s2 = agent.goal_stack()[1];
    assert_eq!(values(&agent, s2, "memo"), vec![str_("seen")], "the substate applied its operator");
    (agent, s, s2, input_key)
}

/// Upstream `elaborate_gds`: the o-supported substate WME's GDS holds the superstate WMEs
/// its firing depended on, directly and through the i-supported proposal of `sub`.
#[test]
fn gds_collects_superstate_dependencies() {
    let (agent, s, s2, input_key) = memo_agent();
    let gds = agent.goal_dependency_set(s2);
    assert!(gds.contains(&input_key), "tested directly by apply*sub");
    let operator = agent.symbols.resolve(agent.wm.get(gds[0]).map(|w| w.attr).unwrap()).cloned();
    assert!(operator.is_some());
    let superstate_operator = agent
        .wm
        .iter()
        .find(|w| w.id == s && !w.acceptable && matches!(agent.symbols.resolve(w.attr), Some(SymbolData::String(a)) if a == "operator"))
        .map(|w| w.key)
        .unwrap();
    assert!(gds.contains(&superstate_operator), "tested by the proposal the substate operator came from");
    assert!(agent.goal_dependency_set(s).is_empty(), "the top state has no GDS");
}

/// Upstream `gds_invalid_so_remove_goal`: removing a WME in the GDS removes the substate.
#[test]
fn gds_change_removes_the_substate() {
    let (mut agent, s, s2, input_key) = memo_agent();
    agent.remove_wme(input_key);
    assert_eq!(agent.goal_stack(), vec![s], "the substate's dependency changed");
    assert!(!agent.goal_stack().contains(&s2));
    // Its o-supported memo goes with it at the next working-memory phase.
    agent.run_decision_cycle(s);
    assert!(agent.wm.iter().all(|w| w.id != s2));
}

/// Changes the substate did not depend on leave it alone, and its o-supported WME stays.
#[test]
fn unrelated_change_keeps_the_substate() {
    let (mut agent, s, s2, _) = memo_agent();
    let (other, x) = (agent.symbols.intern_str("other"), agent.symbols.intern_str("x"));
    let other_key = agent.insert_wme(s, other, x);
    agent.remove_wme(other_key);
    assert_eq!(agent.goal_stack()[..2], [s, s2]);
    assert_eq!(values(&agent, s2, "memo"), vec![str_("seen")]);
}

/// Only o-supported substate WMEs create a GDS; i-supported ones simply retract.
#[test]
fn i_supported_substate_wmes_make_no_gds() {
    let (mut agent, s) = agent_with(
        r#"
        sp {propose*top (state <s> ^superstate nil) --> (<s> ^operator <o> +) (<o> ^name top)}
        sp {elaborate (state <s> ^superstate <ss>) (<ss> ^input <v>) --> (<s> ^copy <v>)}
        "#,
    );
    let (input, v) = (agent.symbols.intern_str("input"), agent.symbols.intern_str("v"));
    agent.insert_wme(s, input, v);
    agent.run_decision_cycle(s);
    agent.run_decision_cycle(s);
    agent.run_decision_cycle(s);
    let s2 = agent.goal_stack()[1];
    assert_eq!(values(&agent, s2, "copy"), vec![str_("v")]);
    assert!(agent.goal_dependency_set(s2).is_empty());
}

/// An agent sitting in a state no-change substate, with rules at both levels triggered by
/// the top state's `^trigger`.
fn waterfall_agent(max_elaborations: usize) -> (SoarAgent, SymbolId, SymbolId) {
    let (mut agent, s) = agent_with(
        r#"
        sp {top*elaborate (state <s> ^superstate nil ^trigger yes) --> (<s> ^top-elab yes)}
        sp {sub*local (state <s> ^superstate <ss>) (<ss> ^trigger yes) --> (<s> ^sub-elab yes)}
        sp {sub*result (state <s> ^superstate <ss>) (<ss> ^trigger yes) --> (<ss> ^from-sub yes)}
        "#,
    );
    assert_eq!(agent.run_decision_cycle(s), DecisionResult::NoChangeImpasse);
    let s2 = agent.goal_stack()[1];
    let (trigger, yes) = (agent.symbols.intern_str("trigger"), agent.symbols.intern_str("yes"));
    agent.insert_wme(s, trigger, yes);
    agent.max_elaborations = max_elaborations;
    (agent, s, s2)
}

/// Upstream's inner waterfall loop: after the top goal fires, the substate's matches fire in
/// the same elaboration cycle when they only change the substate's own structure; a match
/// that would change the superstate waits for the next cycle.
#[test]
fn inner_waterfall_fires_lower_goals_in_the_same_cycle() {
    let (mut agent, s, s2) = waterfall_agent(1);
    agent.run_elaboration_phase();
    assert_eq!(values(&agent, s, "top-elab"), vec![str_("yes")]);
    assert_eq!(values(&agent, s2, "sub-elab"), vec![str_("yes")], "local change: same cycle");
    assert!(values(&agent, s, "from-sub").is_empty(), "changes the superstate: postponed");

    let (mut agent, s, _) = waterfall_agent(2);
    agent.run_elaboration_phase();
    assert_eq!(values(&agent, s, "from-sub"), vec![str_("yes")], "fires in the following cycle");
}

/// With no activity at the top goal, the substate is the highest active goal and fires
/// everything, including results.
#[test]
fn highest_active_goal_fires_everything() {
    let (mut agent, s) = agent_with(
        "sp {sub*result (state <s> ^superstate <ss>) (<ss> ^trigger yes) --> (<ss> ^from-sub yes)}",
    );
    agent.run_decision_cycle(s);
    let (trigger, yes) = (agent.symbols.intern_str("trigger"), agent.symbols.intern_str("yes"));
    agent.insert_wme(s, trigger, yes);
    agent.max_elaborations = 1;
    agent.run_elaboration_phase();
    assert_eq!(values(&agent, s, "from-sub"), vec![str_("yes")]);
}

fn wmes_of(agent: &SoarAgent, id: SymbolId) -> usize {
    agent.wm.iter().filter(|w| w.id == id).count()
}

fn value_id(agent: &SoarAgent, id: SymbolId, attr: &str) -> SymbolId {
    agent
        .wm
        .iter()
        .find(|w| !w.acceptable && w.id == id && matches!(agent.symbols.resolve(w.attr), Some(SymbolData::String(a)) if a == attr))
        .map(|w| w.val)
        .unwrap_or_else(|| panic!("no ^{attr}"))
}

/// Builds `(S1 ^thing T)`, with `T` holding a part and, optionally, a cycle through a peer,
/// then lets `^drop` reject the `^thing` link.
fn gc_agent(extra: &str) -> (SoarAgent, SymbolId) {
    let rules = format!(
        "sp {{make :o-support (state <s> ^superstate nil ^make yes) --> (<s> ^thing <t>) (<t> ^color red ^part <p>) (<p> ^size big) {extra}}}
         sp {{drop :o-support (state <s> ^drop yes ^thing <t>) --> (<s> ^thing <t> -)}}"
    );
    let (mut agent, s) = agent_with(&rules);
    let (make, yes) = (agent.symbols.intern_str("make"), agent.symbols.intern_str("yes"));
    agent.insert_wme(s, make, yes);
    agent.run_application_phase();
    (agent, s)
}

fn drop_thing(agent: &mut SoarAgent, s: SymbolId) {
    let (drop, yes) = (agent.symbols.intern_str("drop"), agent.symbols.intern_str("yes"));
    agent.insert_wme(s, drop, yes);
    agent.run_application_phase();
}

/// Upstream `do_demotion`: an identifier whose last link goes is garbage collected, and so is
/// everything only it linked to.
#[test]
fn disconnected_structure_is_garbage_collected() {
    let (mut agent, s) = gc_agent("");
    let t = value_id(&agent, s, "thing");
    let p = value_id(&agent, t, "part");
    assert_eq!(agent.identifier_level(t), Some(1));
    assert_eq!(agent.identifier_level(p), Some(1));

    drop_thing(&mut agent, s);
    assert_eq!(wmes_of(&agent, t), 0);
    assert_eq!(wmes_of(&agent, p), 0);
    assert_eq!(agent.identifier_level(t), None);
}

/// A cycle keeps link counts above zero; the mark-and-walk still finds it disconnected.
#[test]
fn disconnected_cycle_is_garbage_collected() {
    let (mut agent, s) = gc_agent("(<t> ^peer <u>) (<u> ^peer <t>)");
    let t = value_id(&agent, s, "thing");
    let u = value_id(&agent, t, "peer");
    drop_thing(&mut agent, s);
    assert_eq!(wmes_of(&agent, t), 0);
    assert_eq!(wmes_of(&agent, u), 0);
}

/// Structure still linked from the goal another way stays.
#[test]
fn structure_linked_elsewhere_survives() {
    let (mut agent, s) = gc_agent("(<s> ^also <t>)");
    let t = value_id(&agent, s, "thing");
    drop_thing(&mut agent, s);
    assert!(values(&agent, s, "thing").is_empty());
    assert_eq!(values(&agent, t, "color"), vec![str_("red")]);
    assert_eq!(agent.identifier_level(t), Some(1));
}

/// Input structure is collected too once nothing links to it.
#[test]
fn disconnected_input_is_garbage_collected() {
    let (mut agent, s) = agent_with("sp {noop (state <s> ^superstate nil) --> (<s> ^seen yes)}");
    let (obj_attr, size) = (agent.symbols.intern_str("object"), agent.symbols.intern_str("size"));
    let obj = agent.symbols.make_new_identifier('O');
    let big = agent.symbols.intern_str("big");
    let link = agent.insert_wme(s, obj_attr, obj);
    agent.insert_wme(obj, size, big);
    agent.run_elaboration_phase();
    assert_eq!(agent.identifier_level(obj), Some(1));

    agent.remove_wme(link);
    agent.run_elaboration_phase();
    assert_eq!(wmes_of(&agent, obj), 0);
}

/// Substate structure sits at the substate's level; linking it from the top state promotes it.
#[test]
fn identifier_levels_follow_links() {
    let (mut agent, s) = agent_with("sp {noop (state <s> ^superstate nil) --> (<s> ^seen yes)}");
    agent.run_decision_cycle(s);
    let s2 = agent.goal_stack()[1];
    assert_eq!(agent.identifier_level(s), Some(1));
    assert_eq!(agent.identifier_level(s2), Some(2));
    let epmem = value_id(&agent, s2, "epmem");
    assert_eq!(agent.identifier_level(epmem), Some(2));

    let link = agent.symbols.intern_str("shared");
    agent.insert_wme(s, link, epmem);
    agent.run_elaboration_phase();
    assert_eq!(agent.identifier_level(epmem), Some(1), "promoted by a link from the top state");
}
