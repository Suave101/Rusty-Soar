//! Rete behaviour checked against upstream `rete.cpp` semantics.

use rusty_soar_core::rete::{
    AlphaTest, Condition, ConditionTests, Field, Referent, Relation, ReteNetwork, ReteTest, VarLocation,
};
use rusty_soar_core::symbol::{SymbolId, SymbolTable};
use rusty_soar_core::wm::WmeKey;

struct Fixture {
    rete: ReteNetwork,
    symbols: SymbolTable,
    next_key: usize,
}

impl Fixture {
    fn new() -> Self {
        Self {
            rete: ReteNetwork::new(),
            symbols: SymbolTable::new(),
            next_key: 0,
        }
    }

    fn sym(&mut self, s: &str) -> SymbolId {
        self.symbols.intern_str(s)
    }

    fn add(&mut self, id: SymbolId, attr: SymbolId, val: SymbolId) -> WmeKey {
        let key = WmeKey(self.next_key);
        self.next_key += 1;
        self.rete.add_wme(key, (id, attr, val), false, &self.symbols).expect("capacity");
        key
    }

    fn remove(&mut self, key: WmeKey) {
        self.rete.remove_wme(key, &self.symbols);
    }

    fn production(&mut self, conditions: Vec<Condition>) {
        self.rete.add_production("p", conditions, &self.symbols).expect("capacity");
    }

    /// Fires pending matches and returns (asserted, retracted) counts.
    fn fire(&mut self) -> (usize, usize) {
        (self.rete.take_assertions().len(), self.rete.take_retractions().len())
    }
}

fn attr_test(attr: SymbolId) -> AlphaTest {
    AlphaTest {
        attr: Some(attr),
        ..AlphaTest::default()
    }
}

fn var(depth: usize, field: Field) -> Referent {
    Referent::Variable(VarLocation { depth, field })
}

fn eq(field: Field, referent: Referent) -> ReteTest {
    ReteTest::Relational {
        field,
        relation: Relation::Equal,
        referent,
    }
}

fn positive(alpha: AlphaTest, tests: Vec<ReteTest>) -> Condition {
    Condition::Positive(ConditionTests {
        alpha,
        acceptable: false,
        tests,
    })
}

fn negative(alpha: AlphaTest, tests: Vec<ReteTest>) -> Condition {
    Condition::Negative(ConditionTests {
        alpha,
        acceptable: false,
        tests,
    })
}

/// `(<s> ^x <a>) (<s> ^x <b>)`: both conditions share one alpha memory. Upstream right-activates
/// descendants before ancestors so a WME joining with itself is matched exactly once.
#[test]
fn self_join_through_shared_alpha_memory_is_not_duplicated() {
    let mut f = Fixture::new();
    let (s, x) = (f.symbols.intern_id('S', 1), f.sym("x"));
    f.production(vec![
        positive(attr_test(x), vec![]),
        positive(attr_test(x), vec![eq(Field::Id, var(1, Field::Id))]),
    ]);

    let one = f.sym("1");
    f.add(s, x, one);
    assert_eq!(f.rete.pending_assertions(), 1, "(w, w) only");

    let two = f.sym("2");
    f.add(s, x, two);
    assert_eq!(f.rete.pending_assertions(), 4, "every ordered pair of the two WMEs");
}

/// Upstream `add_production_to_rete` populates new nodes from existing working memory.
#[test]
fn production_added_after_wmes_matches_existing_wmes() {
    let mut f = Fixture::new();
    let (s, a, b) = (f.symbols.intern_id('S', 1), f.sym("a"), f.sym("b"));
    f.add(s, a, b);
    f.production(vec![positive(attr_test(a), vec![])]);
    assert_eq!(f.fire(), (1, 0));
}

/// `(<x> ^self <x>)`: a variable bound twice in one condition tests the WME against itself.
#[test]
fn variable_repeated_within_one_condition() {
    let mut f = Fixture::new();
    let (x1, x2, self_attr) = (f.symbols.intern_id('X', 1), f.symbols.intern_id('X', 2), f.sym("self"));
    f.production(vec![
        positive(AlphaTest::default(), vec![]),
        positive(attr_test(self_attr), vec![eq(Field::Val, var(2, Field::Id))]),
    ]);
    f.add(x1, self_attr, x2);
    assert_eq!(f.fire(), (0, 0));
    f.add(x1, self_attr, x1);
    assert_eq!(f.fire(), (2, 0), "(X1 ^self X1) joined with each of the two WMEs");
}

