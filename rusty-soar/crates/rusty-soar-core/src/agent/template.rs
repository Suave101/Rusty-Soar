//! Reinforcement-learning templates (upstream `rl_build_template_instantiation` and
//! `ebc_variablize_rl.cpp`).
//!
//! A `:template` rule has one action, a numeric-indifferent preference. It makes no preference
//! when it fires; instead each new match becomes a new RL rule, `rl*<template>*<n>`, whose
//! conditions are the WMEs the match tested with their identifiers made variables again, and
//! whose value starts at the template's.

use super::SoarAgent;
use crate::pool::Layout;
use crate::rete::Instantiation;
use crate::soar_parser::{
    SoarAction, SoarCondition, SoarConditionTests, SoarPreference, SoarProduction, SoarProductionType, SoarRelation, SoarRhsValue,
    SoarSupport, SoarSymbol, SoarTest,
};
use crate::symbol::{SymbolData, SymbolId};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Upstream `rl_valid_template`: one action, a numeric-indifferent preference or a binary
/// indifferent one with a variable referent.
pub(super) fn valid_template(production: &SoarProduction) -> bool {
    matches!(
        production.actions.as_slice(),
        [SoarAction::Make { preference: SoarPreference::NumericIndifferent, .. }]
            | [SoarAction::Make { preference: SoarPreference::BinaryIndifferent, referent: Some(SoarRhsValue::Symbol(SoarSymbol::Variable(_))), .. }]
    )
}

/// Upstream `rl_get_template_id`: the number ending a name of the form `rl*<name>*<n>`.
pub(super) fn template_id(name: &str) -> Option<u64> {
    if name.len() < 6 || !name.starts_with("rl*") {
        return None;
    }
    let id = &name[name.rfind('*')? + 1..];
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}

/// The new rule's variables: one per identifier, named by its letter as upstream's variable
/// generator does.
#[derive(Default)]
struct Variables {
    of: BTreeMap<SymbolId, String>,
    counters: BTreeMap<char, u32>,
}

