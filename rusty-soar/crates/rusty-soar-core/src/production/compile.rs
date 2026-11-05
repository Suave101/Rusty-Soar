//! Compiles parsed productions into Rete conditions and executable RHS actions.
//!
//! This is the semantic part of upstream `production_reorder.cpp` and the variable-binding part
//! of `rete.cpp` (SECTIONS 6-8):
//!
//! * Positive conditions are ordered so each one's identifier is a state/impasse root or is
//!   bound by an earlier condition; productions with no root, or with conditions not connected to
//!   one, are rejected as upstream does.
//! * Negated conditions and conjunctive negations are placed after every variable they share
//!   with the positive conditions is bound.
//! * A relational test whose referent is bound by a later condition is reversed and moved to
//!   that condition (upstream `restore_and_deallocate_saved_tests`); one whose referent is never
//!   bound is dropped with upstream's warning, or rejected inside a negation.
//! * RHS identifiers must be bound on the LHS or created as a value by another action.
//!
//! Upstream's cost-based choice among valid orders only affects matching speed, so a valid
//! order is chosen by source position instead.

use crate::rete::{AlphaTest, Condition, ConditionTests, Field, Referent, Relation, ReteTest, VarLocation};
use crate::soar_parser::{
    SoarAction, SoarCondition, SoarConditionTests, SoarPreference, SoarProduction, SoarRelation, SoarRhsValue,
    SoarSymbol, SoarTest,
};
use crate::symbol::{SymbolId, SymbolTable};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A RHS value resolved against the production's variable bindings.
#[derive(Debug, Clone, PartialEq)]
pub enum RhsTerm {
    /// A constant.
    Constant(SymbolId),
    /// A variable bound on the LHS.
    Bound(VarLocation),
    /// A variable not bound on the LHS: a new identifier is created for it on each firing.
    /// The index refers to [`CompiledProduction::new_identifier_letters`].
    NewIdentifier(usize),
    /// A RHS function call.
    Call {
        /// Function name.
        name: String,
        /// Arguments.
        args: Vec<RhsTerm>,
    },
}

/// An executable RHS action.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledAction {
    /// Creates a preference (for `^operator`) or a WME / WME rejection (other attributes).
    Make {
        /// Identifier.
        id: RhsTerm,
        /// Attribute.
        attr: RhsTerm,
        /// Value.
        value: RhsTerm,
        /// Preference type.
        preference: SoarPreference,
        /// Referent of a binary preference.
        referent: Option<RhsTerm>,
    },
    /// A stand-alone function call.
    Call(RhsTerm),
}

/// A production ready to be added to the Rete and fired.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledProduction {
    /// Production name.
    pub name: String,
    /// Conditions in Rete order.
    pub conditions: Vec<Condition>,
    /// The same conditions as written, in that order.
    pub ordered_conditions: Vec<SoarCondition>,
    /// Actions in source order.
    pub actions: Vec<CompiledAction>,
    /// Identifier letter for each RHS-unbound variable.
    pub new_identifier_letters: Vec<char>,
    /// Warnings upstream would print.
    pub warnings: Vec<String>,
}

/// Why a production was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError(pub String);

impl core::fmt::Display for CompileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn fail<T>(production: &str, message: impl core::fmt::Display) -> Result<T, CompileError> {
    Err(CompileError(format!("production {}: {}", production, message)))
}

/// Compiles a parsed production.
pub fn compile(production: &SoarProduction, symbols: &mut SymbolTable) -> Result<CompiledProduction, CompileError> {
    let name = production.name.as_str();
    let ordered = order_conditions(name, &production.conditions, &mut Vec::new(), true)?;

    let mut compiler = Compiler { name, symbols, scope: Vec::new(), deferred: Vec::new(), warnings: production.warnings.clone() };
    let conditions = compiler.compile_list(&ordered, 1, 0)?;
    compiler.finish_deferred_scope(0)?;

    let mut new_identifier_letters = Vec::new();
    let mut new_identifier_names: Vec<String> = Vec::new();
    let actions = production
        .actions
        .iter()
        .map(|action| compiler.compile_action(action, &mut new_identifier_names, &mut new_identifier_letters))
        .collect::<Result<Vec<_>, _>>()?;
    check_rhs_identifiers(name, &production.actions, &compiler.scope, &new_identifier_names)?;

    Ok(CompiledProduction {
        name: production.name.clone(),
        conditions,
        ordered_conditions: ordered,
        actions,
        new_identifier_letters,
        warnings: compiler.warnings,
    })
}