/// `(<s> ^a <v>) -(<s> ^blocked yes)`: a negated condition blocks while a WME matches it, and a
/// fired match is retracted when it becomes blocked.
#[test]
fn negated_condition_blocks_and_retracts() {
    let mut f = Fixture::new();
    let (s, a, blocked, yes, v) = (
        f.symbols.intern_id('S', 1),
        f.sym("a"),
        f.sym("blocked"),
        f.sym("yes"),
        f.sym("v"),
    );
    f.production(vec![
        positive(attr_test(a), vec![]),
        negative(
            AlphaTest {
                attr: Some(blocked),
                val: Some(yes),
                ..AlphaTest::default()
            },
            vec![eq(Field::Id, var(1, Field::Id))],
        ),
    ]);

    f.add(s, a, v);
    assert_eq!(f.fire(), (1, 0));

    let blocker = f.add(s, blocked, yes);
    assert_eq!(f.fire(), (0, 1));

    f.remove(blocker);
    assert_eq!(f.fire(), (1, 0));
}

/// A blocker on a different identifier doesn't block: the negation's join test applies.
#[test]
fn negated_condition_respects_join_tests() {
    let mut f = Fixture::new();
    let (s1, s2, a, blocked, yes, v) = (
        f.symbols.intern_id('S', 1),
        f.symbols.intern_id('S', 2),
        f.sym("a"),
        f.sym("blocked"),
        f.sym("yes"),
        f.sym("v"),
    );
    f.production(vec![
        positive(attr_test(a), vec![]),
        negative(attr_test(blocked), vec![eq(Field::Id, var(1, Field::Id))]),
    ]);
    f.add(s1, a, v);
    f.add(s2, blocked, yes);
    assert_eq!(f.fire(), (1, 0));
}

/// `(<s> ^a <v>) -{ (<s> ^item <i>) (<i> ^color red) }`: blocked only while the whole
/// conjunction matches.
#[test]
fn conjunctive_negation_blocks_only_on_full_conjunction() {
    let mut f = Fixture::new();
    let (s, i, a, item, color, red, v) = (
        f.symbols.intern_id('S', 1),
        f.symbols.intern_id('I', 1),
        f.sym("a"),
        f.sym("item"),
        f.sym("color"),
        f.sym("red"),
        f.sym("v"),
    );
    f.production(vec![
        positive(attr_test(a), vec![]),
        Condition::ConjunctiveNegation(vec![
            positive(attr_test(item), vec![eq(Field::Id, var(1, Field::Id))]),
            positive(
                AlphaTest {
                    attr: Some(color),
                    val: Some(red),
                    ..AlphaTest::default()
                },
                vec![eq(Field::Id, var(2, Field::Val))],
            ),
        ]),
    ]);

    f.add(s, a, v);
    assert_eq!(f.fire(), (1, 0));

    let item_wme = f.add(s, item, i);
    assert_eq!(f.fire(), (0, 0), "half the conjunction doesn't block");

    let red_wme = f.add(i, color, red);
    assert_eq!(f.fire(), (0, 1));

    f.remove(red_wme);
    assert_eq!(f.fire(), (1, 0));

    f.add(i, color, red);
    assert_eq!(f.fire(), (0, 1));
    f.remove(item_wme);
    assert_eq!(f.fire(), (1, 0));
}

/// A production added while the NCC is already satisfied starts blocked.
#[test]
fn conjunctive_negation_added_over_existing_wmes() {
    let mut f = Fixture::new();
    let (s, i, a, item, v) = (
        f.symbols.intern_id('S', 1),
        f.symbols.intern_id('I', 1),
        f.sym("a"),
        f.sym("item"),
        f.sym("v"),
    );
    f.add(s, a, v);
    let item_wme = f.add(s, item, i);
    f.production(vec![
        positive(attr_test(a), vec![]),
        Condition::ConjunctiveNegation(vec![positive(attr_test(item), vec![eq(Field::Id, var(1, Field::Id))])]),
    ]);
    assert_eq!(f.fire(), (0, 0));
    f.remove(item_wme);
    assert_eq!(f.fire(), (1, 0));
}

