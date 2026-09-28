//! Parser and AST representations for native `.soar` production rules (`sp { ... }`).

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

/// Values assigned or tested inside Soar production rules.
#[derive(Debug, Clone, PartialEq)]
pub enum SoarValue {
    /// String symbol or variable identifier.
    Symbol(String),
    /// Integer literal.
    Int(i64),
    /// Float literal.
    Float(f32),
    /// Boolean literal.
    Bool(bool),
}

/// Attribute-Value condition inside a Soar production LHS.
#[derive(Debug, Clone)]
pub struct SoarCondition {
    /// Attribute path (e.g., `"engine_status"`).
    pub attribute: String,
    /// Target value to match.
    pub value: SoarValue,
}

/// Proposed operator action inside a Soar production RHS.
#[derive(Debug, Clone)]
pub struct SoarAction {
    /// Name of the proposed operator.
    pub operator_name: String,
    /// Output operator ID.
    pub operator_id: u32,
    /// Target altitude action setting.
    pub target_altitude_ft: f32,
}

/// AST representation of a Soar Production (`sp { ... }`).
#[derive(Debug, Clone)]
pub struct SoarProduction {
    /// Name identifier of the production rule.
    pub name: String,
    /// Left-Hand Side (LHS) conditions.
    pub conditions: Vec<SoarCondition>,
    /// Right-Hand Side (RHS) proposed actions.
    pub actions: Vec<SoarAction>,
}

/// Errors encountered while parsing `.soar` scripts.
#[derive(Debug)]
pub enum SoarParseError {
    /// Syntax error message.
    InvalidSyntax(String),
}

impl fmt::Display for SoarParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SoarParseError::InvalidSyntax(msg) => write!(f, "Soar Script Parse Error: {}", msg),
        }
    }
}

/// Container for a set of parsed Soar production rules.
#[derive(Debug, Default)]
pub struct SoarScript {
    /// List of parsed rules in the script.
    pub productions: Vec<SoarProduction>,
}

impl SoarScript {
    /// Parses raw `.soar` script source text into a `SoarScript` AST.
    pub fn parse(script_content: &str) -> Result<Self, SoarParseError> {
        let mut productions = Vec::new();

        let mut pos = 0;
        let len = script_content.len();

        while pos < len {
            if let Some(sp_start) = script_content[pos..].find("sp {") {
                let start_idx = pos + sp_start + 4;
                if let Some(end_rel) = script_content[start_idx..].find('}') {
                    let end_idx = start_idx + end_rel;
                    let rule_body = &script_content[start_idx..end_idx];

                    let production = Self::parse_production_body(rule_body)?;
                    productions.push(production);

                    pos = end_idx + 1;
                } else {
                    return Err(SoarParseError::InvalidSyntax(
                        "Unterminated 'sp {' rule block".to_string(),
                    ));
                }
            } else {
                break;
            }
        }

        Ok(SoarScript { productions })
    }

    fn parse_production_body(body: &str) -> Result<SoarProduction, SoarParseError> {
        let parts: Vec<&str> = body.split("-->").collect();
        if parts.len() != 2 {
            return Err(SoarParseError::InvalidSyntax(
                "Rule missing '-->' separator between LHS and RHS".to_string(),
            ));
        }

        let lhs = parts[0].trim();
        let rhs = parts[1].trim();

        // First token in LHS is the rule name
        let name = lhs
            .split_whitespace()
            .next()
            .ok_or_else(|| SoarParseError::InvalidSyntax("Missing rule name".to_string()))?
            .to_string();

        let mut conditions = Vec::new();

        // Extract condition patterns: e.g., ^engine_status 2
        for line in lhs.lines() {
            let line = line.trim();
            if let Some(caret_pos) = line.find('^') {
                let rest = line[caret_pos + 1..].trim_end_matches(')');
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                if tokens.len() >= 2 {
                    let attr = tokens[0].to_string();
                    let val = match tokens[1].parse::<i64>() {
                        Ok(i) => SoarValue::Int(i),
                        Err(_) => match tokens[1].parse::<f32>() {
                            Ok(f) => SoarValue::Float(f),
                            Err(_) => SoarValue::Symbol(tokens[1].to_string()),
                        },
                    };
                    conditions.push(SoarCondition { attribute: attr, value: val });
                }
            }
        }

        let mut actions = Vec::new();
        let mut op_name = String::from("default");
        let mut op_id = 0u32;
        let mut target_alt = 0.0f32;

        for line in rhs.lines() {
            let line = line.trim();
            if line.contains("^name") {
                if let Some(val) = line.split_whitespace().nth(1) {
                    op_name = val.trim_end_matches(')').to_string();
                }
            } else if line.contains("^operator_id") {
                if let Some(val) = line.split_whitespace().nth(1) {
                    op_id = val.trim_end_matches(')').parse().unwrap_or(0);
                }
            } else if line.contains("^target_altitude_ft") {
                if let Some(val) = line.split_whitespace().nth(1) {
                    target_alt = val.trim_end_matches(')').parse().unwrap_or(0.0);
                }
            }
        }

        actions.push(SoarAction {
            operator_name: op_name,
            operator_id: op_id,
            target_altitude_ft: target_alt,
        });

        Ok(SoarProduction {
            name,
            conditions,
            actions,
        })
    }
}