// ----------------------------------------------------------------------
// Ordering
// ----------------------------------------------------------------------

fn test_eq_var(test: &SoarTest) -> Option<&str> {
    match test.eq_test() {
        Some(SoarSymbol::Variable(name)) => Some(name),
        _ => None,
    }
}

fn is_root(tests: &SoarConditionTests) -> bool {
    fn has_goal(test: &SoarTest) -> bool {
        match test {
            SoarTest::Goal | SoarTest::Impasse => true,
            SoarTest::Conjunctive(tests) => tests.iter().any(has_goal),
            _ => false,
        }
    }
    has_goal(&tests.id)
}

fn equality_vars(tests: &SoarConditionTests) -> impl Iterator<Item = &str> {
    [&tests.id, &tests.attr, &tests.value].into_iter().filter_map(test_eq_var)
}

fn all_vars_in_test(test: &SoarTest, out: &mut Vec<String>) {
    match test {
        SoarTest::Relational(_, SoarSymbol::Variable(name)) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        SoarTest::Conjunctive(tests) => tests.iter().for_each(|t| all_vars_in_test(t, out)),
        _ => {}
    }
}

fn all_vars_in_condition(condition: &SoarCondition, out: &mut Vec<String>) {
    match condition {
        SoarCondition::Positive(t) | SoarCondition::Negative(t) => {
            all_vars_in_test(&t.id, out);
            all_vars_in_test(&t.attr, out);
            all_vars_in_test(&t.value, out);
        }
        SoarCondition::ConjunctiveNegation(sub) => sub.iter().for_each(|c| all_vars_in_condition(c, out)),
    }
}

/// Orders `conditions` given the variables already `bound` (which it extends with the
/// positives' bindings). `top_level` requires at least one state/impasse root.
fn order_conditions(
    name: &str,
    conditions: &[SoarCondition],
    bound: &mut Vec<String>,
    top_level: bool,
) -> Result<Vec<SoarCondition>, CompileError> {
    let mut positives: Vec<&SoarConditionTests> = conditions
        .iter()
        .filter_map(|c| match c {
            SoarCondition::Positive(t) => Some(t),
            _ => None,
        })
        .collect();
    let mut pending: Vec<&SoarCondition> = conditions.iter().filter(|c| !matches!(c, SoarCondition::Positive(_))).collect();

    if top_level && !positives.iter().any(|t| is_root(t)) {
        return fail(name, "has no positive conditions that test a state (state or impasse)");
    }

    // Variables the positives will bind: a negation must wait for those it mentions.
    let mut positive_vars: Vec<String> = bound.clone();
    for t in &positives {
        for var in equality_vars(t) {
            if !positive_vars.iter().any(|v| v == var) {
                positive_vars.push(var.to_string());
            }
        }
    }

    let mut ordered = Vec::new();
    loop {
        // Place every pending negation whose outer variables are now all bound.
        let mut i = 0;
        while i < pending.len() {
            let mut vars = Vec::new();
            all_vars_in_condition(pending[i], &mut vars);
            let ready = vars.iter().all(|v| !positive_vars.contains(v) || bound.contains(v));
            if ready {
                let condition = pending.remove(i);
                ordered.push(order_negation(name, condition, bound)?);
            } else {
                i += 1;
            }
        }

        if positives.is_empty() {
            break;
        }
        let next = positives
            .iter()
            .position(|t| is_root(t) || test_eq_var(&t.id).is_some_and(|v| bound.iter().any(|b| b == v)));
        let Some(index) = next else {
            let var = test_eq_var(&positives[0].id).unwrap_or("?");
            return fail(name, format!("identifier {} is not connected to any goal or impasse", var));
        };
        let tests = positives.remove(index);
        for var in equality_vars(tests) {
            if !bound.iter().any(|b| b == var) {
                bound.push(var.to_string());
            }
        }
        ordered.push(SoarCondition::Positive(tests.clone()));
    }
    Ok(ordered)
}

