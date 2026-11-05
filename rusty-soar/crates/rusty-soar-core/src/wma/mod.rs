//! Working-memory activation (upstream `working_memory_activation.cpp`): settings, reference
//! histories and the activation they give. The agent's side (which WMEs are referenced, and
//! forgetting) is in `agent/wma.rs`.
//!
//! A WME's activation is the base-level activation of its references: the log of the sum, over
//! its last [`DECAY_HISTORY`] referenced decisions, of the references in that decision times
//! the decisions since raised to the decay rate (negative), with Petrov's approximation for older
//! references when `petrov-approx` is on.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Upstream `WMA_DECAY_HISTORY`: how many referenced decisions each WME remembers.
pub const DECAY_HISTORY: usize = 10;
/// Upstream `WMA_REFERENCES_PER_DECISION`: references expected per decision, which sizes the
/// power and approximation caches.
pub const REFERENCES_PER_DECISION: u64 = 50;
/// Upstream `WMA_ACTIVATION_NONE`: the activation of a WME with no decay history.
pub const ACTIVATION_NONE: f64 = 1.0;
/// Upstream `WMA_TIME_SUM_NONE`: the time sum of a WME with no decay history. It is upstream's
/// approximation of e, not `core::f64::consts::E`.
#[allow(clippy::approx_constant)]
pub const TIME_SUM_NONE: f64 = 2.71828182845905;
/// Upstream `WMA_ACTIVATION_LOW`: the activation of a history that sums to nothing.
pub const ACTIVATION_LOW: f64 = -1000000000.0;
/// Upstream `WMA_FORGOTTEN_CYCLE`: the forget cycle of an element found below threshold.
pub const FORGOTTEN_CYCLE: u64 = 0;

/// `forgetting`: whether, and how, WMEs below the decay threshold are removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Forgetting {
    /// `off`
    #[default]
    Off,
    /// `naive`: every o-supported WME is checked every decision.
    Naive,
    /// `bsearch`: each WME is checked when a binary search says it will fall below threshold.
    Bsearch,
    /// `on` (upstream `approx`): as `bsearch`, with an approximate first estimate.
    Approx,
}

/// `forget-wme`: which WMEs forgetting removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForgetWme {
    /// `all`
    #[default]
    All,
    /// `lti`: only the WMEs of long-term identifiers, and only all of an identifier's at once.
    Lti,
}

/// Upstream `wma_param_container`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WmaParams {
    /// `activation` (default off).
    pub activation: bool,
    /// `decay-rate`, kept negated as upstream does (default -0.5).
    pub decay_rate: f64,
    /// `decay-thresh`, kept negated as upstream does (default -2.0).
    pub decay_thresh: f64,
    /// `petrov-approx` (default off).
    pub petrov_approx: bool,
    /// `forgetting` (default off).
    pub forgetting: Forgetting,
    /// `forget-wme` (default all).
    pub forget_wme: ForgetWme,
    /// `fake-forgetting` (default off): find WMEs to forget but leave them.
    pub fake_forgetting: bool,
    /// `timers` (off or one). Rusty-Soar keeps no timers; the setting is kept for `wma`.
    pub timers: bool,
    /// `max-pow-cache`, in MB (default 10).
    pub max_pow_cache: i64,
}

impl Default for WmaParams {
    fn default() -> Self {
        Self {
            activation: false,
            decay_rate: -0.5,
            decay_thresh: -2.0,
            petrov_approx: false,
            forgetting: Forgetting::Off,
            forget_wme: ForgetWme::All,
            fake_forgetting: false,
            timers: false,
            max_pow_cache: 10,
        }
    }
}

/// One `wma --set` setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WmaSetting {
    Activation(bool),
    /// The value as given (positive); it is stored negated.
    DecayRate(f64),
    /// The value as given (positive); it is stored negated.
    DecayThresh(f64),
    PetrovApprox(bool),
    Forgetting(Forgetting),
    ForgetWme(ForgetWme),
    FakeForgetting(bool),
    Timers(bool),
    MaxPowCache(i64),
}

