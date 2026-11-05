//! Parser, compiler and RHS behaviour checked against upstream `lexer.cpp`, `parser.cpp`,
//! `production_reorder.cpp` and the RHS functions.

use rusty_soar_core::agent::SoarAgent;
use rusty_soar_core::aadl_bridge::FlightTelemetryFFI;
use rusty_soar_core::preference::DecisionResult;
use rusty_soar_core::soar_parser::{
    SoarAction, SoarCondition, SoarPreference, SoarProduction, SoarProductionType, SoarRelation, SoarRhsValue,
    SoarScript, SoarSupport, SoarSymbol, SoarTest,
};
use rusty_soar_core::symbol::{SymbolData, SymbolId};

fn parse(body: &str) -> SoarProduction {
    SoarScript::parse_production(body).unwrap_or_else(|e| panic!("{e}"))
}

fn var(name: &str) -> SoarSymbol {
    SoarSymbol::Variable(name.into())
}

fn str_(s: &str) -> SoarSymbol {
    SoarSymbol::Str(s.into())
}

fn eq(symbol: SoarSymbol) -> SoarTest {
    SoarTest::Relational(SoarRelation::Equal, symbol)
}

fn positive(c: &SoarCondition) -> &rusty_soar_core::soar_parser::SoarConditionTests {
    match c {
        SoarCondition::Positive(t) => t,
        other => panic!("expected a positive condition, got {other:?}"),
    }
}

// ----------------------------------------------------------------------
// Lexer
// ----------------------------------------------------------------------

#[test]
fn lexer_symbol_types_follow_upstream() {
    let p = parse("p (state <s> ^a 5 ^b -5 ^c +3.5 ^d .5 ^e 1e5 ^f |hello world| ^g s1 ^h 2.) --> (<s> ^done yes)");
    let values: Vec<_> = p.conditions.iter().map(|c| positive(c).value.eq_test().cloned().unwrap()).collect();
    assert_eq!(
        values,
        vec![
            SoarSymbol::Int(5),
            SoarSymbol::Int(-5),
            SoarSymbol::Float(3.5),
            SoarSymbol::Float(0.5),
            str_("1e5"), // no decimal point, so not a number
            str_("hello world"),
            str_("S1"), // identifier lexemes become upper-case string constants
            SoarSymbol::Float(2.0),
        ]
    );
}

#[test]
fn comments_and_semicolons_are_skipped() {
    let script = SoarScript::parse(
        "# a comment\nsp {p ;\n (state <s> ^a b); # trailing } brace\n --> (<s> ^c d)}\nwatch 5\n",
    )
    .unwrap();
    assert_eq!(script.productions.len(), 1);
    assert_eq!(script.productions[0].conditions.len(), 1);
}

// ----------------------------------------------------------------------
// LHS
// ----------------------------------------------------------------------

#[test]
fn attribute_path_expands_into_chained_conditions() {
    let p = parse("p (state <s> ^io.input-link.telemetry <t>) --> (<s> ^x y)");
    assert_eq!(p.conditions.len(), 3);
    let c: Vec<_> = p.conditions.iter().map(positive).collect();
    // Placeholders are renamed like upstream's generate_new_variable: <i1>, <i2>.
    assert_eq!(c[0].id, eq(var("<s>")).add_goal_for_test());
    assert_eq!((c[0].attr.clone(), c[0].value.clone()), (eq(str_("io")), eq(var("<i1>"))));
    assert_eq!((c[1].id.clone(), c[1].attr.clone(), c[1].value.clone()), (eq(var("<i1>")), eq(str_("input-link")), eq(var("<i2>"))));
    assert_eq!((c[2].id.clone(), c[2].attr.clone(), c[2].value.clone()), (eq(var("<i2>")), eq(str_("telemetry")), eq(var("<t>"))));
}

trait GoalTest {
    fn add_goal_for_test(self) -> SoarTest;
}

