//! AGREE Annex specification parser and dynamic AST contract evaluator.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

/// Value types supported in AGREE annex evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum AgreeVal {
    /// Boolean value (`true` or `false`).
    Bool(bool),
    /// Signed 64-bit integer value.
    Int(i64),
    /// Single-precision floating point value.
    Float(f32),
}

/// AST Binary Operators for AGREE expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    /// Equality operator (`=`).
    Eq,
    /// Inequality operator (`/=`).
    Neq,
    /// Greater-than operator (`>`).
    Gt,
    /// Greater-than or equal-to operator (`>=`).
    Gte,
    /// Less-than operator (`<`).
    Lt,
    /// Less-than or equal-to operator (`<=`).
    Lte,
    /// Logical AND operator (`and`).
    And,
    /// Logical OR operator (`or`).
    Or,
    /// Logical implication operator (`=>`).
    Implies,
}

/// AST Expression Tree for AGREE annex assertions.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Variable identifier reference.
    Var(String),
    /// Literal constant value.
    Lit(AgreeVal),
    /// Binary expression tree.
    Binary(Box<Expr>, BinOp, Box<Expr>),
}

/// Individual Assume or Guarantee specification rule.
#[derive(Debug, Clone)]
pub struct ContractRule {
    /// Identifier tag/label for the rule (e.g., `"A01_SENSOR"`).
    pub tag: String,
    /// Parsed expression tree for the rule condition.
    pub expr: Expr,
}

/// Parsed AGREE Annex Specification container.
#[derive(Debug, Default)]
pub struct AgreeAnnex {
    /// Set of parsed environment assumption rules.
    pub assumes: Vec<ContractRule>,
    /// Set of parsed safety guarantee rules.
    pub guarantees: Vec<ContractRule>,
}

/// Errors encountered during AGREE annex parsing or contract evaluation.
#[derive(Debug)]
pub enum AgreeError {
    /// Syntax error encountered while parsing AGREE syntax.
    ParseError(String),
    /// Evaluation error encountered during expression resolution (e.g., unbound variable).
    EvalError(String),
    /// Contract violation where a guarantee condition evaluated to false.
    GuaranteeViolation(String),
}

impl fmt::Display for AgreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgreeError::ParseError(msg) => write!(f, "AGREE Parse Error: {}", msg),
            AgreeError::EvalError(msg) => write!(f, "AGREE Eval Error: {}", msg),
            AgreeError::GuaranteeViolation(msg) => write!(f, "AGREE Guarantee Violation: {}", msg),
        }
    }
}

/// Symbol Environment trait for looking up variable values during AGREE evaluation.
pub trait SymbolEnvironment {
    /// Looks up a variable name and returns its current evaluated value if bound.
    fn lookup(&self, var_name: &str) -> Option<AgreeVal>;
}

impl AgreeAnnex {
    /// Extracts and parses the `annex agree {** ... **}` block from raw AADL file content.
    pub fn parse_aadl_file(aadl_source: &str) -> Result<Self, AgreeError> {
        let mut annex = AgreeAnnex::default();

        let start_tag = "annex agree {**";
        let end_tag = "**};";

        let start_idx = aadl_source
            .find(start_tag)
            .map(|i| i + start_tag.len())
            .ok_or_else(|| AgreeError::ParseError("No 'annex agree {**' block found in AADL".into()))?;

        let end_idx = aadl_source[start_idx..]
            .find(end_tag)
            .map(|i| start_idx + i)
            .ok_or_else(|| AgreeError::ParseError("Unterminated AGREE annex block".into()))?;

        let agree_content = &aadl_source[start_idx..end_idx];

        for line in agree_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("assume") {
                let rule = Self::parse_statement(trimmed, "assume")?;
                annex.assumes.push(rule);
            } else if trimmed.starts_with("guarantee") {
                let rule = Self::parse_statement(trimmed, "guarantee")?;
                annex.guarantees.push(rule);
            }
        }

