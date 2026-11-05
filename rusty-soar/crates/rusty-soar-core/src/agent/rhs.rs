//! Built-in RHS functions, ported from upstream `rhs_functions.cpp`, `rhs_functions_math.cpp`
//! and SML's `concat`. Functions not listed here are rejected when a production is installed
//! rather than failing when it fires.
//!
//! Not ported: `accept` (reads standard input), `timestamp` (reads the clock), `deep-copy`,
//! `from-st-xml`, and SML's `exec`, `cmd` and `interrupt`.

use super::SoarAgent;
use crate::production::compile::RhsTerm;
use crate::symbol::{SymbolData, SymbolId, SymbolTable};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// (name, argument count or `None` for any, usable as a value, usable as a stand-alone action),
/// as upstream registers them.
const FUNCTIONS: &[(&str, Option<usize>, bool, bool)] = &[
    // Stand-alone actions.
    ("count", None, false, true),
    ("dont-learn", Some(1), false, true),
    ("force-learn", Some(1), false, true),
    ("halt", Some(0), false, true),
    ("link-stm-to-ltm", Some(2), false, true),
    ("wait", Some(1), false, true),
    ("write", None, false, true),
    ("trace", None, false, true),
    ("log", None, false, true),
    // Simple values.
    ("@", Some(1), true, false),
    ("capitalize-symbol", Some(1), true, false),
    ("crlf", Some(0), true, false),
    ("dc", Some(0), true, false),
    ("make-constant-symbol", None, true, false),
    ("strlen", Some(1), true, false),
    ("trim", Some(1), true, false),
    ("string", Some(1), true, false),
    ("ifeq", Some(4), true, false),
    // SML's.
    ("concat", None, true, false),
    // Math.
    ("+", None, true, false),
    ("*", None, true, false),
    ("-", None, true, false),
    ("/", None, true, false),
    ("div", Some(2), true, false),
    ("size", Some(1), true, false),
    ("sum", Some(1), true, false),
    ("product", Some(1), true, false),
    ("mod", Some(2), true, false),
    ("min", None, true, false),
    ("max", None, true, false),
    ("set-count", None, true, false),
    ("set-sum", None, true, false),
    ("set-multiply", None, true, false),
    ("set-min", None, true, false),
    ("set-max", None, true, false),
    ("set-range", None, true, false),
    ("set-mean", None, true, false),
    ("set-stdev", None, true, false),
    ("sin", Some(1), true, false),
    ("cos", Some(1), true, false),
    ("atan2", Some(2), true, false),
    ("sqrt", Some(1), true, false),
    ("abs", Some(1), true, false),
    ("int", Some(1), true, false),
    ("float", Some(1), true, false),
    ("round-off-heading", Some(2), true, false),
    ("round-off", Some(2), true, false),
    ("compute-heading", Some(4), true, false),
    ("compute-range", Some(4), true, false),
    ("extrapolate-x-position", Some(4), true, false),
    ("extrapolate-y-position", Some(4), true, false),
    ("select-point-closest-to-vector", Some(4), true, false),
    ("haversine", Some(4), true, false),
    ("compute-dice-probability", Some(4), true, false),
    ("rand-int", None, true, false),
    ("rand-float", None, true, false),
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

/// Upstream's cast of a number to an integer (`static_cast<int64_t>` of a float).
fn to_i64(n: Number) -> i64 {
    match n {
        Number::Int(i) => i,
        Number::Float(x) => x as i64,
    }
}

/// Text of a symbol as `write` prints it (upstream `to_string()`, not rereadable).
pub(super) fn symbol_text(symbols: &SymbolTable, symbol: SymbolId) -> String {
    super::symbol_to_string(symbols, symbol, false)
}

/// A symbol as upstream's `%y` prints it in a message.
fn shown(symbols: &SymbolTable, symbol: SymbolId) -> String {
    super::symbol_to_string(symbols, symbol, true)
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

/// C's `strtoll(text, NULL, 10)`: leading space, a sign and digits; 0 if there are none.
/// `None` when the number overflows (`errno` is set).
fn strtoll(text: &str) -> Option<i64> {
    let t = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = digits.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return Some(0);
    }
    let value: i128 = digits[..end].parse().unwrap_or(i128::MAX);
    let value = if negative { -value } else { value };
    i64::try_from(value).ok()
}

/// C's `strtod(text, NULL)`: the longest leading decimal number, 0 if there is none. `None`
/// when it overflows (`errno` is set).
fn strtod(text: &str) -> Option<f64> {
    let t = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let lower = t.to_ascii_lowercase();
    for (word, value) in [("infinity", f64::INFINITY), ("inf", f64::INFINITY), ("nan", f64::NAN)] {
        for (sign, factor) in [("", 1.0), ("+", 1.0), ("-", -1.0)] {
            if lower.starts_with(&format!("{}{}", sign, word)) {
                return Some(factor * value);
            }
        }
    }
    let b = t.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start || (i == digits_start + 1 && b[digits_start] == b'.') {
        return Some(0.0);
    }
    let mut end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exponent_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exponent_start {
            end = j;
        }
    }
    let value: f64 = t[..end].parse().ok()?;
    value.is_finite().then_some(value)
}

