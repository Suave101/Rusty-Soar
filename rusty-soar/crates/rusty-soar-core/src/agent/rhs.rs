//! Built-in RHS functions, ported from upstream `rhs_functions.cpp` and
//! `rhs_functions_math.cpp`. Functions not listed here are rejected when a production is
//! installed rather than failing when it fires.

use super::SoarAgent;
use crate::production::compile::RhsTerm;
use crate::symbol::{SymbolData, SymbolId, SymbolTable};
use alloc::format;
use alloc::string::String;

/// (name, argument count or `None` for any, usable as a value, usable as a stand-alone action)
const FUNCTIONS: &[(&str, Option<usize>, bool, bool)] = &[
    ("+", None, true, false),
    ("*", None, true, false),
    ("-", None, true, false),
    ("/", None, true, false),
    ("div", Some(2), true, false),
    ("mod", Some(2), true, false),
    ("abs", Some(1), true, false),
    ("int", Some(1), true, false),
    ("float", Some(1), true, false),
    ("crlf", Some(0), true, false),
    ("make-constant-symbol", None, true, false),
    ("ifeq", Some(4), true, false),
    ("write", None, false, true),
    ("halt", Some(0), false, true),
];

/// Upstream's parse-time checks: the function exists, may be used here, and has the right
/// number of arguments.
pub(super) fn check_term(term: &RhsTerm, as_value: bool) -> Result<(), String> {
    let RhsTerm::Call { name, args } = term else { return Ok(()) };
    let Some((_, arity, can_be_value, can_be_action)) = FUNCTIONS.iter().find(|(n, ..)| n == name) else {
        return Err(format!("RHS function {} is not supported", name));
    };
    if as_value && !can_be_value {
        return Err(format!("function {} cannot be used as a value", name));
    }
    if !as_value && !can_be_action {
        return Err(format!("function {} cannot be used as a stand-alone action", name));
    }
    if arity.is_some_and(|n| n != args.len()) {
        return Err(format!("function {} expects {} argument(s)", name, arity.unwrap_or(0)));
    }
    args.iter().try_for_each(|arg| check_term(arg, true))
}

#[derive(Clone, Copy)]
enum Number {
    Int(i64),
    Float(f64),
}

fn number(symbols: &SymbolTable, symbol: SymbolId) -> Option<Number> {
    match symbols.resolve(symbol)? {
        SymbolData::Integer(i) => Some(Number::Int(*i)),
        SymbolData::Float(x) => Some(Number::Float(*x)),
        _ => None,
    }
}

fn to_f64(n: Number) -> f64 {
    match n {
        Number::Int(i) => i as f64,
        Number::Float(x) => x,
    }
}

/// Text of a symbol as `write` prints it (upstream `to_string()`, not rereadable).
pub(super) fn symbol_text(symbols: &SymbolTable, symbol: SymbolId) -> String {
    super::symbol_to_string(symbols, symbol, false)
}

fn make_number(symbols: &mut SymbolTable, n: Number) -> SymbolId {
    match n {
        Number::Int(i) => symbols.intern_int(i),
        Number::Float(x) => symbols.intern_float(x),
    }
}

/// Upstream `plus_rhs_function_code` / `times_rhs_function_code`: integer arithmetic until the
/// first float argument, float from then on.
fn fold(args: &[Number], int_op: fn(i64, i64) -> i64, float_op: fn(f64, f64) -> f64, start: i64) -> Number {
    let mut acc = Number::Int(start);
    for &arg in args {
        acc = match (acc, arg) {
            (Number::Int(a), Number::Int(b)) => Number::Int(int_op(a, b)),
            (a, b) => Number::Float(float_op(to_f64(a), to_f64(b))),
        };
    }
    acc
}

