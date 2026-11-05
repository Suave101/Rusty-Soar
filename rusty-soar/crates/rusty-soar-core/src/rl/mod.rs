//! Reinforcement-learning settings (upstream `rl_param_container`). The algorithm itself is
//! in the agent (`agent/rl.rs`), since it works on the agent's goals and productions.

use alloc::vec::Vec;

/// Upstream `learning-policy`. Upstream's `on-policy-gq-lambda` and `off-policy-gq-lambda` are
/// not ported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearningPolicy {
    /// `sarsa` (upstream default): updates toward the value of the operator selected.
    Sarsa,
    /// `q-learning`: updates toward the best value among the candidates (off-policy).
    QLearning,
}

/// Upstream `decay-mode`: how the learning rate decays with a rule's update count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecayMode {
    /// `normal` (upstream default): the learning rate as set.
    Normal,
    /// `exp`: `1 / (updates + 1)`.
    Exponential,
    /// `log`: `1 / (ln(updates + 1) + 1)`.
    Logarithmic,
    /// `delta-bar-delta`: per-rule rates adapted by `meta-learning-rate`.
    DeltaBarDelta,
}

/// Reinforcement-learning settings, with upstream's defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RlParams {
    /// `learning` (default off).
    pub learning: bool,
    /// `discount-rate`, gamma (default 0.9).
    pub discount_rate: f64,
    /// `learning-rate`, alpha (default 0.3).
    pub learning_rate: f64,
    /// `step-size-parameter`, eta (default 1.0).
    pub step_size_parameter: f64,
    /// `meta-learning-rate`, theta, for delta-bar-delta decay (default 0.1).
    pub meta_learning_rate: f64,
    /// `learning-policy` (default sarsa).
    pub learning_policy: LearningPolicy,
    /// `decay-mode` (default normal).
    pub decay_mode: DecayMode,
    /// `eligibility-trace-decay-rate`, lambda (default 0).
    pub et_decay_rate: f64,
    /// `eligibility-trace-tolerance`: traces below it are dropped (default 0.001).
    pub et_tolerance: f64,
    /// `temporal-extension`: updates wait across decisions with no RL rule (default on).
    pub temporal_extension: bool,
    /// `hrl-discount`: discount rewards of goals above the bottom one by age (default off).
    pub hrl_discount: bool,
    /// `temporal-discount`: discount across gaps (default on).
    pub temporal_discount: bool,
}

impl Default for RlParams {
    fn default() -> Self {
        Self {
            learning: false,
            discount_rate: 0.9,
            learning_rate: 0.3,
            step_size_parameter: 1.0,
            meta_learning_rate: 0.1,
            learning_policy: LearningPolicy::Sarsa,
            decay_mode: DecayMode::Normal,
            et_decay_rate: 0.0,
            et_tolerance: 0.001,
            temporal_extension: true,
            hrl_discount: false,
            temporal_discount: true,
        }
    }
}

/// One `rl --set <name> <value>` setting, validated as upstream validates it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RlSetting {
    Learning(bool),
    DiscountRate(f64),
    LearningRate(f64),
    StepSizeParameter(f64),
    MetaLearningRate(f64),
    LearningPolicy(LearningPolicy),
    DecayMode(DecayMode),
    EtDecayRate(f64),
    EtTolerance(f64),
    TemporalExtension(bool),
    HrlDiscount(bool),
    TemporalDiscount(bool),
}