impl GoalTest for SoarTest {
    fn add_goal_for_test(self) -> SoarTest {
        SoarTest::Conjunctive(vec![self, SoarTest::Goal])
    }
}

#[test]
fn several_values_make_several_conditions() {
    let p = parse("p (state <s> ^color red blue) --> (<s> ^x y)");
    assert_eq!(p.conditions.len(), 2);
    assert_eq!(positive(&p.conditions[1]).value, eq(str_("blue")));
    // Only the first condition carries the full id test; the others carry its equality part.
    assert_eq!(positive(&p.conditions[1]).id, eq(var("<s>")));
}

#[test]
fn negations() {
    let p = parse("p (state <s> ^a b -^c d) -(<s> ^e f) -{(<s> ^g <x>) (<x> ^h i)} --> (<s> ^x y)");
    assert!(matches!(p.conditions[1], SoarCondition::Negative(_)));
    assert!(matches!(p.conditions[2], SoarCondition::Negative(_)));
    let SoarCondition::ConjunctiveNegation(sub) = &p.conditions[3] else { panic!() };
    assert_eq!(sub.len(), 2);
}

#[test]
fn braces_around_positive_conditions_just_group() {
    let p = parse("p { (state <s> ^a b) (<s> ^c d) } --> (<s> ^x y)");
    assert_eq!(p.conditions.len(), 2);
    assert!(p.conditions.iter().all(|c| matches!(c, SoarCondition::Positive(_))));
}

#[test]
fn tests_relational_conjunctive_and_disjunction() {
    let p = parse("p (state <s> ^n { <n> > 5 <= 10 <> 7 } ^c << red green >> ^t <=> 1) --> (<s> ^x y)");
    let n = &positive(&p.conditions[0]).value;
    assert_eq!(
        *n,
        SoarTest::Conjunctive(vec![
            eq(var("<n>")),
            SoarTest::Relational(SoarRelation::Greater, SoarSymbol::Int(5)),
            SoarTest::Relational(SoarRelation::LessOrEqual, SoarSymbol::Int(10)),
            SoarTest::Relational(SoarRelation::NotEqual, SoarSymbol::Int(7)),
        ])
    );
    let SoarTest::Conjunctive(c) = &positive(&p.conditions[1]).value else { panic!() };
    assert_eq!(c[0], SoarTest::Disjunction(vec![str_("red"), str_("green")]));
    let SoarTest::Conjunctive(t) = &positive(&p.conditions[2]).value else { panic!() };
    assert_eq!(t[0], SoarTest::Relational(SoarRelation::SameType, SoarSymbol::Int(1)));
}

#[test]
fn two_equality_tests_keep_the_constant_with_a_warning() {
    let p = parse("p (state <s> ^a { <x> 5 }) --> (<s> ^x y)");
    assert_eq!(positive(&p.conditions[0]).value, eq(SoarSymbol::Int(5)));
    assert!(p.warnings.iter().any(|w| w.contains("two equality tests")));
}

#[test]
fn acceptable_preference_test() {
    let p = parse("p (state <s> ^operator <o> +) --> (<s> ^x y)");
    assert!(positive(&p.conditions[0]).acceptable);
}

#[test]
fn lhs_errors() {
    for (body, message) in [
        ("p (state <s>) --> (<s> ^a b)", "Expected attribute-value test"),
        ("p (state <s> ^a b) (foo ^c d) --> (<s> ^a b)", "Constant foo in id field"),
        ("p (state <s> ^a b) (<s> ^c d)", "Expected -->"),
        ("p (state <s> ^a << b c) --> (<s> ^a b)", "disjunction"),
    ] {
        let error = SoarScript::parse_production(body).unwrap_err().to_string();
        assert!(error.contains(message), "{body}: {error}");
    }
}

// ----------------------------------------------------------------------
// RHS
// ----------------------------------------------------------------------