impl WmaSetting {
    /// Parses `wma --set <name> <value>` as upstream validates it.
    pub fn parse(name: &str, value: &str) -> Result<Self, String> {
        let switch = || match value {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err("Invalid value for activation setting.".to_string()),
        };
        let invalid = || "Invalid value for activation setting.".to_string();
        Ok(match name {
            "activation" => Self::Activation(switch()?),
            "decay-rate" => match value.parse::<f64>() {
                Ok(x) if (0.0..=1.0).contains(&x) => Self::DecayRate(x),
                _ => return Err(invalid()),
            },
            "decay-thresh" => match value.parse::<f64>() {
                Ok(x) if x > 0.0 => Self::DecayThresh(x),
                _ => return Err(invalid()),
            },
            "petrov-approx" => Self::PetrovApprox(switch()?),
            "forgetting" => Self::Forgetting(match value {
                "off" => Forgetting::Off,
                "naive" => Forgetting::Naive,
                "bsearch" => Forgetting::Bsearch,
                "on" => Forgetting::Approx,
                _ => return Err(invalid()),
            }),
            "forget-wme" => Self::ForgetWme(match value {
                "all" => ForgetWme::All,
                "lti" => ForgetWme::Lti,
                _ => return Err(invalid()),
            }),
            "fake-forgetting" => Self::FakeForgetting(switch()?),
            "timers" => Self::Timers(match value {
                "off" => false,
                "one" => true,
                _ => return Err(invalid()),
            }),
            "max-pow-cache" => match value.parse::<i64>() {
                Ok(x) if x > 0 => Self::MaxPowCache(x),
                _ => return Err(invalid()),
            },
            _ => return Err("Invalid activation setting.".to_string()),
        })
    }

    /// Every setting but `activation` is protected while activation is on.
    pub fn protected(&self) -> bool {
        !matches!(self, Self::Activation(_))
    }
}

/// `wma` with an option other than `--set`.
#[derive(Debug, Clone, PartialEq)]
pub enum WmaQuery {
    /// `wma`: every setting.
    Settings,
    /// `wma --get <name>`
    Get(String),
    /// `wma --history <timetag>`
    History(u64),
    /// `wma --stats [<name>]`
    Stats(Option<String>),
}

impl WmaParams {
    /// Applies a setting (the caller checks protection). The decay settings are negated.
    pub fn apply(&mut self, setting: WmaSetting) {
        match setting {
            WmaSetting::Activation(x) => self.activation = x,
            WmaSetting::DecayRate(x) => self.decay_rate = -x,
            WmaSetting::DecayThresh(x) => self.decay_thresh = -x,
            WmaSetting::PetrovApprox(x) => self.petrov_approx = x,
            WmaSetting::Forgetting(x) => self.forgetting = x,
            WmaSetting::ForgetWme(x) => self.forget_wme = x,
            WmaSetting::FakeForgetting(x) => self.fake_forgetting = x,
            WmaSetting::Timers(x) => self.timers = x,
            WmaSetting::MaxPowCache(x) => self.max_pow_cache = x,
        }
    }

    /// A setting's value as upstream prints it.
    pub fn get(&self, name: &str) -> Option<String> {
        let switch = |x: bool| if x { "on" } else { "off" }.to_string();
        Some(match name {
            "activation" => switch(self.activation),
            "decay-rate" => format_decimal(self.decay_rate),
            "decay-thresh" => format_decimal(self.decay_thresh),
            "petrov-approx" => switch(self.petrov_approx),
            "forgetting" => match self.forgetting {
                Forgetting::Off => "off",
                Forgetting::Naive => "naive",
                Forgetting::Bsearch => "bsearch",
                Forgetting::Approx => "on",
            }
            .to_string(),
            "forget-wme" => match self.forget_wme {
                ForgetWme::All => "all",
                ForgetWme::Lti => "lti",
            }
            .to_string(),
            "fake-forgetting" => switch(self.fake_forgetting),
            "timers" => if self.timers { "one" } else { "off" }.to_string(),
            "max-pow-cache" => self.max_pow_cache.to_string(),
            _ => return None,
        })
    }
}

/// A decimal as upstream's `to_string` (a stream with 6 significant digits) writes it.
fn format_decimal(x: f64) -> String {
    let text = alloc::format!("{}", x);
    if text.contains('e') || text.len() <= 8 {
        text
    } else {
        alloc::format!("{:.6}", x).trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Upstream `wma_cycle_reference`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CycleReference {
    pub num_references: u64,
    pub d_cycle: u64,
}

/// Upstream `wma_history`: a ring of the last [`DECAY_HISTORY`] referenced decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct History {
    pub access_history: [CycleReference; DECAY_HISTORY],
    pub next_p: usize,
    pub history_ct: usize,
    pub history_references: u64,
    pub total_references: u64,
    pub first_reference: u64,
}

/// Upstream `wma_history_next`.
pub fn history_next(current: usize) -> usize {
    if current == DECAY_HISTORY - 1 { 0 } else { current + 1 }
}

/// Upstream `wma_history_prev`.
pub fn history_prev(current: usize) -> usize {
    if current == 0 { DECAY_HISTORY - 1 } else { current - 1 }
}

/// The caches upstream `wma_init` computes from the settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Caches {
    /// `wma_power_array`: `i ^ decay_rate` for small `i`.
    pub power: Vec<f64>,
    /// `wma_thresh_exp`: `e ^ decay_thresh`.
    pub thresh_exp: f64,
    /// `wma_approx_array`, for `forgetting on`.
    pub approx: Vec<u64>,
}

