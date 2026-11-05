//! Differential test: after every random working-memory change, the Rete's live matches must
//! equal those of a brute-force matcher that evaluates each condition list from scratch.

use rusty_soar_core::rete::{
    AlphaTest, Condition, ConditionTests, Field, Referent, Relation, ReteNetwork, ReteTest, VarLocation,
};
use rusty_soar_core::symbol::{SymbolId, SymbolTable};
use rusty_soar_core::wm::WmeKey;
use std::collections::BTreeMap;

type Triple = (SymbolId, SymbolId, SymbolId);

/// Deterministic xorshift generator, so failures are reproducible from the seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

// ----------------------------------------------------------------------
// Brute-force reference matcher
// ----------------------------------------------------------------------

fn field(triple: &Triple, f: Field) -> SymbolId {
    match f {
        Field::Id => triple.0,
        Field::Attr => triple.1,
        Field::Val => triple.2,
    }
}

/// `levels[d - 1]` is the WME at condition level `d` (`None` for negated levels).
fn passes(symbols: &SymbolTable, c: &ConditionTests, w: &Triple, levels: &[Option<Triple>], acceptable: bool) -> bool {
    if c.acceptable != acceptable || !c.alpha.matches(w.0, w.1, w.2) {
        return false;
    }
    let depth = levels.len() + 1;
    c.tests.iter().all(|test| match test {
        ReteTest::Relational { field: f, relation, referent } => {
            let left = field(w, *f);
            let right = match referent {
                Referent::Constant(s) => *s,
                Referent::Variable(VarLocation { depth: d, field: rf }) if *d == depth => field(w, *rf),
                Referent::Variable(VarLocation { depth: d, field: rf }) => match levels[*d - 1] {
                    Some(t) => field(&t, *rf),
                    None => return false,
                },
            };
            let order = symbols.compare(left, right);
            match relation {
                Relation::Equal => left == right,
                Relation::NotEqual => left != right,
                Relation::Less => order.is_lt(),
                Relation::Greater => order.is_gt(),
                Relation::LessOrEqual => order.is_le(),
                Relation::GreaterOrEqual => order.is_ge(),
                Relation::SameType => symbols.same_type(left, right),
            }
        }
        ReteTest::Disjunction { field: f, values } => values.contains(&field(w, *f)),
        ReteTest::IdIsGoal => symbols.is_goal(w.0),
        ReteTest::IdIsImpasse => symbols.is_impasse(w.0),
    })
}

/// All extensions of `levels` that satisfy `conditions`.
fn matches(
    symbols: &SymbolTable,
    wm: &[(WmeKey, Triple)],
    conditions: &[Condition],
    levels: Vec<Option<Triple>>,
    keys: Vec<WmeKey>,
    out: &mut Vec<Vec<WmeKey>>,
) {
    let Some((first, rest)) = conditions.split_first() else {
        out.push(keys);
        return;
    };
    match first {
        Condition::Positive(c) => {
            for (key, w) in wm {
                if passes(symbols, c, w, &levels, false) {
                    let mut l = levels.clone();
                    l.push(Some(*w));
                    let mut k = keys.clone();
                    k.push(*key);
                    matches(symbols, wm, rest, l, k, out);
                }
            }
        }
        Condition::Negative(c) => {
            if !wm.iter().any(|(_, w)| passes(symbols, c, w, &levels, false)) {
                let mut l = levels;
                l.push(None);
                matches(symbols, wm, rest, l, keys, out);
            }
        }
        Condition::ConjunctiveNegation(sub) => {
            let mut inner = Vec::new();
            matches(symbols, wm, sub, levels.clone(), Vec::new(), &mut inner);
            if inner.is_empty() {
                let mut l = levels;
                l.push(None);
                matches(symbols, wm, rest, l, keys, out);
            }
        }
    }
}

// ----------------------------------------------------------------------
// Productions covering each node type
// ----------------------------------------------------------------------