/// Calls a built-in function; `None` means the call failed and the action is skipped, as an
/// upstream function returning NIL.
pub(super) fn call<S: crate::pool::Layout>(agent: &mut SoarAgent<S>, name: &str, args: &[SymbolId]) -> Option<SymbolId> {
    let symbols = &mut agent.symbols;
    let numbers = || args.iter().map(|a| number(symbols, *a)).collect::<Option<alloc::vec::Vec<_>>>();
    match name {
        "+" => {
            let n = numbers()?;
            Some(make_number(symbols, fold(&n, i64::wrapping_add, |a, b| a + b, 0)))
        }
        "*" => {
            let n = numbers()?;
            Some(make_number(symbols, fold(&n, i64::wrapping_mul, |a, b| a * b, 1)))
        }
        "-" => {
            let n = numbers()?;
            let result = match n.as_slice() {
                [] => return None,
                [Number::Int(i)] => Number::Int(i.wrapping_neg()),
                [Number::Float(x)] => Number::Float(-x),
                [first, rest @ ..] => {
                    let mut acc = *first;
                    for &arg in rest {
                        acc = match (acc, arg) {
                            (Number::Int(a), Number::Int(b)) => Number::Int(a.wrapping_sub(b)),
                            (a, b) => Number::Float(to_f64(a) - to_f64(b)),
                        };
                    }
                    acc
                }
            };
            Some(make_number(symbols, result))
        }
        "/" => {
            // Upstream `fp_divide_rhs_function_code`: always a float; division by zero fails.
            let n = numbers()?;
            let result = match n.as_slice() {
                [] => return None,
                [only] => {
                    let x = to_f64(*only);
                    if x == 0.0 {
                        return None;
                    }
                    1.0 / x
                }
                [first, rest @ ..] => {
                    let mut acc = to_f64(*first);
                    for &arg in rest {
                        let d = to_f64(arg);
                        if d == 0.0 {
                            return None;
                        }
                        acc /= d;
                    }
                    acc
                }
            };
            Some(symbols.intern_float(result))
        }
        "div" | "mod" => match numbers()?.as_slice() {
            [Number::Int(_), Number::Int(0)] => None,
            [Number::Int(a), Number::Int(b)] => {
                let result = if name == "div" { a.wrapping_div(*b) } else { a.wrapping_rem(*b) };
                Some(symbols.intern_int(result))
            }
            _ => None,
        },
        "abs" => match numbers()?.as_slice() {
            [Number::Int(i)] => Some(symbols.intern_int(i.wrapping_abs())),
            [Number::Float(x)] => Some(symbols.intern_float(if *x < 0.0 { -x } else { *x })),
            _ => None,
        },
        "int" => {
            let value = match symbols.resolve(args[0])? {
                SymbolData::Integer(i) => *i,
                SymbolData::Float(x) => *x as i64,
                SymbolData::String(s) => s.parse::<i64>().ok().or_else(|| s.parse::<f64>().ok().map(|x| x as i64))?,
                SymbolData::Identifier { .. } => return None,
            };
            Some(symbols.intern_int(value))
        }
        "float" => {
            let value = match symbols.resolve(args[0])? {
                SymbolData::Integer(i) => *i as f64,
                SymbolData::Float(x) => *x,
                SymbolData::String(s) => s.parse::<f64>().ok()?,
                SymbolData::Identifier { .. } => return None,
            };
            Some(symbols.intern_float(value))
        }
        "crlf" => Some(symbols.predefined.crlf),
        // Upstream `make_constant_symbol_rhs_function_code`: with no arguments, `constant` if no
        // such symbol exists yet; otherwise always a new symbol, the text followed by the
        // agent's counter (upstream `generate_new_str_constant` with `mcs_counter`).
        "make-constant-symbol" => {
            let prefix: String = if args.is_empty() {
                String::from("constant")
            } else {
                args.iter().map(|a| symbol_text(symbols, *a)).collect()
            };
            if args.is_empty() && symbols.lookup_str(&prefix).is_none() {
                return Some(symbols.intern_str(&prefix));
            }
            loop {
                let name = format!("{}{}", prefix, agent.mcs_counter);
                agent.mcs_counter += 1;
                if symbols.lookup_str(&name).is_none() {
                    return Some(symbols.intern_str(&name));
                }
            }
        }
        "ifeq" => Some(if args[0] == args[1] { args[2] } else { args[3] }),
        "write" => {
            for arg in args {
                let text = symbol_text(&agent.symbols, *arg);
                agent.output.push_str(&text);
            }
            None
        }
        "halt" => {
            agent.halted = true;
            None
        }
        _ => None,
    }
}