        Ok(annex)
    }

    fn parse_statement(line: &str, keyword: &str) -> Result<ContractRule, AgreeError> {
        let rest = line.trim_start_matches(keyword).trim();
        let colon_idx = rest
            .find(':')
            .ok_or_else(|| AgreeError::ParseError(format!("Missing ':' in AGREE rule: {}", line)))?;

        let tag = rest[..colon_idx]
            .trim()
            .trim_matches('"')
            .to_string();

        let expr_str = rest[colon_idx + 1..]
            .trim()
            .trim_end_matches(';');

        let expr = Self::parse_expr(expr_str)?;
        Ok(ContractRule { tag, expr })
    }

    fn parse_expr(s: &str) -> Result<Expr, AgreeError> {
        let mut s = s.trim();

        // 1. Strip outer enclosing parentheses when they enclose the entire expression
        while s.starts_with('(') && s.ends_with(')') {
            let mut depth = 0;
            let mut covers_all = true;
            for (i, ch) in s.chars().enumerate() {
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 && i < s.len() - 1 {
                        covers_all = false;
                        break;
                    }
                }
            }
            if covers_all && depth == 0 {
                s = s[1..s.len() - 1].trim();
            } else {
                break;
            }
        }

        // 2. Locate top-level binary operators outside parentheses
        let find_top_level_op = |expr: &str, op: &str| -> Option<usize> {
            let mut depth = 0;
            let bytes = expr.as_bytes();
            let op_bytes = op.as_bytes();
            let len = expr.len();
            let op_len = op.len();

            for i in 0..len {
                let ch = bytes[i];
                if ch == b'(' {
                    depth += 1;
                } else if ch == b')' {
                    if depth > 0 {
                        depth -= 1;
                    }
                } else if depth == 0 && i + op_len <= len && &bytes[i..i + op_len] == op_bytes {
                    return Some(i);
                }
            }
            None
        };

        if let Some(pos) = find_top_level_op(s, "=>") {
            let left = Self::parse_expr(&s[..pos])?;
            let right = Self::parse_expr(&s[pos + 2..])?;
            return Ok(Expr::Binary(Box::new(left), BinOp::Implies, Box::new(right)));
        }

        if let Some(pos) = find_top_level_op(s, "=") {
            let op = if pos > 0 && s.as_bytes()[pos - 1] == b'/' {
                BinOp::Neq
            } else {
                BinOp::Eq
            };
            let left_str = if op == BinOp::Neq { &s[..pos - 1] } else { &s[..pos] };
            let left = Self::parse_expr(left_str)?;
            let right = Self::parse_expr(&s[pos + 1..])?;
            return Ok(Expr::Binary(Box::new(left), op, Box::new(right)));
        }

        if s == "true" {
            Ok(Expr::Lit(AgreeVal::Bool(true)))
        } else if s == "false" {
            Ok(Expr::Lit(AgreeVal::Bool(false)))
        } else if let Ok(i) = s.parse::<i64>() {
            Ok(Expr::Lit(AgreeVal::Int(i)))
        } else if let Ok(f) = s.parse::<f32>() {
            Ok(Expr::Lit(AgreeVal::Float(f)))
        } else {
            Ok(Expr::Var(s.to_string()))
        }
    }

    /// Evaluates an AST expression against a given runtime state environment.
    pub fn eval_expr(expr: &Expr, env: &impl SymbolEnvironment) -> Result<AgreeVal, AgreeError> {
        match expr {
            Expr::Lit(val) => Ok(val.clone()),
            Expr::Var(name) => env
                .lookup(name)
                .ok_or_else(|| AgreeError::EvalError(format!("Unbound variable: {}", name))),
            Expr::Binary(left, op, right) => {
                let l_val = Self::eval_expr(left, env)?;
                let r_val = Self::eval_expr(right, env)?;

                match op {
                    BinOp::Eq => Ok(AgreeVal::Bool(l_val == r_val)),
                    BinOp::Neq => Ok(AgreeVal::Bool(l_val != r_val)),
                    BinOp::Implies => {
                        let l_bool = match l_val {
                            AgreeVal::Bool(b) => b,
                            _ => return Err(AgreeError::EvalError("Implies requires bool lhs".into())),
                        };
                        if !l_bool {
                            Ok(AgreeVal::Bool(true))
                        } else {
                            Ok(r_val)
                        }
                    }
                    _ => Err(AgreeError::EvalError("Operator not implemented".into())),
                }
            }
        }
    }

    /// Evaluates all parsed assumptions for a given runtime state.
    pub fn verify_assumes(&self, env: &impl SymbolEnvironment) -> Result<(), AgreeError> {
        for assume in &self.assumes {
            let res = Self::eval_expr(&assume.expr, env)?;
            if res != AgreeVal::Bool(true) {
                return Err(AgreeError::EvalError(format!(
                    "Assumption '{}' not satisfied",
                    assume.tag
                )));
            }
        }
        Ok(())
    }

    /// Evaluates all parsed guarantees for a Soar execution step.
    pub fn verify_guarantees(&self, env: &impl SymbolEnvironment) -> Result<(), AgreeError> {
        for guarantee in &self.guarantees {
            let res = Self::eval_expr(&guarantee.expr, env)?;
            if res != AgreeVal::Bool(true) {
                return Err(AgreeError::GuaranteeViolation(format!(
                    "Guarantee '{}' violated!",
                    guarantee.tag
                )));
            }
        }
        Ok(())
    }
}