/// Upstream's `PI`, shorter than `core::f64::consts::PI`; results depend on it.
#[allow(clippy::approx_constant)]
const PI: f64 = 3.141592653589;

/// Upstream `normalize_heading_int`.
fn normalize_heading_int(mut n: i64) -> i64 {
    while n <= -180 {
        n += 360;
    }
    while n > 180 {
        n -= 360;
    }
    n
}

/// Upstream `normalize_heading_float`.
fn normalize_heading_float(mut n: f64) -> f64 {
    while n <= -180.0 {
        n += 360.0;
    }
    while n > 180.0 {
        n -= 360.0;
    }
    n
}

/// Upstream `round_off_heading_int`: `n` rounded to the nearest multiple of `m`.
fn round_off_heading_int(n: i64, m: i64) -> i64 {
    if n < 0 { m * ((n - (m / 2)) / m) } else { m * ((n + (m / 2)) / m) }
}

/// Upstream `round_off_heading_float`, in tenths.
fn round_off_heading_float(n: f64, m: f64) -> f64 {
    let n_10 = libm::trunc(n * 10.0);
    let m_10 = libm::trunc(m * 10.0);
    let half = libm::trunc(m_10 / 2.0);
    let rounded = if n_10 < 0.0 { m_10 * libm::trunc((n_10 - half) / m_10) } else { m_10 * libm::trunc((n_10 + half) / m_10) };
    rounded / 10.0
}

/// Upstream `heading_to_point`: the compass heading in whole degrees from one point to another.
fn heading_to_point(current_x: i64, current_y: i64, x: i64, y: i64) -> i64 {
    let mut dir = [(x - current_x) as f64, (y - current_y) as f64];
    // vec2_norm
    let mag = libm::sqrt(dir[0] * dir[0] + dir[1] * dir[1]);
    if mag < 0.01 {
        dir = [1.0, 0.0];
    } else {
        dir = [dir[0] / mag, dir[1] / mag];
    }
    // convert_to_soar_angle
    let mut heading = libm::atan2(dir[1], dir[0]) - PI / 2.0;
    if heading < 0.0 {
        heading += PI * 2.0;
    }
    heading = PI * 2.0 - heading;
    if heading > PI {
        heading -= PI * 2.0;
    }
    // bracket_rad_to_deg and air_soar_round_off_angle with m = 1
    let n = ((heading * 180.0) / PI) as i64;
    let mut bounded = round_off_heading_int(n, 1) % 360;
    if bounded > 180 {
        bounded -= 360;
    }
    if bounded <= -180 {
        bounded += 360;
    }
    bounded
}

/// Upstream `_dice_binom`.
fn dice_binom(n: u64, m: u64) -> u64 {
    let mut b = alloc::vec![0u64; n as usize + 1];
    b[0] = 1;
    for i in 1..=n as usize {
        b[i] = 1;
        for j in (1..i).rev() {
            b[j] = b[j].wrapping_add(b[j - 1]);
        }
    }
    b[m as usize]
}

/// Upstream `_dice_prob_exact`.
fn dice_prob_exact(dice: i64, sides: i64, count: i64) -> f64 {
    if dice < 0 || count < 0 || sides < 1 {
        return 0.0;
    }
    if dice == 0 {
        return if count == 0 { 1.0 } else { 0.0 };
    }
    if count > dice {
        return 0.0;
    }
    let p1kd = libm::pow(sides as f64, count as f64);
    let p2nkn = libm::pow((sides - 1) as f64, (dice - count) as f64);
    let p2nkd = libm::pow(sides as f64, (dice - count) as f64);
    let mut result = dice_binom(dice as u64, count as u64) as f64;
    result *= 1.0 / p1kd;
    result *= p2nkn / p2nkd;
    result
}

/// Upstream `_dice_prob_atleast`.
fn dice_prob_atleast(dice: i64, sides: i64, count: i64) -> f64 {
    if dice < 0 || count < 0 || sides < 1 {
        return 0.0;
    }
    let mut result = 0.0;
    let mut i = 0;
    while count + i <= dice {
        result += dice_prob_exact(dice, sides, count + i);
        i += 1;
    }
    result
}

/// Upstream `_dice_zero_tolerance`.
fn dice_zero_tolerance(x: f64) -> f64 {
    if libm::fabs(x) <= 0.00001 { 0.0 } else { x }
}