fn order_negation(name: &str, condition: &SoarCondition, bound: &[String]) -> Result<SoarCondition, CompileError> {
    match condition {
        SoarCondition::Negative(tests) => {
            if !test_eq_var(&tests.id).is_some_and(|v| bound.iter().any(|b| b == v)) {
                return fail(name, "a negated condition's identifier is not connected to any goal or impasse");
            }
            Ok(condition.clone())
        }
        SoarCondition::ConjunctiveNegation(sub) => {
            let mut inner_bound = bound.to_vec();
            Ok(SoarCondition::ConjunctiveNegation(order_conditions(name, sub, &mut inner_bound, false)?))
        }
        SoarCondition::Positive(_) => Ok(condition.clone()),
    }
}

// ----------------------------------------------------------------------
// Condition compilation
// ----------------------------------------------------------------------

struct Deferred {
    /// Variable the test waits for.
    var: String,
    /// Relation, already reversed to read `var relation referent`.
    relation: Relation,
    referent: Referent,
    /// Scope depth (number of open negation scopes) where the test was written.
    scope_level: usize,
}

struct Compiler<'a> {
    name: &'a str,
    symbols: &'a mut SymbolTable,
    /// Variable bindings visible at this point, innermost last.
    scope: Vec<(String, VarLocation, usize)>,
    deferred: Vec<Deferred>,
    warnings: Vec<String>,
}

fn reverse(relation: Relation) -> Relation {
    match relation {
        Relation::Less => Relation::Greater,
        Relation::Greater => Relation::Less,
        Relation::LessOrEqual => Relation::GreaterOrEqual,
        Relation::GreaterOrEqual => Relation::LessOrEqual,
        other => other,
    }
}

fn flatten(test: &SoarTest) -> Vec<&SoarTest> {
    match test {
        SoarTest::Conjunctive(tests) => tests.iter().flat_map(flatten).collect(),
        other => alloc::vec![other],
    }
}

impl<'a> Compiler<'a> {
    fn intern(&mut self, symbol: &SoarSymbol) -> SymbolId {
        match symbol {
            SoarSymbol::Str(s) => self.symbols.intern_str(s),
            SoarSymbol::Int(i) => self.symbols.intern_int(*i),
            SoarSymbol::Float(x) => self.symbols.intern_float(*x),
            SoarSymbol::Variable(v) => self.symbols.intern_str(v),
        }
    }

    fn lookup(&self, var: &str) -> Option<VarLocation> {
        self.scope.iter().rev().find(|(name, ..)| name == var).map(|(_, loc, _)| *loc)
    }

    /// Compiles a condition list starting at `depth`, inside `level` nested negations (whose
    /// bindings stay local to them).
    fn compile_list(&mut self, conditions: &[SoarCondition], depth: usize, level: usize) -> Result<Vec<Condition>, CompileError> {
        let mut out = Vec::with_capacity(conditions.len());
        for (offset, condition) in conditions.iter().enumerate() {
            let d = depth + offset;
            out.push(match condition {
                SoarCondition::Positive(tests) => Condition::Positive(self.compile_tests(tests, d, false, level)?),
                SoarCondition::Negative(tests) => {
                    let mark = self.scope.len();
                    let compiled = self.compile_tests(tests, d, true, level + 1)?;
                    self.finish_deferred_scope(level + 1)?;
                    self.scope.truncate(mark);
                    Condition::Negative(compiled)
                }
                SoarCondition::ConjunctiveNegation(sub) => {
                    let mark = self.scope.len();
                    // The NCC's subconditions start at the NCC's own level.
                    let compiled = self.compile_list(sub, d, level + 1)?;
                    self.finish_deferred_scope(level + 1)?;
                    self.scope.truncate(mark);
                    Condition::ConjunctiveNegation(compiled)
                }
            });
        }
        Ok(out)
    }