/// Upstream `compare_symbols`: ints and floats compare numerically; strings by bytes.
#[test]
fn relational_tests_follow_upstream_compare_symbols() {
    let mut f = Fixture::new();
    let (s, n) = (f.symbols.intern_id('S', 1), f.sym("n"));
    let five = f.symbols.intern_int(5);
    f.production(vec![positive(
        attr_test(n),
        vec![ReteTest::Relational {
            field: Field::Val,
            relation: Relation::Greater,
            referent: Referent::Constant(five),
        }],
    )]);

    let four = f.symbols.intern_int(4);
    let five_and_a_bit = f.symbols.intern_float(5.5);
    let six = f.symbols.intern_int(6);
    f.add(s, n, four);
    f.add(s, n, five);
    assert_eq!(f.rete.pending_assertions(), 0);
    f.add(s, n, five_and_a_bit);
    f.add(s, n, six);
    assert_eq!(f.rete.pending_assertions(), 2);
}

/// Upstream quirk: symbols of incomparable types compare as "less", so `<` passes.
#[test]
fn incomparable_types_compare_as_less() {
    let mut f = Fixture::new();
    let (s, n) = (f.symbols.intern_id('S', 1), f.sym("n"));
    let five = f.symbols.intern_int(5);
    let word = f.sym("abc");
    f.production(vec![positive(
        attr_test(n),
        vec![ReteTest::Relational {
            field: Field::Val,
            relation: Relation::Less,
            referent: Referent::Constant(five),
        }],
    )]);
    f.add(s, n, word);
    assert_eq!(f.fire(), (1, 0));
}

/// Int 5 and float 5.0 are distinct symbols: `=` fails, `<=>` (same type) fails, `>=` passes.
#[test]
fn int_and_float_are_distinct_symbols() {
    let mut symbols = SymbolTable::new();
    let int = symbols.intern_int(5);
    let float = symbols.intern_float(5.0);
    assert_ne!(int, float);
    assert!(!symbols.same_type(int, float));
    assert_eq!(symbols.compare(int, float), core::cmp::Ordering::Equal);
}

#[test]
fn disjunction_test() {
    let mut f = Fixture::new();
    let (s, color) = (f.symbols.intern_id('S', 1), f.sym("color"));
    let (red, green, blue) = (f.sym("red"), f.sym("green"), f.sym("blue"));
    f.production(vec![positive(
        attr_test(color),
        vec![ReteTest::Disjunction {
            field: Field::Val,
            values: vec![red, green],
        }],
    )]);
    f.add(s, color, blue);
    assert_eq!(f.rete.pending_assertions(), 0);
    f.add(s, color, green);
    assert_eq!(f.rete.pending_assertions(), 1);
}

/// `(state <s> ...)` tests that the identifier is a goal, not a fixed identifier.
#[test]
fn state_test_matches_any_goal() {
    let mut f = Fixture::new();
    let (s1, s2, x1, a, v) = (
        f.symbols.intern_id('S', 1),
        f.symbols.intern_id('S', 2),
        f.symbols.intern_id('X', 1),
        f.sym("a"),
        f.sym("v"),
    );
    f.symbols.mark_goal(s1);
    f.symbols.mark_goal(s2);
    f.production(vec![positive(attr_test(a), vec![ReteTest::IdIsGoal])]);
    f.add(s1, a, v);
    f.add(s2, a, v);
    f.add(x1, a, v);
    assert_eq!(f.fire(), (2, 0));
}

/// A match that disappears before firing is dropped silently (upstream removes it from the
/// tentative assertions), not reported as a retraction.
#[test]
fn unfired_match_that_disappears_is_dropped() {
    let mut f = Fixture::new();
    let (s, a, v) = (f.symbols.intern_id('S', 1), f.sym("a"), f.sym("v"));
    f.production(vec![positive(attr_test(a), vec![])]);
    let w = f.add(s, a, v);
    f.remove(w);
    assert_eq!(f.fire(), (0, 0));
}

