//! Parser and AST representations for native `.soar` production rules (`sp { ... }`).

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

        // 1. Strip UTF-8 BOM if present
        let content = script_content.trim_start_matches('\u{feff}');

        // 2. Strip inline comments ('#' or '//' to end of line)
        let mut clean_lines = Vec::new();
        for line in content.lines() {
            let mut l = line;
            if let Some(idx) = l.find('#') {
                l = &l[..idx];
            }
            if let Some(idx) = l.find("//") {
                l = &l[..idx];
            }
            clean_lines.push(l);
        }
        let clean_content = clean_lines.join("\n");

        // 3. Lexical scanner with flexible rule header tracking
        let chars: Vec<char> = clean_content.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            if (chars[i] == 's' || chars[i] == 'S')
                && i + 1 < len
                && (chars[i + 1] == 'p' || chars[i + 1] == 'P')
            {
                let valid_before = i == 0
                    || chars[i - 1].is_whitespace()
                    || chars[i - 1] == ';'
                    || chars[i - 1] == '{'
                    || chars[i - 1] == '}';

                let valid_after = i + 2 == len
                    || chars[i + 2].is_whitespace()
                    || chars[i + 2] == '{'
                    || chars[i + 2] == '"';

                if valid_before && valid_after {
                    let mut j = i + 2;
                    let mut header_text = String::new();
                    let mut brace_found = false;

                    while j < len {
                        if chars[j] == '{' {
                            brace_found = true;
                            break;
                        }
                        header_text.push(chars[j]);
                        j += 1;
                    }

                    if brace_found {
                        let body_start = j + 1;
                        let mut depth = 1;
                        let mut k = body_start;

                        while k < len && depth > 0 {
                            if chars[k] == '{' {
                                depth += 1;
                            } else if chars[k] == '}' {
                                depth -= 1;
                            }
                            if depth == 0 {
                                break;
                            }
                            k += 1;
                        }

                        if depth == 0 {
                            let rule_body: String = chars[body_start..k].iter().collect();
                            let rule_name_override = header_text.trim();

                            if let Ok(production) =
                                Self::parse_production_body(&rule_body, rule_name_override)
                            {
                                productions.push(production);
                            }

                            i = k + 1;
                            continue;
                        }
                    }
                }
            }
            i += 1;
        }

        Ok(SoarScript { productions })
    }

    fn parse_production_body(
        body: &str,
        header_name: &str,
    ) -> Result<SoarProduction, SoarParseError> {
        let parts: Vec<&str> = body.split("-->").collect();
        if parts.len() != 2 {
            return Err(SoarParseError::InvalidSyntax(format!(
                "Rule missing '-->' separator in body:\n{}",
                body
            )));
        }

        let lhs = parts[0].trim();
        let rhs = parts[1].trim();

        let name = if !header_name.is_empty() {
            header_name
                .split_whitespace()
                .next()
                .unwrap_or(header_name)
                .to_string()
        } else {
            lhs.split_whitespace()
                .next()
                .ok_or_else(|| SoarParseError::InvalidSyntax("Missing rule name".to_string()))?
                .to_string()
        };

        let name = name
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let mut conditions = Vec::new();

        for line in lhs.lines() {
            let line = line.trim();
            let chunks: Vec<&str> = line.split('^').collect();
            for chunk in chunks.iter().skip(1) {
                let clean_chunk = chunk.trim().trim_end_matches(')');
                let tokens: Vec<&str> = clean_chunk.split_whitespace().collect();
                if tokens.len() >= 2 {
                    let raw_attr = tokens[0];
                    let attr = raw_attr.rsplit('.').next().unwrap_or(raw_attr).to_string();
                    let val_str = tokens[1].trim_end_matches(')').trim_start_matches('=');
                    let val = match val_str.parse::<i64>() {
                        Ok(i) => SoarValue::Int(i),
                        Err(_) => match val_str.parse::<f32>() {
                            Ok(f) => SoarValue::Float(f),
                            Err(_) => SoarValue::Symbol(val_str.to_string()),
                        },
                    };
                    conditions.push(SoarCondition {
                        attribute: attr,
                        value: val,
                    });
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
                if let Some(val) = line.split("^name").nth(1) {
                    if let Some(token) = val.split_whitespace().next() {
                        op_name = token.trim_end_matches(')').to_string();
                    }
                }
            }
            if line.contains("^operator_id") || line.contains("^op_id") || line.contains("^id") {
                let key = if line.contains("^operator_id") {
                    "^operator_id"
                } else if line.contains("^op_id") {
                    "^op_id"
                } else {
                    "^id"
                };
                if let Some(val) = line.split(key).nth(1) {
                    if let Some(token) = val.split_whitespace().next() {
                        op_id = token.trim_end_matches(')').parse().unwrap_or(0);
                    }
                }
            }
            if line.contains("^target_altitude_ft")
                || line.contains("^target_altitude")
                || line.contains("^altitude")
            {
                let key = if line.contains("^target_altitude_ft") {
                    "^target_altitude_ft"
                } else if line.contains("^target_altitude") {
                    "^target_altitude"
                } else {
                    "^altitude"
                };
                if let Some(val) = line.split(key).nth(1) {
                    if let Some(token) = val.split_whitespace().next() {
                        target_alt = token.trim_end_matches(')').parse().unwrap_or(0.0);
                    }
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