impl<S: Layout> SoarAgent<S> {
    /// Upstream `rl_build_template_instantiation`, for a match of the template `inst.rule_name`.
    pub(super) fn rl_build_template_instantiation(&mut self, inst: &Instantiation) {
        let Some(source) = self.chunker.sources.get(inst.rule_name).cloned() else { return };
        let Some(SoarAction::Make { id, attr, value, referent, .. }) = source.actions.first() else { return };

        // What each of the template's variables matched.
        let mut bound: BTreeMap<String, SymbolId> = BTreeMap::new();
        for (condition, triple) in source.conditions.iter().zip(&inst.levels) {
            if let (SoarCondition::Positive(tests), Some((i, a, v))) = (condition, triple) {
                for (test, symbol) in [(&tests.id, *i), (&tests.attr, *a), (&tests.value, *v)] {
                    if let Some(SoarSymbol::Variable(name)) = test.eq_test() {
                        bound.entry(name.clone()).or_insert(symbol);
                    }
                }
            }
        }

        // The action, instantiated (upstream `variablize_rl_action`). The value must be a number.
        let id_symbol = self.rl_resolve(&bound, &SoarRhsValue::Symbol(id.clone()));
        let (Some(id_symbol), Some(attr_symbol), Some(value_symbol)) = (id_symbol, self.rl_resolve(&bound, attr), self.rl_resolve(&bound, value)) else {
            return;
        };
        let referent = referent.as_ref().and_then(|r| self.rl_resolve(&bound, r));
        let Some(initial) = referent.and_then(|r| self.symbols.number(r)) else { return };

        let mut variables = Variables::default();
        let mut tested_goals = Vec::new();
        let mut conditions = Vec::new();
        for (condition, triple) in source.conditions.iter().zip(&inst.levels) {
            match (condition, triple) {
                (SoarCondition::Positive(tests), Some((i, a, v))) => {
                    let mut id_test = self.rl_equality(&mut variables, *i);
                    // Upstream `rl_add_goal_or_impasse_tests_to_conds`, once per identifier.
                    if !tested_goals.contains(i) && (self.symbols.is_goal(*i) || self.is_impasse(*i)) {
                        let kind = if self.symbols.is_goal(*i) { SoarTest::Goal } else { SoarTest::Impasse };
                        id_test = SoarTest::Conjunctive(alloc::vec![id_test, kind]);
                        tested_goals.push(*i);
                    }
                    let attr_test = self.rl_field(&mut variables, &bound, &tests.attr, *a);
                    let value_test = self.rl_field(&mut variables, &bound, &tests.value, *v);
                    conditions.push(SoarCondition::Positive(SoarConditionTests { id: id_test, attr: attr_test, value: value_test, acceptable: tests.acceptable }));
                }
                (other, _) => conditions.push(self.rl_negated(&mut variables, &bound, other)),
            }
        }

        let term = |agent: &Self, variables: &mut Variables, symbol: SymbolId| agent.rl_term(variables, symbol);
        let action = SoarAction::Make {
            id: term(self, &mut variables, id_symbol),
            attr: SoarRhsValue::Symbol(term(self, &mut variables, attr_symbol)),
            value: SoarRhsValue::Symbol(term(self, &mut variables, value_symbol)),
            preference: SoarPreference::NumericIndifferent,
            referent: Some(SoarRhsValue::Symbol(SoarSymbol::Float(initial))),
        };

        // A match that has already made a rule makes it again: upstream finds the duplicate in
        // the Rete and gives the number back.
        let body = (conditions, action);
        if self.rl.template_rules.contains(&body) {
            return;
        }
        let template = inst.rule_name;
        let name = loop {
            let n = self.rl.template_count.max(1);
            self.rl.template_count = n + 1;
            let name = format!("rl*{}*{}", template, n);
            if self.symbols.lookup_str(&name).is_none() {
                break name;
            }
        };
        self.symbols.intern_str(&name);
        let production = SoarProduction {
            name,
            documentation: None,
            support: SoarSupport::Undeclared,
            production_type: SoarProductionType::User,
            interrupt: false,
            explain: false,
            conditions: body.0.clone(),
            actions: alloc::vec![body.1.clone()],
            warnings: Vec::new(),
        };
        if self.install_production(&production).is_ok() {
            self.rl.template_rules.push(body);
        } else {
            self.rl.template_count -= 1;
        }
    }

    fn is_impasse(&self, _id: SymbolId) -> bool {
        // Rusty-Soar's impasses are goals; it has no separate impasse identifiers.
        false
    }

    /// An action's value: what its variable matched, or the constant.
    fn rl_resolve(&mut self, bound: &BTreeMap<String, SymbolId>, value: &SoarRhsValue) -> Option<SymbolId> {
        match value {
            SoarRhsValue::Symbol(SoarSymbol::Variable(v)) => bound.get(v).copied(),
            SoarRhsValue::Symbol(SoarSymbol::Str(s)) => Some(self.symbols.intern_str(s)),
            SoarRhsValue::Symbol(SoarSymbol::Int(i)) => Some(self.symbols.intern_int(*i)),
            SoarRhsValue::Symbol(SoarSymbol::Float(x)) => Some(self.symbols.intern_float(*x)),
            _ => None,
        }
    }

    /// A symbol in the new rule: an identifier's variable, or the constant itself.
    fn rl_term(&self, variables: &mut Variables, symbol: SymbolId) -> SoarSymbol {
        match self.symbols.resolve(symbol) {
            Some(SymbolData::String(s)) => SoarSymbol::Str(s.clone()),
            Some(SymbolData::Integer(i)) => SoarSymbol::Int(*i),
            Some(SymbolData::Float(x)) => SoarSymbol::Float(*x),
            Some(SymbolData::Identifier { prefix, .. }) => {
                let letter = prefix.to_ascii_lowercase();
                if let Some(name) = variables.of.get(&symbol) {
                    return SoarSymbol::Variable(name.clone());
                }
                let n = variables.counters.entry(letter).or_insert(0);
                *n += 1;
                let name = format!("<{}{}>", letter, *n);
                variables.of.insert(symbol, name.clone());
                SoarSymbol::Variable(name)
            }
            None => SoarSymbol::Str(String::new()),
        }
    }