fn pos(alpha: AlphaTest, tests: Vec<ReteTest>) -> Condition {
    Condition::Positive(ConditionTests { alpha, acceptable: false, tests })
}

fn neg(alpha: AlphaTest, tests: Vec<ReteTest>) -> Condition {
    Condition::Negative(ConditionTests { alpha, acceptable: false, tests })
}

fn attr(a: SymbolId) -> AlphaTest {
    AlphaTest { attr: Some(a), ..AlphaTest::default() }
}

fn eq(f: Field, depth: usize, rf: Field) -> ReteTest {
    ReteTest::Relational {
        field: f,
        relation: Relation::Equal,
        referent: Referent::Variable(VarLocation { depth, field: rf }),
    }
}

fn rel(f: Field, relation: Relation, referent: Referent) -> ReteTest {
    ReteTest::Relational { field: f, relation, referent }
}

struct Domain {
    ids: Vec<SymbolId>,
    attrs: Vec<SymbolId>,
    values: Vec<SymbolId>,
}

fn productions(symbols: &mut SymbolTable, d: &Domain) -> Vec<Vec<Condition>> {
    let (a, b, item) = (d.attrs[0], d.attrs[1], d.attrs[2]);
    let two = symbols.intern_int(2);
    vec![
        // (state <s> ^a <x>)
        vec![pos(attr(a), vec![ReteTest::IdIsGoal])],
        // (<s> ^a <x>) (<s> ^a <y>)  -- self-join through a shared alpha memory
        vec![pos(attr(a), vec![]), pos(attr(a), vec![eq(Field::Id, 1, Field::Id)])],
        // (<s> ^a <x>) (<s> ^b <x>)
        vec![pos(attr(a), vec![]), pos(attr(b), vec![eq(Field::Id, 1, Field::Id), eq(Field::Val, 1, Field::Val)])],
        // (<s> ^a <x>) -(<s> ^b <x>)
        vec![pos(attr(a), vec![]), neg(attr(b), vec![eq(Field::Id, 1, Field::Id), eq(Field::Val, 1, Field::Val)])],
        // (<s> ^a <x>) -{(<s> ^item <i>) (<i> ^b <x>)}
        vec![
            pos(attr(a), vec![]),
            Condition::ConjunctiveNegation(vec![
                pos(attr(item), vec![eq(Field::Id, 1, Field::Id)]),
                pos(attr(b), vec![eq(Field::Id, 2, Field::Val), eq(Field::Val, 1, Field::Val)]),
            ]),
        ],
        // (<s> ^a <x>) -{(<s> ^item <i>) -(<i> ^b <x>)} (<s> ^b <y> > <x>)  -- NCC with a negation inside, then a join
        vec![
            pos(attr(a), vec![]),
            Condition::ConjunctiveNegation(vec![
                pos(attr(item), vec![eq(Field::Id, 1, Field::Id)]),
                neg(attr(b), vec![eq(Field::Id, 2, Field::Val), eq(Field::Val, 1, Field::Val)]),
            ]),
            pos(
                attr(b),
                vec![
                    eq(Field::Id, 1, Field::Id),
                    rel(Field::Val, Relation::Greater, Referent::Variable(VarLocation { depth: 1, field: Field::Val })),
                ],
            ),
        ],
        // (<s> ^b { <> 2 <=> 2 })  and  (<s> ^a << 1 3 >>)
        vec![pos(
            attr(b),
            vec![
                rel(Field::Val, Relation::NotEqual, Referent::Constant(two)),
                rel(Field::Val, Relation::SameType, Referent::Constant(two)),
            ],
        )],
        vec![pos(
            attr(a),
            vec![ReteTest::Disjunction { field: Field::Val, values: vec![d.values[0], d.values[2]] }],
        )],
        // (<s> ^item <i>) -(<i> ^a <any>) -(<s> ^b <any>)  -- two negations in a row
        vec![
            pos(attr(item), vec![]),
            neg(attr(a), vec![eq(Field::Id, 1, Field::Val)]),
            neg(attr(b), vec![eq(Field::Id, 1, Field::Id)]),
        ],
    ]
}