impl RlSetting {
    /// Parses a setting's name and value. Settings upstream has but this port does not
    /// (`meta`, `apoptosis`, `trace`, the GQ policies, ...) are errors, not ignored.
    pub fn parse(name: &str, value: &str) -> Result<Self, alloc::string::String> {
        use alloc::format;
        let switch = || match value {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err(format!("Invalid value for rl setting {}: {} (expected on or off)", name, value)),
        };
        let between_0_and_1 = || match value.parse::<f64>() {
            Ok(x) if (0.0..=1.0).contains(&x) => Ok(x),
            _ => Err(format!("Invalid value for rl setting {}: {} (expected a number from 0 to 1)", name, value)),
        };
        Ok(match name {
            "learning" => Self::Learning(switch()?),
            "discount-rate" => Self::DiscountRate(between_0_and_1()?),
            "learning-rate" => Self::LearningRate(between_0_and_1()?),
            "step-size-parameter" => Self::StepSizeParameter(between_0_and_1()?),
            "meta-learning-rate" => Self::MetaLearningRate(between_0_and_1()?),
            "eligibility-trace-decay-rate" => Self::EtDecayRate(between_0_and_1()?),
            "eligibility-trace-tolerance" => match value.parse::<f64>() {
                Ok(x) if x > 0.0 => Self::EtTolerance(x),
                _ => return Err(format!("Invalid value for rl setting {}: {} (expected a number above 0)", name, value)),
            },
            "learning-policy" => Self::LearningPolicy(match value {
                "sarsa" => LearningPolicy::Sarsa,
                "q-learning" => LearningPolicy::QLearning,
                "on-policy-gq-lambda" | "off-policy-gq-lambda" => {
                    return Err(format!("rl learning-policy {} is not supported", value));
                }
                _ => return Err(format!("Invalid value for rl setting learning-policy: {}", value)),
            }),
            "decay-mode" => Self::DecayMode(match value {
                "normal" => DecayMode::Normal,
                "exp" => DecayMode::Exponential,
                "log" => DecayMode::Logarithmic,
                "delta-bar-delta" => DecayMode::DeltaBarDelta,
                _ => return Err(format!("Invalid value for rl setting decay-mode: {}", value)),
            }),
            "temporal-extension" => Self::TemporalExtension(switch()?),
            "hrl-discount" => Self::HrlDiscount(switch()?),
            "temporal-discount" => Self::TemporalDiscount(switch()?),
            "chunk-stop" | "meta" | "update-log-path" | "apoptosis" | "apoptosis-decay" | "apoptosis-thresh" | "trace" => {
                return Err(format!("rl setting {} is not supported", name));
            }
            _ => return Err(format!("Invalid rl setting: {}", name)),
        })
    }
}

/// What upstream keeps on an RL rule's `production`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RlRule {
    pub(crate) name: &'static str,
    /// Expected current reward (`rl_ecr`).
    pub(crate) ecr: f64,
    /// Expected future reward (`rl_efr`); starts at the rule's numeric preference value.
    pub(crate) efr: f64,
    /// Upstream `rl_gql`, the gradient-Q weight (updated but only used by GQ policies).
    pub(crate) gql: f64,
    pub(crate) update_count: f64,
    pub(crate) delta_bar_delta_beta: f64,
    pub(crate) delta_bar_delta_h: f64,
    /// The rule's numeric preference value once learning has changed it (upstream replaces the
    /// action's referent); `None` while it is the value written in the rule.
    pub(crate) value: Option<crate::symbol::SymbolId>,
}

impl RlRule {
    pub(crate) fn new(name: &'static str, initial_value: f64) -> Self {
        Self {
            name,
            ecr: 0.0,
            efr: initial_value,
            gql: 0.0,
            update_count: 0.0,
            delta_bar_delta_beta: -3.0,
            delta_bar_delta_h: 0.0,
            value: None,
        }
    }
}

/// What upstream keeps on a goal's `rl_info`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RlGoal {
    /// Eligibility traces by RL rule index. Upstream keys a `std::map` by production pointer,
    /// so its iteration order follows memory addresses; this one follows rule creation order.
    pub(crate) traces: Vec<(usize, f64)>,
    /// RL rules that supported the previously selected operator (upstream `prev_op_rl_rules`).
    pub(crate) prev_op_rl_rules: Vec<usize>,
    pub(crate) previous_q: f64,
    pub(crate) reward: f64,
    pub(crate) rho: f64,
    pub(crate) gap_age: u32,
    pub(crate) hrl_age: u32,
    /// The goal's `^reward-link` identifier.
    pub(crate) reward_link: Option<crate::symbol::SymbolId>,
}

impl Default for RlGoal {
    fn default() -> Self {
        Self { traces: Default::default(), prev_op_rl_rules: Default::default(), previous_q: 0.0, reward: 0.0, rho: 1.0, gap_age: 0, hrl_age: 0, reward_link: None }
    }
}

/// An agent's reinforcement-learning settings and RL rules.
#[derive(Debug, Clone, Default)]
pub struct Rl {
    /// Settings (upstream `rl --set`).
    pub params: RlParams,
    /// RL rules in creation order.
    pub(crate) rules: alloc::vec::Vec<RlRule>,
}

impl Rl {
    /// The current value of an RL rule's numeric preference (upstream `rl_ecr + rl_efr`), or
    /// `None` if the production is not an RL rule.
    pub fn rule_value(&self, name: &str) -> Option<f64> {
        self.rules.iter().find(|r| r.name == name).map(|r| r.ecr + r.efr)
    }

    /// How many times learning has updated an RL rule.
    pub fn rule_update_count(&self, name: &str) -> Option<f64> {
        self.rules.iter().find(|r| r.name == name).map(|r| r.update_count)
    }
}