fn prefs(p: &SoarProduction) -> Vec<(SoarPreference, Option<SoarRhsValue>)> {
    p.actions
        .iter()
        .map(|a| match a {
            SoarAction::Make { preference, referent, .. } => (*preference, referent.clone()),
            SoarAction::FunctionCall(_) => panic!("unexpected call"),
        })
        .collect()
}

#[test]
fn operator_preferences() {
    use SoarPreference::*;
    let o2 = Some(SoarRhsValue::Symbol(var("<o2>")));
    let cases = [
        ("(<s> ^operator <o>)", vec![(Acceptable, None)]),
        ("(<s> ^operator <o> + =)", vec![(Acceptable, None), (UnaryIndifferent, None)]),
        ("(<s> ^operator <o> > <o2>)", vec![(Better, o2.clone())]),
        ("(<s> ^operator <o> < <o2>)", vec![(Worse, o2.clone())]),
        ("(<s> ^operator <o> = <o2>)", vec![(BinaryIndifferent, o2)]),
        ("(<s> ^operator <o> = 0.5)", vec![(NumericIndifferent, Some(SoarRhsValue::Symbol(SoarSymbol::Float(0.5))))]),
        ("(<s> ^operator <o> >)", vec![(Best, None)]),
        ("(<s> ^operator <o> <, !)", vec![(Worst, None), (Require, None)]),
        ("(<s> ^operator <o> - ~)", vec![(Reject, None), (Prohibit, None)]),
    ];
    for (rhs, expected) in cases {
        let p = parse(&format!("p (state <s> ^a <o> ^b <o2>) --> {rhs}"));
        assert_eq!(prefs(&p), expected, "{rhs}");
    }
}

#[test]
fn non_operator_preferences() {
    let p = parse("p (state <s> ^a b) --> (<s> ^c d !)");
    assert_eq!(prefs(&p), vec![(SoarPreference::Acceptable, None)]);
    assert!(p.warnings.iter().any(|w| w.contains("only allowable non-operator preference")));

    let error = SoarScript::parse_production("p (state <s> ^a b) --> (<s> ^c d > e)").unwrap_err();
    assert!(error.to_string().contains("Binary preference illegal"));
}

#[test]
fn rhs_attribute_path_and_function_calls() {
    let p = parse("p (state <s> ^a <a>) --> (<s> ^x.y (+ <a> 1)) (write |a=| <a> (crlf))");
    assert_eq!(p.actions.len(), 3);
    let SoarAction::Make { id, value, .. } = &p.actions[0] else { panic!() };
    assert_eq!((id.clone(), value.clone()), (var("<s>"), SoarRhsValue::Symbol(var("<x1>"))));
    let SoarAction::Make { id, value, .. } = &p.actions[1] else { panic!() };
    assert_eq!(*id, var("<x1>"));
    assert_eq!(
        *value,
        SoarRhsValue::FunctionCall {
            name: "+".into(),
            args: vec![SoarRhsValue::Symbol(var("<a>")), SoarRhsValue::Symbol(SoarSymbol::Int(1))]
        }
    );
    assert!(matches!(&p.actions[2], SoarAction::FunctionCall(SoarRhsValue::FunctionCall { name, .. }) if name == "write"));
}

#[test]
fn header_documentation_and_flags() {
    let p = parse("p \"does a thing\" :o-support :default :interrupt (state <s> ^a b) --> (<s> ^c d)");
    assert_eq!(p.documentation.as_deref(), Some("does a thing"));
    assert_eq!(p.support, SoarSupport::OSupport);
    assert_eq!(p.production_type, SoarProductionType::Default);
    assert!(p.interrupt);
}

// ----------------------------------------------------------------------
// Compilation and execution
// ----------------------------------------------------------------------

fn agent_with(rules: &str) -> (SoarAgent, SymbolId) {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    let script = SoarScript::parse(rules).unwrap();
    agent.install_soar_script(&script, state).unwrap_or_else(|e| panic!("{e}"));
    (agent, state)
}