/// A fired match that is retracted and re-established with the same WMEs before the retraction
/// is processed is kept, not retracted (upstream's tentative-retraction check).
#[test]
fn strobing_match_keeps_its_instantiation() {
    let mut f = Fixture::new();
    let (s, a, v, blocked, yes) = (
        f.symbols.intern_id('S', 1),
        f.sym("a"),
        f.sym("v"),
        f.sym("blocked"),
        f.sym("yes"),
    );
    f.production(vec![
        positive(attr_test(a), vec![]),
        negative(attr_test(blocked), vec![eq(Field::Id, var(1, Field::Id))]),
    ]);
    f.add(s, a, v);
    assert_eq!(f.fire(), (1, 0));

    let blocker = f.add(s, blocked, yes);
    f.remove(blocker);
    assert_eq!(f.fire(), (0, 0));
}

/// Removing a WME retracts every fired match that used it.
#[test]
fn removing_a_wme_retracts_its_matches() {
    let mut f = Fixture::new();
    let (s, a, b) = (f.symbols.intern_id('S', 1), f.sym("a"), f.sym("b"));
    let (v1, v2) = (f.sym("1"), f.sym("2"));
    f.production(vec![
        positive(attr_test(a), vec![]),
        positive(attr_test(b), vec![eq(Field::Id, var(1, Field::Id))]),
    ]);
    let wa = f.add(s, a, v1);
    f.add(s, b, v1);
    f.add(s, b, v2);
    assert_eq!(f.fire(), (2, 0));
    f.remove(wa);
    assert_eq!(f.fire(), (0, 2));
}

/// Productions with identical leading conditions share nodes and both match.
#[test]
fn shared_nodes_feed_every_production() {
    let mut f = Fixture::new();
    let (s, a, b, v) = (f.symbols.intern_id('S', 1), f.sym("a"), f.sym("b"), f.sym("v"));
    f.rete.add_production("p1", vec![positive(attr_test(a), vec![])], &f.symbols).expect("capacity");
    f.rete
        .add_production(
            "p2",
            vec![
                positive(attr_test(a), vec![]),
                positive(attr_test(b), vec![eq(Field::Id, var(1, Field::Id))]),
            ],
            &f.symbols,
        )
        .expect("capacity");
    f.add(s, a, v);
    f.add(s, b, v);
    let names: Vec<_> = f.rete.take_assertions().into_iter().map(|i| i.rule_name).collect();
    assert_eq!(names, vec!["p1", "p2"]);
}

#[test]
fn flattened_conditions_mark_conjunctive_negations_with_their_length() {
    use rusty_soar_core::rete::{flatten_conditions, FlatCondition};
    let positive = || Condition::Positive(ConditionTests { alpha: AlphaTest::default(), acceptable: false, tests: Vec::new() });
    let conditions = vec![
        positive(),
        Condition::ConjunctiveNegation(vec![positive(), Condition::ConjunctiveNegation(vec![positive()])]),
        positive(),
    ];
    let flat = flatten_conditions(&conditions);
    let shape: Vec<Option<usize>> = flat
        .iter()
        .map(|c| match c {
            FlatCondition::ConjunctiveNegation { len } => Some(*len),
            _ => None,
        })
        .collect();
    assert_eq!(shape, vec![None, Some(3), None, Some(1), None, None]);
}

#[test]
fn malformed_flat_conditions_are_rejected() {
    use rusty_soar_core::rete::{CapacityError, FlatCondition, FlatTests};
    let mut rete = ReteNetwork::new();
    let symbols = SymbolTable::new();
    let positive = FlatCondition::Positive(FlatTests { alpha: AlphaTest::default(), acceptable: false, tests: &[] });
    let result = rete.add_production_flat("p", &[positive, FlatCondition::ConjunctiveNegation { len: 2 }], &symbols);
    assert_eq!(result, Err(CapacityError::MalformedConditions));
    assert_eq!(rete.overflow(), None, "a rejected production leaves the network usable");
}