    fn compile_tests(
        &mut self,
        tests: &SoarConditionTests,
        depth: usize,
        negated: bool,
        scope_level: usize,
    ) -> Result<ConditionTests, CompileError> {
        let fields = [(Field::Id, &tests.id), (Field::Attr, &tests.attr), (Field::Val, &tests.value)];
        let mut alpha = AlphaTest::default();
        let mut rete_tests = Vec::new();
        let mut new_bindings: Vec<(String, VarLocation)> = Vec::new();

        // Pass 1: equality tests (constants go to the alpha memory, variables bind or join).
        for (field, test) in fields {
            for simple in flatten(test) {
                if let SoarTest::Relational(SoarRelation::Equal, symbol) = simple {
                    match symbol {
                        SoarSymbol::Variable(var) => {
                            if let Some(location) = self.lookup(var) {
                                rete_tests.push(eq_test(field, Referent::Variable(location)));
                            } else if let Some((_, location)) = new_bindings.iter().find(|(v, _)| v == var) {
                                rete_tests.push(eq_test(field, Referent::Variable(*location)));
                            } else {
                                new_bindings.push((var.clone(), VarLocation { depth, field }));
                            }
                        }
                        constant => {
                            let id = self.intern(constant);
                            let slot = match field {
                                Field::Id => &mut alpha.id,
                                Field::Attr => &mut alpha.attr,
                                Field::Val => &mut alpha.val,
                            };
                            match slot {
                                None => *slot = Some(id),
                                Some(_) => rete_tests.push(eq_test(field, Referent::Constant(id))),
                            }
                        }
                    }
                }
            }
        }
        for (var, location) in &new_bindings {
            self.scope.push((var.clone(), *location, scope_level));
        }

        // Pass 2: everything else.
        for (field, test) in fields {
            for simple in flatten(test) {
                match simple {
                    SoarTest::Relational(SoarRelation::Equal, _) => {}
                    SoarTest::Relational(relation, symbol) => {
                        let relation = match relation {
                            SoarRelation::NotEqual => Relation::NotEqual,
                            SoarRelation::Less => Relation::Less,
                            SoarRelation::Greater => Relation::Greater,
                            SoarRelation::LessOrEqual => Relation::LessOrEqual,
                            SoarRelation::GreaterOrEqual => Relation::GreaterOrEqual,
                            SoarRelation::SameType => Relation::SameType,
                            SoarRelation::ConstantMatch => Relation::ConstantMatch,
                            SoarRelation::SmemLink => Relation::SmemLink,
                            SoarRelation::SmemLinkNot => Relation::SmemLinkNot,
                            other => return fail(self.name, format!("{:?} tests are not supported yet", other)),
                        };
                        match symbol {
                            SoarSymbol::Variable(var) => match self.lookup(var) {
                                Some(location) => rete_tests.push(ReteTest::Relational {
                                    field,
                                    relation,
                                    referent: Referent::Variable(location),
                                }),
                                None => {
                                    if negated {
                                        return fail(
                                            self.name,
                                            format!("unbound referent {} in negated relational test", var),
                                        );
                                    }
                                    let referent = match test.eq_test() {
                                        Some(SoarSymbol::Variable(own)) => {
                                            Referent::Variable(self.lookup(own).unwrap_or(VarLocation { depth, field }))
                                        }
                                        Some(constant) => Referent::Constant(self.intern(constant)),
                                        None => Referent::Variable(VarLocation { depth, field }),
                                    };
                                    self.deferred.push(Deferred {
                                        var: var.clone(),
                                        relation: reverse(relation),
                                        referent,
                                        scope_level,
                                    });
                                }
                            },
                            constant => {
                                let id = self.intern(constant);
                                rete_tests.push(ReteTest::Relational { field, relation, referent: Referent::Constant(id) });
                            }
                        }
                    }
                    SoarTest::Disjunction(values) => {
                        let values = values.iter().map(|v| self.intern(v)).collect();
                        rete_tests.push(ReteTest::Disjunction { field, values });
                    }
                    SoarTest::Goal | SoarTest::Impasse if field != Field::Id => {
                        return fail(self.name, "state/impasse tests are only allowed on identifiers");
                    }
                    SoarTest::Goal => rete_tests.push(ReteTest::IdIsGoal),
                    SoarTest::Impasse => rete_tests.push(ReteTest::IdIsImpasse),
                    SoarTest::SmemLinkUnary => rete_tests.push(ReteTest::SmemLinked { field, linked: true }),
                    SoarTest::SmemLinkUnaryNot => rete_tests.push(ReteTest::SmemLinked { field, linked: false }),
                    SoarTest::Conjunctive(_) => {}
                }
            }
        }

        // Deferred tests waiting for a variable this condition binds attach here.
        if !negated {
            let mut i = 0;
            while i < self.deferred.len() {
                let resolved = new_bindings
                    .iter()
                    .find(|(v, _)| *v == self.deferred[i].var && self.deferred[i].scope_level == scope_level)
                    .map(|(_, loc)| *loc);
                match resolved {
                    Some(location) => {
                        let d = self.deferred.remove(i);
                        rete_tests.push(ReteTest::Relational { field: location.field, relation: d.relation, referent: d.referent });
                    }
                    None => i += 1,
                }
            }
        }

        Ok(ConditionTests { alpha, acceptable: tests.acceptable, tests: rete_tests })
    }