fn run(seed: u64, steps: usize, add_productions_midway: bool) {
    let mut rng = Rng(seed);
    let mut symbols = SymbolTable::new();
    let ids = vec![symbols.intern_id('S', 1), symbols.intern_id('S', 2), symbols.intern_id('I', 1)];
    symbols.mark_goal(ids[0]);
    symbols.mark_goal(ids[1]);
    let attrs = vec![symbols.intern_str("a"), symbols.intern_str("b"), symbols.intern_str("item")];
    let mut values = vec![symbols.intern_int(1), symbols.intern_int(2), symbols.intern_int(3), symbols.intern_float(2.5), symbols.intern_str("x")];
    values.extend(ids.iter().copied());
    let domain = Domain { ids, attrs, values };
    let prods = productions(&mut symbols, &domain);

    let mut rete = ReteNetwork::new();
    let mut names = Vec::new();
    let mut installed = 0;
    let install_at = if add_productions_midway { steps / 2 } else { 0 };

    let mut wm: Vec<(WmeKey, Triple)> = Vec::new();
    let mut next_key = 0;
    let mut live: BTreeMap<usize, (&'static str, Vec<WmeKey>)> = BTreeMap::new();

    for step in 0..=steps {
        if step == install_at && installed == 0 {
            for (i, conditions) in prods.iter().enumerate() {
                let name: &'static str = Box::leak(format!("p{i}").into_boxed_str());
                rete.add_production(name, conditions.clone(), &symbols).expect("capacity");
                names.push((name, conditions.clone()));
            }
            installed = prods.len();
        }

        if step < steps {
            if !wm.is_empty() && rng.below(5) < 2 {
                let (key, _) = wm.remove(rng.below(wm.len()));
                rete.remove_wme(key, &symbols);
            } else {
                let triple = (
                    domain.ids[rng.below(domain.ids.len())],
                    domain.attrs[rng.below(domain.attrs.len())],
                    domain.values[rng.below(domain.values.len())],
                );
                if !wm.iter().any(|(_, t)| *t == triple) {
                    let key = WmeKey(next_key);
                    next_key += 1;
                    wm.push((key, triple));
                    rete.add_wme(key, triple, false, &symbols).expect("capacity");
                }
            }
        }

        for inst in rete.take_retractions() {
            assert!(live.remove(&inst.id.0).is_some(), "seed {seed} step {step}: retracted unknown match");
        }
        for inst in rete.take_assertions() {
            let keys = inst.matched_wmes.iter().map(|w| w.key).collect();
            live.insert(inst.id.0, (inst.rule_name, keys));
        }

        for (name, conditions) in &names {
            let mut expected = Vec::new();
            matches(&symbols, &wm, conditions, Vec::new(), Vec::new(), &mut expected);
            expected.sort_by_key(|keys| keys.iter().map(|k| k.0).collect::<Vec<_>>());
            let mut actual: Vec<Vec<WmeKey>> = live
                .values()
                .filter(|(n, _)| n == name)
                .map(|(_, keys)| keys.clone())
                .collect();
            actual.sort_by_key(|keys| keys.iter().map(|k| k.0).collect::<Vec<_>>());
            assert_eq!(actual, expected, "seed {seed} step {step}: production {name} diverged");
        }
    }
}

#[test]
fn rete_matches_brute_force_on_random_histories() {
    for seed in 1..=300u64 {
        run(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 60, false);
    }
}

#[test]
fn rete_matches_brute_force_when_productions_are_added_to_populated_memory() {
    for seed in 1..=300u64 {
        run(seed.wrapping_mul(0xD1B5_4A32_D192_ED03), 60, true);
    }
}
