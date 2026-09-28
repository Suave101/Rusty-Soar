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
        let mut saw_rule_header = false;

        // 1. Strip UTF-8 BOM if present
        let expanded_content = Self::expand_generated_rules(script_content)?;
        let content = expanded_content.trim_start_matches('\u{feff}');

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
                    saw_rule_header = true;
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

                    if !brace_found {
                        return Err(SoarParseError::InvalidSyntax(
                            "Rule header missing opening '{'".to_string(),
                        ));
                    }

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

                    if depth != 0 {
                        return Err(SoarParseError::InvalidSyntax(
                            "Rule body missing closing '}'".to_string(),
                        ));
                    }

                    let rule_body: String = chars[body_start..k].iter().collect();
                    let rule_name_override = header_text.trim();
                    productions.push(Self::parse_production_body(&rule_body, rule_name_override)?);

                    i = k + 1;
                    continue;
                }
            }
            i += 1;
        }

        if saw_rule_header && productions.is_empty() {
            return Err(SoarParseError::InvalidSyntax(
                "No valid productions found".to_string(),
            ));
        }

        Ok(SoarScript { productions })
    }

    fn expand_generated_rules(script_content: &str) -> Result<String, SoarParseError> {
        let mut content = script_content.to_string();
        let mut search_from = 0;

        while let Some(proc_offset) = content[search_from..].find("proc ") {
            let proc_start = search_from + proc_offset;
            let header_start = proc_start + "proc ".len();
            let args_start = content[header_start..]
                .find('{')
                .map(|offset| header_start + offset)
                .ok_or_else(|| {
                    SoarParseError::InvalidSyntax("Procedure missing arguments".into())
                })?;
            let args_end = Self::matching_brace(&content, args_start)?;
            let body_start = content[args_end + 1..]
                .find('{')
                .map(|offset| args_end + 1 + offset)
                .ok_or_else(|| SoarParseError::InvalidSyntax("Procedure missing body".into()))?;
            let body_end = Self::matching_brace(&content, body_start)?;

            let name = content[header_start..args_start].trim().to_string();
            let parameter = content[args_start + 1..args_end].trim().to_string();
            let body = content[body_start + 1..body_end].to_string();
            let (loop_body, loop_variable) = if let Some(foreach_offset) = body.find("foreach ") {
                let foreach_start = foreach_offset + "foreach ".len();
                let loop_args_end = body[foreach_start..]
                    .find('{')
                    .map(|offset| foreach_start + offset)
                    .ok_or_else(|| SoarParseError::InvalidSyntax("foreach missing body".into()))?;
                let loop_body_end = Self::matching_brace(&body, loop_args_end)?;
                let loop_variable = body[foreach_start..loop_args_end]
                    .split_whitespace()
                    .next()
                    .ok_or_else(|| {
                        SoarParseError::InvalidSyntax("foreach missing variable".into())
                    })?
                    .to_string();
                (
                    body[loop_args_end + 1..loop_body_end].to_string(),
                    loop_variable,
                )
            } else {
                (body.clone(), parameter.clone())
            };

            content.replace_range(proc_start..=body_end, "");
            let invocation = format!("{} {{", name);
            let mut invocation_search = 0;
            while let Some(invocation_offset) = content[invocation_search..].find(&invocation) {
                let invocation_start = invocation_search + invocation_offset;
                let list_start = invocation_start + name.len() + 1;
                let list_end = Self::matching_brace(&content, list_start)?;
                let values = content[list_start + 1..list_end]
                    .split_whitespace()
                    .collect::<Vec<&str>>();
                let mut replacement = String::new();
                for value in values {
                    let generated = loop_body
                        .replace(&format!("${{{}}}", loop_variable), value)
                        .replace(&format!("${}", loop_variable), value);
                    replacement.push_str(&generated);
                    replacement.push('\n');
                }
                content.replace_range(invocation_start..=list_end, &replacement);
                invocation_search = invocation_start + replacement.len();
            }

            search_from = proc_start;
        }

        let mut normalized = String::new();
        let chars: Vec<char> = content.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            if index + 3 < chars.len() && chars[index..].starts_with(&['s', 'p', ' ', '"']) {
                normalized.push_str("sp {");
                index += 4;
                while index < chars.len() && chars[index] != '"' {
                    normalized.push(chars[index]);
                    index += 1;
                }
                normalized.push('}');
                if index < chars.len() {
                    index += 1;
                }
            } else {
                normalized.push(chars[index]);
                index += 1;
            }
        }

        Ok(normalized)
    }

    fn matching_brace(content: &str, open: usize) -> Result<usize, SoarParseError> {
        let bytes = content.as_bytes();
        let mut depth = 0;
        for (index, byte) in bytes.iter().enumerate().skip(open) {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(index);
                    }
                }
                _ => {}
            }
        }
        Err(SoarParseError::InvalidSyntax("Unclosed brace".into()))
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

        let name = name.trim_matches('"').trim_matches('\'').to_string();

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
                    let val_str = val_str.trim_end_matches(['u', 'U']);
                    let val = match val_str {
                        "true" => SoarValue::Bool(true),
                        "false" => SoarValue::Bool(false),
                        _ => match val_str.parse::<i64>() {
                            Ok(i) => SoarValue::Int(i),
                            Err(_) => match val_str.parse::<f32>() {
                                Ok(f) => SoarValue::Float(f),
                                Err(_) => SoarValue::Symbol(val_str.to_string()),
                            },
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
                        op_id = token
                            .trim_end_matches(')')
                            .trim_end_matches(['u', 'U'])
                            .parse()
                            .map_err(|_| {
                                SoarParseError::InvalidSyntax(format!(
                                    "Invalid operator ID: {}",
                                    token
                                ))
                            })?;
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
                        target_alt = token
                            .trim_end_matches(')')
                            .trim_end_matches(['f', 'F'])
                            .parse()
                            .map_err(|_| {
                                SoarParseError::InvalidSyntax(format!(
                                    "Invalid target altitude: {}",
                                    token
                                ))
                            })?;
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