    /// Drops deferred tests from a closing scope whose referent never got bound.
    fn finish_deferred_scope(&mut self, scope_level: usize) -> Result<(), CompileError> {
        let name = self.name;
        let warnings = &mut self.warnings;
        self.deferred.retain(|d| {
            if d.scope_level >= scope_level {
                warnings.push(format!("Warning:  Ignoring test whose referent {} is unbound in production {}", d.var, name));
                false
            } else {
                true
            }
        });
        Ok(())
    }

    // ------------------------------------------------------------------
    // RHS
    // ------------------------------------------------------------------

    fn compile_rhs(
        &mut self,
        value: &SoarRhsValue,
        new_names: &mut Vec<String>,
        new_letters: &mut Vec<char>,
    ) -> RhsTerm {
        match value {
            SoarRhsValue::Symbol(SoarSymbol::Variable(var)) => {
                if let Some(location) = self.top_level_binding(var) {
                    return RhsTerm::Bound(location);
                }
                let index = match new_names.iter().position(|n| n == var) {
                    Some(index) => index,
                    None => {
                        new_names.push(var.clone());
                        // Upstream names a new identifier after the variable's first letter.
                        let letter = var.chars().nth(1).filter(char::is_ascii_alphabetic).unwrap_or('I');
                        new_letters.push(letter.to_ascii_uppercase());
                        new_names.len() - 1
                    }
                };
                RhsTerm::NewIdentifier(index)
            }
            SoarRhsValue::Symbol(constant) => RhsTerm::Constant(self.intern(constant)),
            SoarRhsValue::FunctionCall { name, args } => RhsTerm::Call {
                name: name.clone(),
                args: args.iter().map(|a| self.compile_rhs(a, new_names, new_letters)).collect(),
            },
        }
    }

    fn top_level_binding(&self, var: &str) -> Option<VarLocation> {
        self.scope.iter().rev().find(|(name, _, level)| name == var && *level == 0).map(|(_, loc, _)| *loc)
    }

    fn compile_action(
        &mut self,
        action: &SoarAction,
        new_names: &mut Vec<String>,
        new_letters: &mut Vec<char>,
    ) -> Result<CompiledAction, CompileError> {
        Ok(match action {
            SoarAction::Make { id, attr, value, preference, referent } => CompiledAction::Make {
                id: self.compile_rhs(&SoarRhsValue::Symbol(id.clone()), new_names, new_letters),
                attr: self.compile_rhs(attr, new_names, new_letters),
                value: self.compile_rhs(value, new_names, new_letters),
                preference: *preference,
                referent: referent.as_ref().map(|r| self.compile_rhs(r, new_names, new_letters)),
            },
            SoarAction::FunctionCall(call) => CompiledAction::Call(self.compile_rhs(call, new_names, new_letters)),
        })
    }
}

fn eq_test(field: Field, referent: Referent) -> ReteTest {
    ReteTest::Relational { field, relation: Relation::Equal, referent }
}

/// Upstream `legal_to_execute_action`: an RHS identifier must be bound on the LHS or be
/// created as the value of another action.
fn check_rhs_identifiers(
    name: &str,
    actions: &[SoarAction],
    scope: &[(String, VarLocation, usize)],
    new_names: &[String],
) -> Result<(), CompileError> {
    let created_as_value = |var: &str| {
        actions.iter().any(|a| match a {
            SoarAction::Make { value, referent, .. } => {
                *value == SoarRhsValue::Symbol(SoarSymbol::Variable(var.to_string()))
                    || referent.as_ref() == Some(&SoarRhsValue::Symbol(SoarSymbol::Variable(var.to_string())))
            }
            SoarAction::FunctionCall(_) => false,
        })
    };
    for action in actions {
        if let SoarAction::Make { id: SoarSymbol::Variable(var), .. } = action {
            let bound = scope.iter().any(|(n, _, level)| n == var && *level == 0);
            if !bound && new_names.contains(var) && !created_as_value(var) {
                return fail(name, format!("the identifier {} on the RHS is not bound on the LHS or created by another action", var));
            }
        }
    }
    Ok(())
}
