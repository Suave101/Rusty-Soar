//! Upstream's context-decision trace (`watch 1`, the default trace level), printed after each
//! decide phase by `print_lowest_slot_in_context_stack`. Its lines are what the differential
//! test compares against a run of upstream Soar.
//!
//! Only upstream's default trace formats are supported:
//! - stack: `%right[6,%dc]: %rsd[   ]   O: %co` and `%right[6,%dc]: %rsd[   ]==>S: %cs`
//! - states: `%id %ifdef[(%v[attribute] %v[impasse])]`
//! - operators named `evaluate-object`: `%id (evaluate-object %o[object])`
//! - anything else: `%id %ifdef[(%v[name])]`

use super::SoarAgent;
use crate::soar_parser::rereadable_string;
use crate::symbol::{SymbolData, SymbolId, SymbolTable};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Upstream `Symbol::to_string(rereadable)`: floats in fixed notation with 6 decimals
/// (`DEFAULT_DECIMAL_PRECISION`), and strings in `|bars|` when `rereadable` and they would not
/// read back as the same string constant.
pub fn symbol_to_string(symbols: &SymbolTable, symbol: SymbolId, rereadable: bool) -> String {
    match symbols.resolve(symbol) {
        Some(SymbolData::String(s)) if rereadable => rereadable_string(s),
        Some(SymbolData::String(s)) => s.clone(),
        Some(SymbolData::Integer(i)) => format!("{}", i),
        Some(SymbolData::Float(x)) => format!("{:.6}", x),
        Some(SymbolData::Identifier { prefix, number }) => format!("{}{}", prefix, number),
        None => String::new(),
    }
}

impl<S: crate::pool::Layout> SoarAgent<S> {
    /// The trace line upstream prints after a decide phase for the lowest context slot: the
    /// bottom goal's operator if it has one, otherwise the bottom goal itself. `None` before the
    /// top goal exists.
    pub fn decision_trace_line(&self) -> Option<String> {
        let goal = self.decider.goals.last()?;
        let depth = self.decider.goals.len() - 1;
        let mut line = format!("{:>6}: {}", self.cycle_counter, "   ".repeat(depth));
        match goal.operator {
            Some((operator, _)) => {
                line.push_str("   O: ");
                line.push_str(&self.object_trace(operator, &mut Vec::new()));
            }
            None => {
                line.push_str("==>S: ");
                line.push_str(&self.object_trace(goal.id, &mut Vec::new()));
            }
        }
        Some(line)
    }

    /// Writes the decision trace line to `output` when `trace_decisions` is set, preceded by a
    /// newline as upstream prints it (`printa("\n")` then the line).
    pub(super) fn print_decision_trace(&mut self) {
        if !self.trace_decisions {
            return;
        }
        if let Some(line) = self.decision_trace_line() {
            self.output.push('\n');
            self.output.push_str(&line);
        }
    }

    /// Upstream `object_to_trace_string`. `printing` holds the objects being printed, which
    /// print as bare identifiers to stop cycles (upstream's `tf_printing_tc`).
    fn object_trace(&self, object: SymbolId, printing: &mut Vec<SymbolId>) -> String {
        let is_identifier = matches!(self.symbols.resolve(object), Some(SymbolData::Identifier { .. }));
        if !is_identifier || printing.contains(&object) {
            return symbol_to_string(&self.symbols, object, true);
        }
        printing.push(object);
        let mut text = symbol_to_string(&self.symbols, object, true);
        if self.symbols.is_goal(object) {
            text.push(' ');
            if let (Some(attribute), Some(impasse)) = (self.trace_values(object, "attribute"), self.trace_values(object, "impasse")) {
                text.push_str(&format!("({} {})", attribute, impasse));
            }
        } else if self.is_named(object, "evaluate-object") {
            text.push_str(" (evaluate-object ");
            let values: Vec<SymbolId> = self.attribute_values(object, "object");
            let parts: Vec<String> = values.into_iter().map(|v| self.object_trace(v, printing)).collect();
            text.push_str(&parts.join(" "));
            text.push(')');
        } else {
            text.push(' ');
            if let Some(name) = self.trace_values(object, "name") {
                text.push_str(&format!("({})", name));
            }
        }
        printing.pop();
        text
    }

    /// Upstream `%v[attr]`: the values of `attr`, space separated, or `None` (undefined) when
    /// there are none.
    fn trace_values(&self, object: SymbolId, attr: &str) -> Option<String> {
        let values = self.attribute_values(object, attr);
        if values.is_empty() {
            return None;
        }
        let parts: Vec<String> = values.into_iter().map(|v| symbol_to_string(&self.symbols, v, true)).collect();
        Some(parts.join(" "))
    }

    /// Values of `(object ^attr *)` WMEs, excluding acceptable-preference WMEs. Upstream lists
    /// impasse WMEs, then input WMEs, then slot WMEs newest first; with several values this
    /// lists them in arena order, so only single-valued attributes print identically.
    fn attribute_values(&self, object: SymbolId, attr: &str) -> Vec<SymbolId> {
        let Some(attr) = self.symbols.lookup_str(attr) else { return Vec::new() };
        self.wm.iter().filter(|w| w.id == object && w.attr == attr && !w.acceptable).map(|w| w.val).collect()
    }

    /// Upstream `find_name_of_object` compared against `name`.
    fn is_named(&self, object: SymbolId, name: &str) -> bool {
        let Some(name) = self.symbols.lookup_str(name) else { return false };
        let attr = self.symbols.predefined.name;
        self.wm.iter().any(|w| w.id == object && w.attr == attr && w.val == name && !w.acceptable)
    }
}