    fn rl_equality(&self, variables: &mut Variables, symbol: SymbolId) -> SoarTest {
        SoarTest::Relational(SoarRelation::Equal, self.rl_term(variables, symbol))
    }

    /// A positive condition's attribute or value test: the WME's symbol, with the template's
    /// equality and inequality tests against variables bound to identifiers (upstream
    /// `add_explanation_to_RL_condition`).
    fn rl_field(&self, variables: &mut Variables, bound: &BTreeMap<String, SymbolId>, test: &SoarTest, symbol: SymbolId) -> SoarTest {
        let mut tests = alloc::vec![self.rl_equality(variables, symbol)];
        let parts: Vec<&SoarTest> = match test {
            SoarTest::Conjunctive(parts) => parts.iter().collect(),
            other => alloc::vec![other],
        };
        for part in parts {
            if let SoarTest::Relational(relation @ (SoarRelation::Equal | SoarRelation::NotEqual), SoarSymbol::Variable(v)) = part {
                let Some(&other) = bound.get(v) else { continue };
                if self.symbols.is_identifier(other) && !(*relation == SoarRelation::Equal && other == symbol) {
                    tests.push(SoarTest::Relational(*relation, self.rl_term(variables, other)));
                }
            }
        }
        if tests.len() == 1 { tests.pop().unwrap_or(SoarTest::Goal) } else { SoarTest::Conjunctive(tests) }
    }

    /// A negated condition with the template's bound variables replaced by what they matched.
    fn rl_negated(&self, variables: &mut Variables, bound: &BTreeMap<String, SymbolId>, condition: &SoarCondition) -> SoarCondition {
        match condition {
            SoarCondition::Positive(tests) => SoarCondition::Positive(self.rl_negated_tests(variables, bound, tests)),
            SoarCondition::Negative(tests) => SoarCondition::Negative(self.rl_negated_tests(variables, bound, tests)),
            SoarCondition::ConjunctiveNegation(sub) => {
                SoarCondition::ConjunctiveNegation(sub.iter().map(|c| self.rl_negated(variables, bound, c)).collect())
            }
        }
    }

    fn rl_negated_tests(&self, variables: &mut Variables, bound: &BTreeMap<String, SymbolId>, tests: &SoarConditionTests) -> SoarConditionTests {
        SoarConditionTests {
            id: self.rl_negated_test(variables, bound, &tests.id),
            attr: self.rl_negated_test(variables, bound, &tests.attr),
            value: self.rl_negated_test(variables, bound, &tests.value),
            acceptable: tests.acceptable,
        }
    }

    fn rl_negated_test(&self, variables: &mut Variables, bound: &BTreeMap<String, SymbolId>, test: &SoarTest) -> SoarTest {
        let symbol = |variables: &mut Variables, s: &SoarSymbol| match s {
            SoarSymbol::Variable(v) => match bound.get(v) {
                Some(&value) => self.rl_term(variables, value),
                // A variable only the negation binds keeps a name of its own.
                None => SoarSymbol::Variable(format!("<{}*t>", v.trim_start_matches('<').trim_end_matches('>'))),
            },
            other => other.clone(),
        };
        match test {
            SoarTest::Relational(relation, s) => SoarTest::Relational(*relation, symbol(variables, s)),
            SoarTest::Conjunctive(parts) => SoarTest::Conjunctive(parts.iter().map(|t| self.rl_negated_test(variables, bound, t)).collect()),
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::template_id;

    #[test]
    fn template_ids_end_rl_names() {
        assert_eq!(template_id("rl*move*12"), Some(12));
        assert_eq!(template_id("rl*a*b*3"), Some(3));
        assert_eq!(template_id("rl*move*"), None);
        assert_eq!(template_id("move*12"), None);
        assert_eq!(template_id("rl*x*1a"), None);
    }
}