fn values_of(agent: &SoarAgent, id: SymbolId, attr: &str) -> Vec<SymbolData> {
    agent
        .wm
        .iter()
        .filter(|w| w.id == id && matches!(agent.symbols.resolve(w.attr), Some(SymbolData::String(s)) if s == attr))
        .filter_map(|w| agent.symbols.resolve(w.val).cloned())
        .collect()
}

fn set(agent: &mut SoarAgent, id: SymbolId, attr: &str, value: SymbolId) {
    let attr = agent.symbols.intern_str(attr);
    agent.insert_wme(id, attr, value);
}

#[test]
fn compile_errors_follow_upstream_validation() {
    for (rules, message) in [
        ("sp {p (<s> ^a b) --> (<s> ^c d)}", "test a state"),
        ("sp {p (state <s> ^a b) (<x> ^c d) --> (<s> ^c d)}", "not connected"),
        ("sp {p (state <s> ^a b) --> (<x> ^c d)}", "not bound on the LHS"),
        ("sp {p (state <s> ^a b) -(<s> ^c > <z>) --> (<s> ^c d)}", "unbound referent"),
        ("sp {p (state <s> ^a b) --> (<s> ^c (no-such-function))}", "not supported"),
        ("sp {p (state <s> ^a b) --> (<s> ^c (write x))}", "cannot be used as a value"),
    ] {
        let mut agent = SoarAgent::new();
        let (state, _) = agent.create_top_state();
        let error = agent.install_soar_script(&SoarScript::parse(rules).unwrap(), state).unwrap_err().to_string();
        assert!(error.contains(message), "{rules}: {error}");
    }
}

/// The test `^a > <y>` comes before `<y>` is bound, so it moves to the condition binding `<y>`.
#[test]
fn relational_test_on_a_later_variable() {
    let (mut agent, s) = agent_with("sp {bigger (state <s> ^a <x> > <y>) (<s> ^b <y>) --> (<s> ^bigger yes)}");
    let (three, five) = (agent.symbols.intern_int(3), agent.symbols.intern_int(5));
    set(&mut agent, s, "a", five);
    set(&mut agent, s, "b", three);
    agent.run_elaboration_phase();
    assert_eq!(values_of(&agent, s, "bigger"), vec![SymbolData::String("yes".into())]);

    let (mut agent, s) = agent_with("sp {bigger (state <s> ^a <x> > <y>) (<s> ^b <y>) --> (<s> ^bigger yes)}");
    let (three, five) = (agent.symbols.intern_int(3), agent.symbols.intern_int(5));
    set(&mut agent, s, "a", three);
    set(&mut agent, s, "b", five);
    agent.run_elaboration_phase();
    assert!(values_of(&agent, s, "bigger").is_empty());
}

/// A negation written before the condition binding its variable still works, and its result
/// retracts when the negation becomes blocked.
#[test]
fn negation_written_first_and_i_support_retraction() {
    let (mut agent, s) = agent_with("sp {p -(<s> ^blocked <v>) (state <s> ^v <v>) --> (<s> ^free <v>)}");
    let one = agent.symbols.intern_int(1);
    set(&mut agent, s, "v", one);
    agent.run_elaboration_phase();
    assert_eq!(values_of(&agent, s, "free"), vec![SymbolData::Integer(1)]);

    set(&mut agent, s, "blocked", one);
    agent.run_elaboration_phase();
    assert!(values_of(&agent, s, "free").is_empty());
}

#[test]
fn conjunctive_negation_through_the_parser() {
    let rules = "sp {p (state <s> ^item <i>) -{(<s> ^owner <o>) (<o> ^owns <i>)} --> (<s> ^unowned <i>)}";
    let (mut agent, s) = agent_with(rules);
    let (item, owner) = (agent.symbols.make_new_identifier('X'), agent.symbols.make_new_identifier('P'));
    set(&mut agent, s, "item", item);
    set(&mut agent, s, "owner", owner);
    agent.run_elaboration_phase();
    assert_eq!(values_of(&agent, s, "unowned").len(), 1);

    set(&mut agent, owner, "owns", item);
    agent.run_elaboration_phase();
    assert!(values_of(&agent, s, "unowned").is_empty());
}

