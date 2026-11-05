//! Chunking (explanation-based learning), ported from upstream `explanation_based_chunking/`:
//! settings and identity sets. The analysis itself is in `agent/chunk.rs`.
//!
//! Every variable of every rule firing has an *identity*. Backtracing joins the identities of a
//! condition and the action that made the WME it matched, so one set stands for a value that
//! flowed through the firings; the learned rule has one variable per set. A set is *literalized*
//! when a rule tested or made the value as a constant, and stays a constant in the learned rule.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Which states learn (upstream `chunk always|never|only|except`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LearnIn {
    /// Every state.
    Always,
    /// No state.
    #[default]
    Never,
    /// Only states flagged with `force-learn`.
    Only,
    /// Every state except those flagged with `dont-learn`.
    Except,
}

/// Chunking settings (upstream `ebc_param_container`), with upstream's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSettings {
    /// `learn`.
    pub learn: LearnIn,
    /// `bottom-only`: learn only in the lowest state that makes a result.
    pub bottom_only: bool,
    /// `add-osk`: learn from the preferences that selected an operator.
    pub add_osk: bool,
    /// Whether every string attribute is a singleton unless it has a pattern (upstream
    /// `automatically-create-singletons`).
    pub automatic_singletons: bool,
    /// `allow-local-negations`.
    pub allow_local_negations: bool,
    /// `allow-opaque`: learn from results that depend on memory retrievals.
    pub allow_opaque: bool,
    /// `max-chunks`: the most rules learned in a decision.
    pub max_chunks: u64,
    /// `max-dupes`: the most duplicates of a rule learned from one rule in a decision.
    pub max_dupes: u64,
}

impl Default for ChunkSettings {
    fn default() -> Self {
        Self { learn: LearnIn::Never, bottom_only: false, add_osk: true, automatic_singletons: true, allow_local_negations: true, allow_opaque: true, max_chunks: 50, max_dupes: 3 }
    }
}

/// A kind of element in a singleton pattern (upstream `singleton_element_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingletonElement {
    /// `any`
    Any,
    /// `constant`
    Constant,
    /// `identifier`: not a state or operator.
    Identifier,
    /// `operator`
    Operator,
    /// `state`
    State,
}

impl SingletonElement {
    /// Parses a pattern's element.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "any" => Ok(Self::Any),
            "constant" => Ok(Self::Constant),
            "identifier" => Ok(Self::Identifier),
            "operator" => Ok(Self::Operator),
            "state" => Ok(Self::State),
            _ => Err(alloc::format!("Invalid pattern type {} for a singleton element (expected any, constant, identifier, operator or state)", text)),
        }
    }
}

/// One `chunk` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkSetting {
    /// `chunk always|never|only|except`
    Learn(LearnIn),
    /// `bottom-only`
    BottomOnly(bool),
    /// `add-osk`
    AddOsk(bool),
    AutomaticSingletons(bool),
    /// `allow-local-negations`
    AllowLocalNegations(bool),
    /// `allow-opaque`
    AllowOpaque(bool),
    /// `max-chunks`
    MaxChunks(u64),
    /// `max-dupes`
    MaxDupes(u64),
}

impl ChunkSetting {
    /// Parses `chunk <name> <value>`.
    pub fn parse(name: &str, value: &str) -> Result<Self, String> {
        let on_off = || match value {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err(alloc::format!("Invalid value for {} (expected on or off)", name)),
        };
        let number = || value.parse::<u64>().map_err(|_| alloc::format!("Invalid value for {} (expected a whole number)", name));
        match name {
            "bottom-only" => on_off().map(Self::BottomOnly),
            "add-osk" => on_off().map(Self::AddOsk),
            "automatically-create-singletons" => on_off().map(Self::AutomaticSingletons),
            "allow-local-negations" => on_off().map(Self::AllowLocalNegations),
            "allow-opaque" => on_off().map(Self::AllowOpaque),
            "max-chunks" => number().map(Self::MaxChunks),
            "max-dupes" => number().map(Self::MaxDupes),
            _ => Err(alloc::format!("Unsupported chunk setting {}", name)),
        }
    }

    /// Applies the setting.
    pub fn apply(self, settings: &mut ChunkSettings) {
        match self {
            Self::Learn(x) => settings.learn = x,
            Self::BottomOnly(x) => settings.bottom_only = x,
            Self::AddOsk(x) => settings.add_osk = x,
            Self::AutomaticSingletons(x) => settings.automatic_singletons = x,
            Self::AllowLocalNegations(x) => settings.allow_local_negations = x,
            Self::AllowOpaque(x) => settings.allow_opaque = x,
            Self::MaxChunks(x) => settings.max_chunks = x,
            Self::MaxDupes(x) => settings.max_dupes = x,
        }
    }
}

/// Upstream's `Identity` objects as a union-find: identities of (firing, variable).
#[derive(Debug, Default)]
pub(crate) struct Identities {
    by_variable: BTreeMap<(u64, String), usize>,
    parent: Vec<usize>,
    literal: Vec<bool>,
}

impl Identities {
    /// The identity of a variable in a firing, new if it has none.
    pub(crate) fn of(&mut self, firing: u64, variable: &str) -> usize {
        if let Some(&identity) = self.by_variable.get(&(firing, variable.into())) {
            return self.root(identity);
        }
        let identity = self.parent.len();
        self.parent.push(identity);
        self.literal.push(false);
        self.by_variable.insert((firing, variable.into()), identity);
        identity
    }

    /// The set an identity is in.
    pub(crate) fn root(&self, mut identity: usize) -> usize {
        while self.parent[identity] != identity {
            identity = self.parent[identity];
        }
        identity
    }

    /// Upstream `join_identities`.
    pub(crate) fn join(&mut self, from: usize, to: usize) {
        let (from, to) = (self.root(from), self.root(to));
        if from != to {
            self.parent[from] = to;
            self.literal[to] |= self.literal[from];
        }
    }

    /// Upstream `Identity::literalize`.
    pub(crate) fn literalize(&mut self, identity: usize) {
        let root = self.root(identity);
        self.literal[root] = true;
    }

    /// Whether the set stays a constant in the learned rule.
    pub(crate) fn literalized(&self, identity: usize) -> bool {
        self.literal[self.root(identity)]
    }
}

/// Learning statistics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChunkStats {
    /// Rules learned.
    pub chunks: u64,
    /// Rules not learned because an equal one exists.
    pub duplicates: u64,
    /// Results that got no rule: only the justification for this firing supports them.
    pub justifications: u64,
}