/// Upstream's `wme_val_stats`.
struct SetStats {
    count: i64,
    sum: f64,
    product: f64,
    min: f64,
    max: f64,
}

impl<S: crate::pool::Layout> SoarAgent<S> {
    /// An identifier's slot WMEs for one attribute, newest first, as upstream's slot lists hold
    /// them. Input and impasse WMEs are not in slots.
    fn slot_wme_values(&self, id: SymbolId, attr: SymbolId) -> Vec<SymbolId> {
        let mut values: Vec<SymbolId> =
            self.decider.slot_wmes.iter().rev().filter(|s| s.0 == id && s.1 == attr).map(|s| s.2).collect();
        if attr == self.symbols.predefined.operator {
            values.extend(self.decider.goals.iter().filter(|g| g.id == id).filter_map(|g| g.operator.map(|(op, _)| op)));
        }
        values
    }

    /// All of an identifier's slot WMEs, `(attr, value)`.
    fn all_slot_wme_values(&self, id: SymbolId) -> Vec<(SymbolId, SymbolId)> {
        let mut values: Vec<(SymbolId, SymbolId)> =
            self.decider.slot_wmes.iter().rev().filter(|s| s.0 == id).map(|s| (s.1, s.2)).collect();
        let operator = self.symbols.predefined.operator;
        values.extend(self.decider.goals.iter().filter(|g| g.id == id).filter_map(|g| g.operator.map(|(op, _)| (operator, op))));
        values
    }

    /// Upstream `set_reduce`: the values under `(<set> ^attr [^level-2 [^level-3]])`. An error
    /// is a string symbol, as upstream returns.
    fn set_values(&mut self, args: &[SymbolId]) -> Result<Vec<SymbolId>, SymbolId> {
        let Some(&set) = args.first().filter(|s| self.symbols.is_identifier(**s)) else {
            return Err(self.symbols.intern_str("|ERROR: First parameter needs to be an identifier.|"));
        };
        let Some(&attr) = args.get(1) else { return Ok(Vec::new()) };
        let mut values = self.slot_wme_values(set, attr);
        for &level in args.iter().skip(2).take(2) {
            values = values.into_iter().flat_map(|v| self.slot_wme_values(v, level)).collect();
        }
        Ok(values)
    }

    fn set_stats(&mut self, args: &[SymbolId]) -> Result<SetStats, SymbolId> {
        let mut stats = SetStats { count: 0, sum: 0.0, product: 1.0, min: f64::MAX, max: -f64::MAX };
        for value in self.set_values(args)? {
            if let Some(n) = number(&self.symbols, value) {
                let x = to_f64(n);
                stats.count += 1;
                stats.sum += x;
                stats.product *= x;
                if x < stats.min {
                    stats.min = x;
                }
                if x > stats.max {
                    stats.max = x;
                }
            }
        }
        Ok(stats)
    }

    /// Upstream's `trace_settings[n]`, as the `trace` level sets them.
    fn trace_setting(&self, n: i64) -> bool {
        match n {
            1 => self.trace_decisions,
            2 => self.trace_level >= 2,
            3..=5 => self.trace_level >= 3,
            _ => false,
        }
    }

    fn error(&mut self, message: &str) -> Option<SymbolId> {
        self.output.push_str(message);
        None
    }
}