#[test]
fn rhs_arithmetic_follows_upstream() {
    let (mut agent, s) = agent_with(
        "sp {p (state <s> ^go yes) --> (<s> ^sum (+ 1 2.5) ^ints (+ 1 2) ^quot (/ 6 3) ^div (div 7 2) ^mod (mod 7 2) ^neg (- 4) ^diff (- 10 1 2.0) ^name (make-constant-symbol a <s> 1))}",
    );
    let yes = agent.symbols.intern_str("yes");
    set(&mut agent, s, "go", yes);
    agent.run_elaboration_phase();
    assert_eq!(values_of(&agent, s, "sum"), vec![SymbolData::Float(3.5)]);
    assert_eq!(values_of(&agent, s, "ints"), vec![SymbolData::Integer(3)]);
    assert_eq!(values_of(&agent, s, "quot"), vec![SymbolData::Float(2.0)], "`/` always returns a float");
    assert_eq!(values_of(&agent, s, "div"), vec![SymbolData::Integer(3)]);
    assert_eq!(values_of(&agent, s, "mod"), vec![SymbolData::Integer(1)]);
    assert_eq!(values_of(&agent, s, "neg"), vec![SymbolData::Integer(-4)]);
    assert_eq!(values_of(&agent, s, "diff"), vec![SymbolData::Float(7.0)]);
    // Upstream always makes a new symbol from arguments: the text plus a counter from 1.
    assert_eq!(values_of(&agent, s, "name"), vec![SymbolData::String("aS111".into())]);
}

#[test]
fn make_constant_symbol_without_arguments_is_numbered() {
    let (mut agent, s) = agent_with("sp {p (state <s> ^go <g>) --> (<s> ^name (make-constant-symbol))}");
    for go in ["first", "second"] {
        let value = agent.symbols.intern_str(go);
        set(&mut agent, s, "go", value);
    }
    agent.run_elaboration_phase();
    let mut names: Vec<SymbolData> = values_of(&agent, s, "name");
    names.sort_by_key(|n| format!("{n:?}"));
    assert_eq!(names, vec![SymbolData::String("constant1".into()), SymbolData::String("constant2".into())]);
}

#[test]
fn division_by_zero_skips_the_action() {
    let (mut agent, s) = agent_with("sp {p (state <s> ^go yes) --> (<s> ^q (/ 1 0) ^ok yes)}");
    let yes = agent.symbols.intern_str("yes");
    set(&mut agent, s, "go", yes);
    agent.run_elaboration_phase();
    assert!(values_of(&agent, s, "q").is_empty());
    assert_eq!(values_of(&agent, s, "ok").len(), 1);
}

#[test]
fn write_and_halt() {
    let (mut agent, s) = agent_with("sp {p (state <s> ^n <n>) --> (write |n is | <n> (crlf)) (halt)}");
    let seven = agent.symbols.intern_int(7);
    set(&mut agent, s, "n", seven);
    agent.run_elaboration_phase();
    assert_eq!(agent.output, "n is 7\n");
    assert!(agent.halted);
}

/// Each firing creates new identifiers for RHS-unbound variables.
#[test]
fn each_firing_creates_new_identifiers() {
    let (mut agent, s) = agent_with("sp {p (state <s> ^item <i>) --> (<s> ^operator <o> +) (<o> ^name pick ^item <i>)}");
    let (a, b) = (agent.symbols.intern_str("a"), agent.symbols.intern_str("b"));
    set(&mut agent, s, "item", a);
    set(&mut agent, s, "item", b);
    agent.run_elaboration_phase();
    let operator = agent.symbols.intern_str("operator");
    let operators: Vec<_> = agent.preferences.iter().filter(|p| p.attr == operator).map(|p| p.value).collect();
    assert_eq!(operators.len(), 2);
    assert_ne!(operators[0], operators[1]);
    assert!(operators.iter().all(|o| matches!(agent.symbols.resolve(*o), Some(SymbolData::Identifier { prefix: 'O', .. }))));
}

