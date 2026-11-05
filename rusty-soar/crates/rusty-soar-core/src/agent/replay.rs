//! Recorded input: replays what an environment put on the input link, one decision at a time.
//!
//! A recording is text, one decision per block:
//!
//! ```text
//! # comments start with #
//! decision 1
//! flightdata.airspeed 0
//! flightdata.lat 28.06
//! flightdata.failures.engine-0 |ok|
//! command rl --set learning on
//! decision 2
//! flightdata.airspeed 3
//! ```
//!
//! A path names attributes from the input link down; the identifiers along it are made the first
//! time a path needs them. Values are written as in a rule: `3` is an integer, `3.0` a float and
//! `|ok|` (or `ok`) a string. A `command` line runs a script command, such as the settings an
//! environment changes between decisions. The lines of a block run in order, before the decision.
//!
//! Each path holds one value, as with JSoar's `InputBuilder`. Replaying a value works like
//! JSoar's `InputWme.update`: the same value changes nothing, and a different one removes the
//! WME and adds a new one. So a recording can list every value each decision, as a recorder
//! that copies the whole input link does, or only those that changed.

use super::SoarAgent;
use crate::pool::Layout;
use crate::soar_parser::SoarScript;
use crate::symbol::SymbolId;
use crate::wm::WmeKey;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A recorded value.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedValue {
    Int(i64),
    Float(f64),
    Str(String),
}

/// One line of a decision's input.
#[derive(Debug, Clone, PartialEq)]
pub enum InputChange {
    /// Sets the value at a path below the input link.
    Set { path: Vec<String>, value: RecordedValue },
    /// Runs a script command.
    Command(String),
}

/// A recording: the input for each decision in turn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputRecording {
    pub decisions: Vec<Vec<InputChange>>,
}

impl InputRecording {
    /// Parses a recording. Errors give the line number.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut recording = InputRecording::default();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let at = |why: &str| format!("line {}: {}: {}", index + 1, why, line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(w, r)| (w, r.trim()));
            if word == "decision" {
                recording.decisions.push(Vec::new());
                continue;
            }
            let Some(decision) = recording.decisions.last_mut() else { return Err(at("input before the first `decision` line")) };
            if word == "command" {
                if rest.is_empty() {
                    return Err(at("command missing"));
                }
                decision.push(InputChange::Command(rest.to_string()));
                continue;
            }
            let path: Vec<String> = word.split('.').map(String::from).collect();
            if path.iter().any(String::is_empty) {
                return Err(at("empty attribute in path"));
            }
            let value = parse_value(rest).ok_or_else(|| at("value missing or unclosed"))?;
            decision.push(InputChange::Set { path, value });
        }
        Ok(recording)
    }
}

/// A value as written in a rule: integer, float, `|string|` or a bare string.
fn parse_value(text: &str) -> Option<RecordedValue> {
    if let Some(inner) = text.strip_prefix('|') {
        let mut value = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '|' => return chars.as_str().trim().is_empty().then_some(RecordedValue::Str(value)),
                '\\' => value.push(chars.next()?),
                _ => value.push(c),
            }
        }
        return None;
    }
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    if let Ok(i) = text.parse::<i64>() {
        return Some(RecordedValue::Int(i));
    }
    // Java writes non-finite doubles as `NaN` and `Infinity`.
    let numeric = text.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') || matches!(text, "NaN" | "Infinity");
    match text.parse::<f64>() {
        Ok(x) if numeric => Some(RecordedValue::Float(x)),
        _ => Some(RecordedValue::Str(text.to_string())),
    }
}

/// Replays a recording into an agent, one decision at a time.
#[derive(Debug, Default)]
pub struct InputReplay {
    recording: InputRecording,
    next: usize,
    /// The identifier each path names, and the WME linking it.
    ids: BTreeMap<Vec<String>, SymbolId>,
    /// The WME and value at each path.
    values: BTreeMap<Vec<String>, (WmeKey, SymbolId)>,
}

impl InputReplay {
    pub fn new(recording: InputRecording) -> Self {
        Self { recording, ..Self::default() }
    }

    /// How many decisions the recording has.
    pub fn len(&self) -> usize {
        self.recording.decisions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recording.decisions.is_empty()
    }

    /// Makes the next decision's input changes; call it before running that decision. Returns
    /// `Ok(false)` once the recording is used up, leaving the input as it is.
    pub fn apply_next<S: Layout>(&mut self, agent: &mut SoarAgent<S>) -> Result<bool, String> {
        let Some(changes) = self.recording.decisions.get(self.next).cloned() else { return Ok(false) };
        self.next += 1;
        for change in changes {
            match change {
                InputChange::Command(text) => {
                    let script = SoarScript::parse(&text).map_err(|e| format!("decision {}: {}: {}", self.next, text, e))?;
                    for (_, command) in &script.commands {
                        agent.run_script_command(command);
                    }
                }
                InputChange::Set { path, value } => self.set(agent, path, value)?,
            }
        }
        Ok(true)
    }

    fn set<S: Layout>(&mut self, agent: &mut SoarAgent<S>, path: Vec<String>, value: RecordedValue) -> Result<(), String> {
        let (attr, parents) = path.split_last().ok_or("empty path")?;
        if self.ids.contains_key(&path) {
            return Err(format!("{} holds attributes, so it can't have a value", path.join(".")));
        }
        let id = self.identifier(agent, parents)?;
        let value = match value {
            RecordedValue::Int(i) => agent.symbols.intern_int(i),
            RecordedValue::Float(x) => agent.symbols.intern_float(x),
            RecordedValue::Str(s) => agent.symbols.intern_str(&s),
        };
        if let Some(&(key, old)) = self.values.get(&path) {
            if old == value {
                return Ok(());
            }
            agent.remove_input_wme(key);
        }
        let attr = agent.symbols.intern_str(attr);
        let key = agent.add_input_wme(id, attr, value);
        self.values.insert(path, (key, value));
        Ok(())
    }

    /// The identifier a path names, made with its link WME if it's new.
    fn identifier<S: Layout>(&mut self, agent: &mut SoarAgent<S>, path: &[String]) -> Result<SymbolId, String> {
        let Some((attr, parents)) = path.split_last() else {
            return agent.input_link().ok_or_else(|| String::from("the agent has no input link yet"));
        };
        if let Some(id) = self.ids.get(path) {
            return Ok(*id);
        }
        if self.values.contains_key(path) {
            return Err(format!("{} has a value, so it can't hold attributes", path.join(".")));
        }
        let parent = self.identifier(agent, parents)?;
        let letter = attr.chars().next().map_or('I', |c| c.to_ascii_uppercase());
        let id = agent.new_input_identifier(if letter.is_ascii_alphabetic() { letter } else { 'I' });
        let attr = agent.symbols.intern_str(attr);
        agent.add_input_wme(parent, attr, id);
        self.ids.insert(path.to_vec(), id);
        Ok(id)
    }
}