/// Calls a built-in function; `None` means the call failed and the action is skipped, as an
/// upstream function returning NIL.
pub(super) fn call<S: crate::pool::Layout>(agent: &mut SoarAgent<S>, name: &str, args: &[SymbolId]) -> Option<SymbolId> {
    let nums: Option<Vec<Number>> = args.iter().map(|a| number(&agent.symbols, *a)).collect();
    // Upstream's "non-number" check for functions taking any number of numbers.
    let first_non_number = || args.iter().copied().find(|a| number(&agent.symbols, *a).is_none());
    match name {
        "+" | "*" | "-" | "/" | "min" | "max" => {
            if args.is_empty() && matches!(name, "-" | "/") {
                return agent.error(&format!("Error: '{}' function called with no arguments\n", name));
            }
            if let Some(bad) = first_non_number() {
                let message = format!("Error: non-number ({}) passed to {} function\n", shown(&agent.symbols, bad), name);
                return agent.error(&message);
            }
            let n = nums?;
            let symbols = &mut agent.symbols;
            match name {
                "+" => Some(make_number(symbols, fold(&n, i64::wrapping_add, |a, b| a + b, 0))),
                "*" => Some(make_number(symbols, fold(&n, i64::wrapping_mul, |a, b| a * b, 1))),
                "-" => {
                    let result = match n.as_slice() {
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
                        [] => return None,
                    };
                    Some(make_number(symbols, result))
                }
                "/" => {
                    // Upstream `fp_divide_rhs_function_code`: always a float.
                    let mut acc = if n.len() == 1 { 1.0 } else { to_f64(n[0]) };
                    for &d in if n.len() == 1 { &n[..] } else { &n[1..] } {
                        let d = to_f64(d);
                        if d == 0.0 {
                            return agent.error("Error: attempt to divide ('/') by zero.\n");
                        }
                        acc /= d;
                    }
                    Some(agent.symbols.intern_float(acc))
                }
                _ => {
                    // Upstream `min_rhs_function_code` / `max_rhs_function_code`.
                    let better = |a: f64, b: f64| if name == "min" { a < b } else { a > b };
                    let better_int = |a: i64, b: i64| if name == "min" { a < b } else { a > b };
                    let (mut best_i, mut best_f, mut float_found) = (0i64, 0.0f64, false);
                    for (index, value) in n.iter().enumerate() {
                        let first = index == 0;
                        match *value {
                            Number::Int(i) if float_found => {
                                if first || better(i as f64, best_f) {
                                    best_f = i as f64;
                                }
                            }
                            Number::Int(i) => {
                                if first || better_int(i, best_i) {
                                    best_i = i;
                                }
                            }
                            Number::Float(x) => {
                                if !float_found {
                                    float_found = true;
                                    best_f = best_i as f64;
                                }
                                if first || better(x, best_f) {
                                    best_f = x;
                                }
                            }
                        }
                    }
                    Some(if float_found { symbols.intern_float(best_f) } else { symbols.intern_int(best_i) })
                }
            }
        }
        "div" | "mod" => {
            for &arg in args {
                if !matches!(agent.symbols.resolve(arg), Some(SymbolData::Integer(_))) {
                    let message = format!("Error: non-integer ({}) passed to {} function\n", shown(&agent.symbols, arg), name);
                    return agent.error(&message);
                }
            }
            let (Some(Number::Int(a)), Some(Number::Int(b))) = (number(&agent.symbols, args[0]), number(&agent.symbols, args[1])) else {
                return None;
            };
            if b == 0 {
                return agent.error(&format!("Error: attempt to divide ('{}') by zero.\n", name));
            }
            let result = if name == "div" { a.wrapping_div(b) } else { a.wrapping_rem(b) };
            Some(agent.symbols.intern_int(result))
        }
        "size" | "sum" | "product" => {
            let id = args[0];
            if !agent.symbols.is_identifier(id) {
                let message = format!("Error: non-symbol ({}) passed to {} function\n", shown(&agent.symbols, id), name);
                return agent.error(&message);
            }
            let values = agent.all_slot_wme_values(id);
            // Upstream reads each value as an integer, in a 32-bit `int`.
            let as_int = |v: SymbolId| match agent.symbols.resolve(v) {
                Some(SymbolData::Integer(i)) => *i as i32,
                Some(SymbolData::Float(x)) => x.to_bits() as i64 as i32,
                _ => 0,
            };
            let result = match name {
                "size" => values.len() as i32,
                "sum" => values.iter().fold(0i32, |acc, (_, v)| acc.wrapping_add(as_int(*v))),
                _ => values.iter().fold(1i32, |acc, (_, v)| acc.wrapping_mul(as_int(*v))),
            };
            Some(agent.symbols.intern_int(i64::from(result)))
        }
        "set-count" => match agent.set_values(args) {
            Ok(values) => Some(agent.symbols.intern_int(values.len() as i64)),
            Err(error) => Some(error),
        },
        "set-sum" | "set-multiply" | "set-min" | "set-max" | "set-range" | "set-mean" | "set-stdev" => {
            let stats = match agent.set_stats(args) {
                Ok(stats) => stats,
                Err(error) => return Some(error),
            };
            if stats.count == 0 {
                return Some(agent.symbols.intern_str("NaN"));
            }
            let value = match name {
                "set-sum" => stats.sum,
                "set-multiply" => stats.product,
                "set-min" => stats.min,
                "set-max" => stats.max,
                "set-range" => stats.max - stats.min,
                "set-mean" => stats.sum / stats.count as f64,
                _ => {
                    let mean = stats.sum / stats.count as f64;
                    let mut sum_square = 0.0;
                    for v in agent.set_values(args).unwrap_or_default() {
                        if let Some(n) = number(&agent.symbols, v) {
                            let diff = to_f64(n) - mean;
                            sum_square += diff * diff;
                        }
                    }
                    libm::sqrt(sum_square / stats.count as f64)
                }
            };
            Some(agent.symbols.intern_float(value))
        }
        "sin" | "cos" | "sqrt" => match nums.as_deref() {
            Some([n]) => {
                let x = to_f64(*n);
                let result = match name {
                    "sin" => libm::sin(x),
                    "cos" => libm::cos(x),
                    _ => libm::sqrt(x),
                };
                Some(agent.symbols.intern_float(result))
            }
            _ => {
                let message = format!("Error: '{}' function called with non-numeric argument {}\n", name, shown(&agent.symbols, args[0]));
                agent.error(&message)
            }
        },
        "atan2" => {
            if let Some(bad) = first_non_number() {
                let message = format!("Error: non-number ({}) passed to atan2\n", shown(&agent.symbols, bad));
                return agent.error(&message);
            }
            let n = nums?;
            Some(agent.symbols.intern_float(libm::atan2(to_f64(n[0]), to_f64(n[1]))))
        }
        "abs" => match nums.as_deref() {
            Some([Number::Int(i)]) => Some(agent.symbols.intern_int(i.wrapping_abs())),
            Some([Number::Float(x)]) => Some(agent.symbols.intern_float(libm::fabs(*x))),
            _ => {
                let message = format!("Error: 'abs' function called with non-numeric argument {}\n", shown(&agent.symbols, args[0]));
                agent.error(&message)
            }
        },
        "int" => {
            let arg = args[0];
            let value = match agent.symbols.resolve(arg)? {
                SymbolData::Integer(i) => *i,
                SymbolData::Float(x) => libm::trunc(*x) as i64,
                SymbolData::String(s) => match strtoll(s) {
                    Some(i) => i,
                    None => {
                        let message = format!("Error: bad integer ({}) given to 'int' RHS function\n", shown(&agent.symbols, arg));
                        return agent.error(&message);
                    }
                },
                SymbolData::Identifier { .. } => {
                    let message = format!("Error: identifier ({}) passed to 'int' RHS function.\n", shown(&agent.symbols, arg));
                    return agent.error(&message);
                }
            };
            Some(agent.symbols.intern_int(value))
        }
        "float" => {
            let arg = args[0];
            let value = match agent.symbols.resolve(arg)? {
                SymbolData::Integer(i) => *i as f64,
                SymbolData::Float(x) => *x,
                SymbolData::String(s) => match strtod(s) {
                    Some(x) => x,
                    None => {
                        let message = format!("Error: bad float ({}) given to 'float' RHS function\n", shown(&agent.symbols, arg));
                        return agent.error(&message);
                    }
                },
                SymbolData::Identifier { .. } => {
                    let message = format!("Error: identifier ({}) passed to 'float' RHS function.\n", shown(&agent.symbols, arg));
                    return agent.error(&message);
                }
            };
            Some(agent.symbols.intern_float(value))
        }
        "round-off-heading" | "round-off" => {
            // Upstream reads a non-number as 0, and the type of the second argument decides.
            let n = number(&agent.symbols, args[0]).map_or(0.0, to_f64);
            let heading = name == "round-off-heading";
            match number(&agent.symbols, args[1]) {
                Some(Number::Float(m)) => {
                    let rounded = round_off_heading_float(n, m);
                    Some(agent.symbols.intern_float(if heading { normalize_heading_float(rounded) } else { rounded }))
                }
                other => {
                    let m = match other {
                        Some(Number::Int(m)) => m,
                        _ => 0,
                    };
                    if m == 0 {
                        // Upstream divides by zero here.
                        return agent.error(&format!("Error: '{}' function called with a zero multiple.\n", name));
                    }
                    let rounded = round_off_heading_int(n as i64, m);
                    Some(agent.symbols.intern_int(if heading { normalize_heading_int(rounded) } else { rounded }))
                }
            }
        }
        "compute-heading" | "compute-range" => {
            if let Some(bad) = first_non_number() {
                let message = format!("Error: non-number ({}) passed to - {}\n", shown(&agent.symbols, bad), name);
                return agent.error(&message);
            }
            let n = nums?;
            if name == "compute-heading" {
                let [x1, y1, x2, y2] = [to_i64(n[0]), to_i64(n[1]), to_i64(n[2]), to_i64(n[3])];
                Some(agent.symbols.intern_int(heading_to_point(x1, y1, x2, y2)))
            } else {
                let [x1, y1, x2, y2] = [to_f64(n[0]), to_f64(n[1]), to_f64(n[2]), to_f64(n[3])];
                let range = libm::sqrt((x1 - x2) * (x1 - x2) + (y1 - y2) * (y1 - y2));
                Some(agent.symbols.intern_int(range as i64))
            }
        }
        "extrapolate-x-position" | "extrapolate-y-position" => {
            for &arg in args {
                if !matches!(agent.symbols.resolve(arg), Some(SymbolData::Integer(_))) {
                    let message = format!("Error: non int({}) passed to {} function.\n", shown(&agent.symbols, arg), name);
                    return agent.error(&message);
                }
            }
            let n: Vec<i64> = nums?.into_iter().map(to_i64).collect();
            let heading_rad = (n[1] as f64) * PI / 180.0;
            let distance = (n[2].wrapping_mul(n[3])) as f64;
            let result = if name == "extrapolate-x-position" {
                n[0] + libm::round(distance * libm::sin(heading_rad)) as i32 as i64
            } else {
                // y is south-positive: upstream negates it on the way in and out.
                -((-n[0]) + libm::round(distance * libm::cos(heading_rad)) as i32 as i64)
            };
            Some(agent.symbols.intern_int(result))
        }
        "haversine" => {
            if let Some(bad) = first_non_number() {
                let message = format!("Error: non int/float ({}) passed to haversine function.\n", shown(&agent.symbols, bad));
                return agent.error(&message);
            }
            let n: Vec<f64> = nums?.into_iter().map(to_f64).collect();
            const R: f64 = 6371.0;
            let to_rad = PI / 180.0;
            let (lat_1, long_1, lat_2, long_2) = (n[0] * to_rad, n[1] * to_rad, n[2] * to_rad, n[3] * to_rad);
            let d_lat = lat_2 - lat_1;
            let d_lon = long_2 - long_1;
            let a = libm::sin(d_lat / 2.0) * libm::sin(d_lat / 2.0)
                + libm::cos(lat_1) * libm::cos(lat_2) * libm::sin(d_lon / 2.0) * libm::sin(d_lon / 2.0);
            let c = 2.0 * libm::atan2(libm::sqrt(a), libm::sqrt(1.0 - a));
            Some(agent.symbols.intern_float(R * c))
        }
        "select-point-closest-to-vector" => select_point_closest_to_vector(agent, args),
        "compute-dice-probability" => {
            let mut values = [0i64; 3];
            for (index, what) in ["dice", "sides", "count"].iter().enumerate() {
                match number(&agent.symbols, args[index]) {
                    Some(n) => values[index] = to_i64(n),
                    None => {
                        let message = format!("Error: non-number ({}) passed as '{}' to - compute-dice-probability\n", shown(&agent.symbols, args[index]), what);
                        return agent.error(&message);
                    }
                }
            }
            let [dice, sides, count] = values;
            let predicate = match agent.symbols.resolve(args[3]) {
                Some(SymbolData::String(s)) => s.clone(),
                _ => {
                    let message = format!("Error: non-string ({}) passed as 'pred' to - compute-dice-probability\n", shown(&agent.symbols, args[3]));
                    return agent.error(&message);
                }
            };
            let exact = || dice_prob_exact(dice, sides, count);
            let at_least = || dice_prob_atleast(dice, sides, count);
            let p = match predicate.as_str() {
                "eq" => if count < 0 || count > dice { 0.0 } else { dice_zero_tolerance(exact()) },
                "ne" => if count < 0 || count > dice { 1.0 } else { dice_zero_tolerance(1.0 - exact()) },
                "lt" => if count <= 0 { 0.0 } else if count > dice { 1.0 } else { dice_zero_tolerance(1.0 - at_least()) },
                "gt" => if count < 0 { 1.0 } else if count >= dice { 0.0 } else { dice_zero_tolerance(at_least() - exact()) },
                "le" => if count < 0 { 0.0 } else if count >= dice { 1.0 } else { dice_zero_tolerance((1.0 - at_least()) + exact()) },
                "ge" => if count <= 0 { 1.0 } else if count > dice { 0.0 } else { dice_zero_tolerance(at_least()) },
                _ => {
                    let message = format!("Error: invalid string ({}) passed as 'pred' to - compute-dice-probability\n", shown(&agent.symbols, args[3]));
                    return agent.error(&message);
                }
            };
            Some(agent.symbols.intern_float(p))
        }
        "rand-float" | "rand-int" => {
            let n = match args.first() {
                None => None,
                Some(&arg) => match number(&agent.symbols, arg) {
                    Some(n) => Some(n),
                    None => {
                        let message = format!("Error: non-number ({}) passed to - {}\n", shown(&agent.symbols, arg), name);
                        return agent.error(&message);
                    }
                },
            };
            let rng = &mut agent.exploration.rng;
            if name == "rand-float" {
                let max = n.map_or(0.0, to_f64);
                let x = if max > 0.0 { rng.rand_max(max) } else { rng.rand() };
                Some(agent.symbols.intern_float(x))
            } else {
                let max = n.map_or(0, to_i64);
                let i = if max > 0 { rng.rand_int_max(max as u32) } else { rng.rand_int() };
                Some(agent.symbols.intern_int(i64::from(i)))
            }
        }
        "crlf" => Some(agent.symbols.predefined.crlf),
        "dc" => Some(agent.symbols.intern_int(agent.decide_phases_count as i64)),
        // Upstream `get_lti_id_rhs_function_code`: the number of the long-term identifier the
        // argument is linked to; fails if it isn't linked.
        "@" => match agent.symbols.lti(args[0]) {
            0 => {
                let message = format!("Error: {} is not linked to a semantic identifier.\n", shown(&agent.symbols, args[0]));
                agent.error(&message)
            }
            lti => Some(agent.symbols.intern_int(lti as i64)),
        },
        // Upstream `set_lti_id_rhs_function_code`.
        "link-stm-to-ltm" => {
            let (id, lti) = (args[0], args[1]);
            if !agent.symbols.is_identifier(id) {
                let message = format!("Error: '@' rhs function cannot accept {} because it is not a Soar identifier\n", shown(&agent.symbols, id));
                return agent.error(&message);
            }
            let Some(SymbolData::Integer(n)) = agent.symbols.resolve(lti).cloned() else {
                let message = format!("Error: '@' rhs function cannot accept {} as an LTI ID because it is not an integer\n", shown(&agent.symbols, lti));
                return agent.error(&message);
            };
            if agent.smem.contains(crate::smem::LtiId(n as u64)) {
                agent.symbols.set_lti(id, n as u64);
            } else {
                let message = format!("Warning: Long-term memory @{} does not exist.  Could not link short-term memory {}.\n", n as u64, shown(&agent.symbols, id));
                agent.output.push_str(&message);
            }
            None
        }
        // Upstream `make_constant_symbol_rhs_function_code`: with no arguments, `constant` if no
        // such symbol exists yet; otherwise always a new symbol, the text followed by the
        // agent's counter (upstream `generate_new_str_constant` with `mcs_counter`).
        "make-constant-symbol" => {
            let symbols = &mut agent.symbols;
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
        "capitalize-symbol" | "trim" => {
            let Some(SymbolData::String(s)) = agent.symbols.resolve(args[0]).cloned() else {
                let message = if name == "trim" {
                    format!("Error: non-symbol ({}) passed to 'trim' function.\n", shown(&agent.symbols, args[0]))
                } else {
                    format!("Error: non-symbol ({}) passed to capitalize-symbol function.\n", shown(&agent.symbols, args[0]))
                };
                return agent.error(&message);
            };
            let result = if name == "trim" {
                String::from(s.trim_matches([' ', '\t', '\n']))
            } else {
                // C's `toupper` on the first byte.
                let mut bytes = s.into_bytes();
                if let Some(first) = bytes.first_mut() {
                    first.make_ascii_uppercase();
                }
                String::from_utf8(bytes).unwrap_or_default()
            };
            Some(agent.symbols.intern_str(&result))
        }
        // Upstream `string_rhs_function_code`: floats keep every digit (`max_digits10`).
        "string" => {
            let text = match agent.symbols.resolve(args[0]) {
                Some(SymbolData::Float(x)) => format!("{:.17}", x),
                _ => symbol_text(&agent.symbols, args[0]),
            };
            Some(agent.symbols.intern_str(&text))
        }
        "strlen" => {
            let length = symbol_text(&agent.symbols, args[0]).len();
            Some(agent.symbols.intern_int(length as i64))
        }
        // SML's `concat`.
        "concat" => {
            let text: String = args.iter().map(|a| symbol_text(&agent.symbols, *a)).collect();
            Some(agent.symbols.intern_str(&text))
        }
        "ifeq" => Some(if args[0] == args[1] { args[2] } else { args[3] }),
        "write" => {
            for arg in args {
                let text = symbol_text(&agent.symbols, *arg);
                agent.output.push_str(&text);
            }
            None
        }
        // Upstream `trace_rhs_function_code`: writes the rest if trace setting `n` is on.
        "trace" => {
            let level = match args.first().and_then(|a| agent.symbols.resolve(*a)) {
                Some(SymbolData::Integer(n)) if (0..=5).contains(n) => *n,
                _ => {
                    let shown_arg = args.first().map(|a| shown(&agent.symbols, *a)).unwrap_or_default();
                    let message = format!("Error: First argument of agent's (trace) rhs-function must be an integer trace level between 0 and 5.  {} is invalid.\n", shown_arg);
                    return agent.error(&message);
                }
            };
            if agent.trace_setting(level) {
                for arg in &args[1..] {
                    let text = symbol_text(&agent.symbols, *arg);
                    agent.output.push_str(&text);
                }
            }
            None
        }
        // Upstream `log_rhs_function_code`: every channel is on, as upstream's start.
        "log" => {
            match args.first().and_then(|a| agent.symbols.resolve(*a)) {
                Some(SymbolData::Integer(n)) if (1..=100).contains(n) => {}
                _ => {
                    let shown_arg = args.first().map(|a| shown(&agent.symbols, *a)).unwrap_or_default();
                    let message = format!("Error: First argument of agent's (log) rhs-function must be an integer channel number between 1 and 100.  {} is invalid.\n", shown_arg);
                    return agent.error(&message);
                }
            }
            for arg in &args[1..] {
                let text = symbol_text(&agent.symbols, *arg);
                agent.output.push_str(&text);
            }
            None
        }
        "count" => {
            for arg in args {
                let text = symbol_text(&agent.symbols, *arg);
                *agent.dyn_counters.entry(text).or_insert(0) += 1;
            }
            None
        }
        // Upstream sleeps for the given milliseconds; a run here never waits on the clock.
        "wait" => None,
        "halt" => {
            agent.halted = true;
            None
        }
        "dont-learn" | "force-learn" => {
            match args.first() {
                Some(state) if agent.goal_level(*state).is_some() => agent.chunker.flag_state(*state, name == "force-learn"),
                _ => agent.output.push_str(&format!("Error: identifier passed to {} is not a state.\n", name)),
            }
            None
        }
        _ => None,
    }
}

/// Upstream `select_point_closest_to_vector_rhs_function_code`: of the points under the fourth
/// argument (`^<attr> <p>` with `(<p> ^x ^y ^id)`), the `^id` of the one whose bearing from
/// `(x, y)` is closest to the heading, the nearer on a tie. As upstream, the points are the
/// WMEs of the identifier's first slot.
fn select_point_closest_to_vector<S: crate::pool::Layout>(agent: &mut SoarAgent<S>, args: &[SymbolId]) -> Option<SymbolId> {
    for &arg in &args[..3] {
        if !matches!(agent.symbols.resolve(arg), Some(SymbolData::Integer(_))) {
            let message = format!("Error: non int ({}) passed to select-point-closest-to-vector function.\n", shown(&agent.symbols, arg));
            return agent.error(&message);
        }
    }
    let n: Vec<i64> = args[..3].iter().filter_map(|a| number(&agent.symbols, *a)).map(to_i64).collect();
    let (start_x, start_y, heading) = (n[0], -n[1], n[2]);
    let points_id = args[3];
    if !agent.symbols.is_identifier(points_id) {
        let message = format!("Error: non-symbol ({}) passed to select-point-closest-to-vector function for points\n", shown(&agent.symbols, points_id));
        return agent.error(&message);
    }
    // Upstream's first slot is the one made last; here, that of the newest slot WME.
    let first_attr = agent.decider.slot_wmes.iter().rev().find(|s| s.0 == points_id).map(|s| s.1);
    let point_ids = first_attr.map(|attr| agent.slot_wme_values(points_id, attr)).unwrap_or_default();
    let (x, y, id) = (agent.symbols.intern_str("x"), agent.symbols.intern_str("y"), agent.symbols.intern_str("id"));
    let mut points: Vec<(i64, i64, String)> = Vec::new();
    for point in point_ids {
        if !agent.symbols.is_identifier(point) {
            let message = format!("Error: non-symbol ({}) passed to select-point-closest-to-vector function for a point\n", shown(&agent.symbols, point));
            return agent.error(&message);
        }
        let value = |attr| agent.slot_wme_values(point, attr).first().copied();
        let (Some(px), Some(py), Some(pid)) = (value(x), value(y), value(id)) else { return None };
        let px = number(&agent.symbols, px).map_or(0, to_i64);
        let py = -number(&agent.symbols, py).map_or(0, to_i64);
        points.push((px, py, symbol_text(&agent.symbols, pid)));
    }
    let angle_to = |px: i64, py: i64| {
        let mut degrees = 90.0 - libm::atan2((py - start_y) as f64, (px - start_x) as f64) * 180.0 / PI;
        if degrees < 0.0 {
            degrees += 360.0;
        }
        degrees
    };
    let distance = |px: i64, py: i64| {
        let (dx, dy) = ((px - start_x) as i32, (py - start_y) as i32);
        libm::sqrt(f64::from(dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))))
    };
    let mut min_diff = f64::MAX;
    let mut nearest = (1000i64, 1000i64, String::from("no_valid_point"));
    for (px, py, pid) in points {
        let diff = libm::fabs(heading as f64 - angle_to(px, py));
        let diff = if diff > 180.0 { 360.0 - diff } else { diff };
        if diff < min_diff {
            min_diff = diff;
            nearest = (px, py, pid.clone());
        }
        if diff == min_diff && distance(px, py) < distance(nearest.0, nearest.1) {
            nearest = (px, py, pid);
        }
    }
    Some(agent.symbols.intern_str(&nearest.2))
}