impl Caches {
    /// Upstream `wma_init`.
    pub fn new(params: &WmaParams) -> Self {
        let decay_rate = params.decay_rate;
        let decay_thresh = params.decay_thresh;
        // The time a single decision's maximum references take to fall below threshold, bounded
        // by max-pow-cache (in MB of doubles).
        let cache_full = libm::exp((decay_thresh - libm::log(REFERENCES_PER_DECISION as f64)) / decay_rate);
        let cache_bound = ((params.max_pow_cache as u64 * 1024 * 1024) as u32 / core::mem::size_of::<f64>() as u32) as f64;
        let size = libm::ceil(if cache_full > cache_bound { cache_bound } else { cache_full }) as u32 as usize;
        let mut power = Vec::with_capacity(size);
        if size > 0 {
            power.push(0.0);
        }
        for i in 1..size {
            power.push(libm::pow(i as f64, decay_rate));
        }
        let approx = if params.forgetting == Forgetting::Approx {
            let mut approx = Vec::with_capacity(REFERENCES_PER_DECISION as usize);
            approx.push(0);
            for i in 1..REFERENCES_PER_DECISION {
                approx.push(libm::ceil(libm::exp((decay_thresh - libm::log(i as f64)) / decay_rate)) as u64);
            }
            approx
        } else {
            Vec::new()
        };
        Self { power, thresh_exp: libm::exp(decay_thresh), approx }
    }

    /// Upstream `wma_pow`.
    pub fn pow(&self, params: &WmaParams, cycle_diff: u64) -> f64 {
        if cycle_diff < self.power.len() as u64 {
            self.power[cycle_diff as usize]
        } else {
            libm::pow(cycle_diff as f64, params.decay_rate)
        }
    }

    /// Upstream `wma_sum_history`.
    pub fn sum_history(&self, params: &WmaParams, history: &History, current_cycle: u64) -> f64 {
        let mut sum = 0.0;
        let mut p = history.next_p;
        let mut cycle_diff = 0;
        for _ in 0..history.history_ct {
            p = history_prev(p);
            cycle_diff = current_cycle.wrapping_sub(history.access_history[p].d_cycle);
            sum += history.access_history[p].num_references as f64 * self.pow(params, cycle_diff);
        }
        // Petrov (2006): the references older than the history.
        if params.petrov_approx && history.total_references > history.history_references {
            let d_inv = 1.0 + params.decay_rate;
            let since_first = current_cycle.wrapping_sub(history.first_reference);
            sum += ((history.total_references - history.history_references) as f64
                * (libm::pow(since_first as f64, d_inv) - libm::pow(cycle_diff as f64, d_inv)))
                / (d_inv * since_first.wrapping_sub(cycle_diff) as f64);
        }
        sum
    }

    /// Upstream `wma_calculate_decay_activation`: the log of the history's sum when `log` is
    /// set, else the sum itself.
    pub fn decay_activation(&self, params: &WmaParams, history: &History, current_cycle: u64, log: bool) -> f64 {
        if history.history_ct == 0 {
            return if log { ACTIVATION_LOW } else { 0.0 };
        }
        let sum = self.sum_history(params, history, current_cycle);
        if !log {
            sum
        } else if sum > 0.0 {
            libm::log(sum)
        } else {
            ACTIVATION_LOW
        }
    }
}

/// Upstream `_wma_ref_to_str`.
pub fn reference_to_string(reference: &CycleReference, current_cycle: u64) -> String {
    alloc::format!(
        "{} @ d{} (-{})",
        reference.num_references,
        reference.d_cycle,
        current_cycle.wrapping_sub(reference.d_cycle)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_settings_are_stored_negated() {
        let mut params = WmaParams::default();
        params.apply(WmaSetting::parse("decay-rate", "0.8").unwrap());
        params.apply(WmaSetting::parse("decay-thresh", "3").unwrap());
        assert_eq!((params.decay_rate, params.decay_thresh), (-0.8, -3.0));
        assert!(WmaSetting::parse("decay-rate", "1.5").is_err());
        assert!(WmaSetting::parse("decay-thresh", "0").is_err());
        assert!(WmaSetting::parse("forgetting", "approx").is_err());
        assert_eq!(WmaSetting::parse("forgetting", "on"), Ok(WmaSetting::Forgetting(Forgetting::Approx)));
    }

    #[test]
    fn one_reference_decays_as_a_power_of_time() {
        let params = WmaParams::default();
        let caches = Caches::new(&params);
        let mut history = History::default();
        history.access_history[0] = CycleReference { num_references: 1, d_cycle: 1 };
        history.next_p = 1;
        history.history_ct = 1;
        history.history_references = 1;
        history.total_references = 1;
        history.first_reference = 1;
        // log(1 * 4^-0.5) = log(0.5)
        let activation = caches.decay_activation(&params, &history, 5, true);
        assert!((activation - libm::log(0.5)).abs() < 1e-12, "{activation}");
    }
}