/// An o-supported reject removes the matching preferences (upstream `process_o_rejects`).
#[test]
fn o_reject_removes_a_wme() {
    let (mut agent, s) = agent_with(
        "sp {make :o-support (state <s> ^make yes) --> (<s> ^old v)}
         sp {clear :o-support (state <s> ^clear yes ^old <v>) --> (<s> ^old <v> -)}",
    );
    let yes = agent.symbols.intern_str("yes");
    set(&mut agent, s, "make", yes);
    agent.run_application_phase();
    assert_eq!(values_of(&agent, s, "old").len(), 1, "o-supported, so it persists");

    set(&mut agent, s, "clear", yes);
    agent.run_application_phase();
    assert!(values_of(&agent, s, "old").is_empty());
}

/// A WME added from outside the rules is not in any slot, so a reject preference cannot
/// remove it.
#[test]
fn reject_does_not_remove_external_wmes() {
    let (mut agent, s) = agent_with("sp {p (state <s> ^clear yes ^old <v>) --> (<s> ^old <v> -)}");
    let (yes, v) = (agent.symbols.intern_str("yes"), agent.symbols.intern_str("v"));
    set(&mut agent, s, "old", v);
    set(&mut agent, s, "clear", yes);
    agent.run_elaboration_phase();
    assert_eq!(values_of(&agent, s, "old").len(), 1);
}

#[test]
fn operator_selection_through_parsed_preferences() {
    let rules = r#"
        sp {propose*a (state <s> ^go yes) --> (<s> ^operator <o> + >) (<o> ^name a)}
        sp {propose*b (state <s> ^go yes) --> (<s> ^operator <o> +) (<o> ^name b)}
    "#;
    let (mut agent, s) = agent_with(rules);
    let yes = agent.symbols.intern_str("yes");
    set(&mut agent, s, "go", yes);
    let DecisionResult::Selected(operator) = agent.run_decision_cycle(s) else { panic!("expected a selection") };
    assert_eq!(values_of(&agent, operator, "name"), vec![SymbolData::String("a".into())]);
}

/// The built-in default rules read telemetry through `^io.input-link.telemetry`.
#[test]
fn evaluate_script_runs_the_default_flight_rules() {
    let script = SoarScript::parse(rusty_soar_core::BAKED_SOAR_SCRIPT_SRC).unwrap();
    let telemetry = |engine_status| FlightTelemetryFFI { airspeed_kts: 200.0, altitude_ft: 12000.0, engine_status, sensor_valid: true };

    let emergency = rusty_soar_core::evaluate_script(&script, &telemetry(2));
    assert_eq!((emergency.operator_id, emergency.target_altitude_ft, emergency.is_valid), (99, 0.0, true));

    let cruise = rusty_soar_core::evaluate_script(&script, &telemetry(0));
    assert_eq!((cruise.operator_id, cruise.target_altitude_ft, cruise.is_valid), (10, 10000.0, true));

    let degraded = rusty_soar_core::evaluate_script(&script, &telemetry(1));
    assert!(!degraded.is_valid, "no rule proposes an operator");
}

/// The Tcl generator example from the documentation's Writing Rules guide.
#[test]
fn documented_tcl_generator_expands_to_one_rule_per_item() {
    let script = SoarScript::parse(
        r#"
proc generateWarn { types } {
   foreach type $types {
      sp "warn*${type}
            (state <s> ^${type}-error <e>)
         -->
            (<s> ^warning ${type})"
   }
}

generateWarn { gps lidar imu }
"#,
    )
    .unwrap();
    let names: Vec<&str> = script.productions.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["warn*gps", "warn*lidar", "warn*imu"]);
    assert!(script.commands.is_empty());
}
