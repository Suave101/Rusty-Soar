//! Semantic memory's long-term store, ported from upstream `semantic_memory/` with its SQLite
//! database replaced by in-memory tables (upstream's default database is in memory too).
//!
//! The tables keep upstream's numbering and order, which decide what the agent sees: long-term
//! identifiers are numbered from 1 in creation order, symbols get "hash" ids in the order they
//! are first stored (upstream's `smem_symbols_type` row ids), and an identifier's edges are
//! listed in the order of upstream's `smem_augmentations_parent_attr_val_lti` index, by
//! attribute, then constant value, then long-term-identifier value.
//!
//! The agent side (commands on the `^smem` link, recall into working memory) is in
//! `agent/smem.rs`.

use crate::symbol::SymbolId;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

/// A long-term identifier (upstream `LTI_ID`), numbered from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LtiId(pub u64);

/// Upstream `SMEM_ACT_LOW`: the activation of an edge whose identifier is not activated yet.
pub const ACT_LOW: f64 = -1_000_000_000.0;

/// Upstream `SMEM_ACT_MAX`: the activation of an edge whose identifier has at least
/// [`SmemSettings::thresh`] edges.
pub const ACT_MAX: f64 = (u64::MAX / 2) as f64;

/// How activation is computed (upstream `activation-mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivationMode {
    /// The time of the last access.
    #[default]
    Recency,
    /// The number of accesses.
    Frequency,
    /// Base-level activation: the log odds of the decayed sum of recent accesses.
    BaseLevel,
}

/// When base-level activations are recomputed (upstream `base-update-policy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BaseUpdate {
    /// Only when an identifier is stored or retrieved.
    #[default]
    Stable,
    /// Also every candidate of a query, before the query is answered.
    Naive,
    /// Also identifiers last accessed one of [`SmemSettings::base_incremental_threshes`]
    /// cycles before an access.
    Incremental,
}

/// Semantic memory settings (upstream `smem_param_container`), with upstream's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct SmemSettings {
    /// `learning`: semantic memory runs only when this is on.
    pub learning: bool,
    /// `activation-mode`.
    pub activation_mode: ActivationMode,
    /// `activate-on-query`.
    pub activate_on_query: bool,
    /// `activate-on-add`.
    pub activate_on_add: bool,
    /// `thresh`: identifiers with at least this many edges keep their edges at [`ACT_MAX`].
    pub thresh: u64,
    /// `base-decay`: the decay rate of base-level activation, above 0.
    pub base_decay: f64,
    /// `base-update-policy`.
    pub base_update: BaseUpdate,
    /// `base-incremental-threshes`: the ages at which the incremental policy recomputes.
    pub base_incremental_threshes: BTreeSet<i64>,
    /// `base-unused-age-offset`: added to the agent's age in the estimate for identifiers with
    /// no accesses.
    pub base_unused_age_offset: i64,
    /// `base-inhibition`: whether a recent access lowers base-level activation for a while.
    pub base_inhibition: bool,
}

impl Default for SmemSettings {
    fn default() -> Self {
        Self {
            learning: false,
            activation_mode: ActivationMode::Recency,
            activate_on_query: true,
            activate_on_add: false,
            thresh: 100,
            base_decay: 0.5,
            base_update: BaseUpdate::Stable,
            base_incremental_threshes: BTreeSet::from([10]),
            base_unused_age_offset: 0,
            base_inhibition: false,
        }
    }
}

/// One `smem --set` (or `--enable` / `--disable`) setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmemSetting {
    /// `learning`
    Learning(bool),
    /// `activation-mode`
    ActivationMode(ActivationMode),
    /// `activate-on-query`
    ActivateOnQuery(bool),
    /// `activate-on-add`
    ActivateOnAdd(bool),
    /// `thresh`
    Thresh(u64),
    /// `base-decay`
    BaseDecay(f64),
    /// `base-update-policy`
    BaseUpdate(BaseUpdate),
    /// `base-incremental-threshes`: adds the value to the set, or removes it if present, as
    /// upstream does.
    BaseIncrementalThresh(i64),
    /// `base-unused-age-offset`
    BaseUnusedAgeOffset(i64),
    /// `base-inhibition`
    BaseInhibition(bool),
}

impl SmemSetting {
    /// Parses `smem --set <name> <value>`.
    pub fn parse(name: &str, value: &str) -> Result<Self, alloc::string::String> {
        let on_off = || match value {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err(alloc::format!("Invalid value for {} (expected on or off)", name)),
        };
        match name {
            "learning" => on_off().map(Self::Learning),
            "activate-on-query" => on_off().map(Self::ActivateOnQuery),
            "activate-on-add" => on_off().map(Self::ActivateOnAdd),
            "activation-mode" => match value {
                "recency" => Ok(Self::ActivationMode(ActivationMode::Recency)),
                "frequency" => Ok(Self::ActivationMode(ActivationMode::Frequency)),
                "base-level" => Ok(Self::ActivationMode(ActivationMode::BaseLevel)),
                _ => Err("Invalid activation mode (expected recency, frequency or base-level)".into()),
            },
            "thresh" => value.parse().map(Self::Thresh).map_err(|_| "Invalid thresh (expected a whole number)".into()),
            "base-decay" => match value.parse::<f64>() {
                Ok(x) if x > 0.0 => Ok(Self::BaseDecay(x)),
                _ => Err("Invalid base-decay (expected a number above 0)".into()),
            },
            "base-update-policy" => match value {
                "stable" => Ok(Self::BaseUpdate(BaseUpdate::Stable)),
                "naive" => Ok(Self::BaseUpdate(BaseUpdate::Naive)),
                "incremental" => Ok(Self::BaseUpdate(BaseUpdate::Incremental)),
                _ => Err("Invalid base-update-policy (expected stable, naive or incremental)".into()),
            },
            "base-incremental-threshes" => {
                value.parse().map(Self::BaseIncrementalThresh).map_err(|_| "Invalid base-incremental-threshes (expected an integer)".into())
            }
            "base-unused-age-offset" => {
                value.parse().map(Self::BaseUnusedAgeOffset).map_err(|_| "Invalid base-unused-age-offset (expected an integer)".into())
            }
            "base-inhibition" => on_off().map(Self::BaseInhibition),
            _ => Err(alloc::format!("Unsupported smem setting {}", name)),
        }
    }

    /// Applies the setting.
    pub fn apply(self, settings: &mut SmemSettings) {
        match self {
            Self::Learning(x) => settings.learning = x,
            Self::ActivationMode(x) => settings.activation_mode = x,
            Self::ActivateOnQuery(x) => settings.activate_on_query = x,
            Self::ActivateOnAdd(x) => settings.activate_on_add = x,
            Self::Thresh(x) => settings.thresh = x,
            Self::BaseDecay(x) => settings.base_decay = x,
            Self::BaseUpdate(x) => settings.base_update = x,
            Self::BaseIncrementalThresh(x) => {
                if !settings.base_incremental_threshes.remove(&x) {
                    settings.base_incremental_threshes.insert(x);
                }
            }
            Self::BaseUnusedAgeOffset(x) => settings.base_unused_age_offset = x,
            Self::BaseInhibition(x) => settings.base_inhibition = x,
        }
    }
}

/// The value of a long-term identifier's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmemValue {
    /// A constant.
    Constant(SymbolId),
    /// Another long-term identifier.
    Lti(LtiId),
}

/// One edge (upstream `smem_augmentations` row). Values are hash ids, 0 for none, as upstream
/// stores them, so sorting the edges sorts them as upstream's index does.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edge {
    attr: u64,
    value_constant: u64,
    value_lti: u64,
    activation: f64,
    /// SQLite's rowid: one more than the largest in the table when the row was inserted. Rows
    /// with equal activation are crawled in descending rowid order.
    rowid: u64,
}

impl Edge {
    fn key(&self) -> (u64, u64, u64) {
        (self.attr, self.value_constant, self.value_lti)
    }
}

/// A long-term identifier's row (upstream `smem_lti`) and its edges.
#[derive(Debug, Clone, Default)]
struct Lti {
    edges: Vec<Edge>,
    lti_edges: u64,
    activation: f64,
    activations_total: f64,
    activations_last: u64,
    activations_first: u64,
    /// Whether upstream's `smem_prohibited` has a row for the identifier (added when it is
    /// first stored); only then can it be prohibited.
    prohibit_row: bool,
    /// Upstream `smem_prohibited`: `Some(dirty)` while prohibited.
    prohibited: Option<bool>,
    /// Upstream `smem_activation_history`, kept for base-level activation.
    history: Option<History>,
}

/// Upstream `SMEM_ACT_HISTORY_ENTRIES`.
const HISTORY_ENTRIES: i64 = 10;
/// The columns of a history row: the times, then the touches.
const HISTORY_COLUMNS: i64 = 2 * HISTORY_ENTRIES;

/// An identifier's last accesses, newest first, 0 where there is none.
#[derive(Debug, Clone, Default)]
struct History {
    times: [u64; HISTORY_ENTRIES as usize],
    touches: [f64; HISTORY_ENTRIES as usize],
}

/// Counters upstream keeps for choosing the most selective cue element.
#[derive(Debug, Clone, Default)]
struct Frequencies {
    /// `smem_attribute_frequency`: edges per attribute.
    attribute: Vec<(u64, u64)>,
    /// `smem_wmes_constant_frequency`: edges per attribute and constant value.
    constant: Vec<((u64, u64), u64)>,
    /// `smem_wmes_lti_frequency`: edges per attribute and identifier value.
    lti: Vec<((u64, u64), u64)>,
}

fn adjust<K: PartialEq + Copy>(table: &mut Vec<(K, u64)>, key: K, delta: i64) {
    match table.iter_mut().find(|(k, _)| *k == key) {
        Some((_, count)) => *count = count.saturating_add_signed(delta),
        None if delta > 0 => table.push((key, delta as u64)),
        None => {}
    }
}

/// Semantic memory statistics (upstream `smem_stat_container`) that the port keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SmemStats {
    /// `retrieves`
    pub retrievals: u64,
    /// `queries`
    pub queries: u64,
    /// `stores`
    pub stores: u64,
    /// `nodes`
    pub nodes: u64,
    /// `edges`
    pub edges: u64,
    /// `act_updates`: activation computations.
    pub act_updates: u64,
}

/// The long-term store.
#[derive(Debug, Default)]
pub struct SemanticMemory {
    /// Settings.
    pub settings: SmemSettings,
    /// Statistics.
    pub stats: SmemStats,
    /// Symbols by hash id (`hashes[i]` has id `i + 1`).
    hashes: Vec<SymbolId>,
    /// Identifiers by id.
    ltis: BTreeMap<u64, Lti>,
    /// Upstream `lti_id_counter`: the last id given out by [`Self::add_lti`].
    lti_id_counter: u64,
    /// Upstream `smem_lti_alias`: identifiers named `@name` in `smem --add`.
    aliases: Vec<(alloc::string::String, LtiId)>,
    frequencies: Frequencies,
    /// Upstream `smem_max_cycle`: the activation clock.
    max_cycle: u64,
}

impl SemanticMemory {
    /// An empty store with default settings.
    pub fn new() -> Self {
        Self { max_cycle: 1, ..Self::default() }
    }

    /// Number of long-term identifiers.
    pub fn len(&self) -> usize {
        self.ltis.len()
    }

    /// Whether the store has no long-term identifiers.
    pub fn is_empty(&self) -> bool {
        self.ltis.is_empty()
    }

    /// Whether `lti` exists.
    pub fn contains(&self, lti: LtiId) -> bool {
        self.lti(lti).is_some()
    }

    fn lti(&self, lti: LtiId) -> Option<&Lti> {
        self.ltis.get(&lti.0)
    }

    fn lti_mut(&mut self, lti: LtiId) -> Option<&mut Lti> {
        self.ltis.get_mut(&lti.0)
    }

    /// Upstream `hash`: the symbol's hash id, if it has one.
    pub(crate) fn hash(&self, symbol: SymbolId) -> Option<u64> {
        self.hashes.iter().position(|s| *s == symbol).map(|i| i as u64 + 1)
    }

    /// Upstream `hash` with `add_on_fail`.
    fn hash_or_add(&mut self, symbol: SymbolId) -> u64 {
        self.hash(symbol).unwrap_or_else(|| {
            self.hashes.push(symbol);
            self.hashes.len() as u64
        })
    }

    fn rhash(&self, hash: u64) -> SymbolId {
        self.hashes[hash as usize - 1]
    }

    /// Upstream `add_new_LTI`: a new identifier with no edges.
    pub fn add_lti(&mut self) -> LtiId {
        self.lti_id_counter += 1;
        while self.ltis.contains_key(&self.lti_id_counter) {
            self.lti_id_counter += 1;
        }
        self.add_specific_lti(LtiId(self.lti_id_counter))
    }

    /// Upstream `add_specific_LTI`: a new identifier with the given id and no edges.
    pub fn add_specific_lti(&mut self, lti: LtiId) -> LtiId {
        self.ltis.insert(lti.0, Lti::default());
        self.stats.nodes += 1;
        lti
    }

    /// Upstream `get_or_add_lti_with_alias`: the identifier named `@alias`, new if there is none.
    pub fn lti_with_alias(&mut self, alias: &str) -> LtiId {
        if let Some((_, lti)) = self.aliases.iter().find(|(a, _)| a == alias) {
            return *lti;
        }
        let lti = self.add_lti();
        self.aliases.push((alias.into(), lti));
        lti
    }

    /// An identifier's edges in upstream's order (upstream `web_expand`).
    pub fn edges(&self, lti: LtiId) -> Vec<(SymbolId, SmemValue)> {
        let Some(record) = self.lti(lti) else { return Vec::new() };
        record
            .edges
            .iter()
            .map(|e| {
                let value = if e.value_lti != 0 { SmemValue::Lti(LtiId(e.value_lti)) } else { SmemValue::Constant(self.rhash(e.value_constant)) };
                (self.rhash(e.attr), value)
            })
            .collect()
    }

    /// An identifier's activation (upstream `activation_value`).
    pub fn activation(&self, lti: LtiId) -> Option<f64> {
        self.lti(lti).map(|l| l.activation)
    }

    /// Upstream `LTM_to_DB` as the agent's stores call it: replaces an identifier's edges with
    /// `edges` (duplicates are stored once), then activates it.
    pub fn store(&mut self, lti: LtiId, edges: &[(SymbolId, SmemValue)]) {
        self.store_edges(lti, edges, true, true);
    }

    /// Upstream `LTM_to_DB`. With `remove_old_children` the identifier's edges are replaced
    /// (upstream `disconnect_ltm` first); otherwise `edges` are added to them, skipping those
    /// it has. With `activate` the identifier is activated (an access if `activate-on-add`)
    /// and the new edges take its activation; otherwise they start at [`ACT_LOW`].
    pub fn store_edges(&mut self, lti: LtiId, edges: &[(SymbolId, SmemValue)], remove_old_children: bool, activate: bool) {
        let Some(record) = self.lti(lti) else { return };
        let (existing_edges, existing_lti_edges) =
            if remove_old_children { (0, 0) } else { (record.edges.len() as u64, record.lti_edges) };
        if remove_old_children {
            self.disconnect(lti);
        }

        // Upstream collects the new edges as sets ordered by hash id.
        let mut constants: Vec<(u64, u64)> = Vec::new();
        let mut ltis: Vec<(u64, u64)> = Vec::new();
        let mut attributes: Vec<u64> = Vec::new();
        for (attr, value) in edges {
            let attr = self.hash_or_add(*attr);
            let has = |smem: &Self, pred: &dyn Fn(&Edge) -> bool| smem.lti(lti).is_some_and(|r| r.edges.iter().any(pred));
            if !attributes.contains(&attr) && (remove_old_children || !has(self, &|e| e.attr == attr)) {
                attributes.push(attr);
            }
            match value {
                SmemValue::Constant(symbol) => {
                    let value = self.hash_or_add(*symbol);
                    let stored = !remove_old_children && has(self, &|e| e.attr == attr && e.value_constant == value);
                    if !stored && !constants.contains(&(attr, value)) {
                        constants.push((attr, value));
                    }
                }
                SmemValue::Lti(child) => {
                    let stored = !remove_old_children && has(self, &|e| e.attr == attr && e.value_lti == child.0);
                    if !stored && !ltis.contains(&(attr, child.0)) {
                        ltis.push((attr, child.0));
                    }
                }
            }
        }
        constants.sort_unstable();
        ltis.sort_unstable();
        attributes.sort_unstable();

        let new_edges = existing_edges + (constants.len() + ltis.len()) as u64;
        let thresh = self.settings.thresh;
        let record = self.lti_mut(lti).expect("checked above");
        // Crossing the threshold: upstream sets the existing edges to the (still low) new-edge
        // activation.
        if existing_edges < thresh && new_edges >= thresh {
            for edge in &mut record.edges {
                edge.activation = ACT_LOW;
            }
        }
        record.lti_edges = existing_lti_edges + ltis.len() as u64;
        record.prohibit_row = true;
        // Upstream activates before inserting the edges; the new edges take the activation, or
        // stay low once the identifier has `thresh` edges.
        let mut edge_activation = ACT_LOW;
        if activate {
            let activate_on_add = self.settings.activate_on_add;
            let activation = self.activate(lti, activate_on_add, new_edges);
            if new_edges < thresh {
                edge_activation = activation;
            }
        }

        let mut rowid = self.ltis.values().flat_map(|l| l.edges.iter().map(|e| e.rowid)).max().unwrap_or(0);
        let record = self.lti_mut(lti).expect("checked above");
        for &(attr, value) in &constants {
            rowid += 1;
            record.edges.push(Edge { attr, value_constant: value, value_lti: 0, activation: edge_activation, rowid });
        }
        for &(attr, child) in &ltis {
            rowid += 1;
            record.edges.push(Edge { attr, value_constant: 0, value_lti: child, activation: edge_activation, rowid });
        }
        record.edges.sort_by_key(Edge::key);
        for &(attr, value) in &constants {
            adjust(&mut self.frequencies.constant, (attr, value), 1);
        }
        for &(attr, child) in &ltis {
            adjust(&mut self.frequencies.lti, (attr, child), 1);
        }
        for &attr in &attributes {
            adjust(&mut self.frequencies.attribute, attr, 1);
        }
        self.stats.edges += new_edges - existing_edges;
    }

    /// Upstream `disconnect_ltm`: removes an identifier's edges and their counts.
    fn disconnect(&mut self, lti: LtiId) {
        let Some(record) = self.lti_mut(lti) else { return };
        let edges = core::mem::take(&mut record.edges);
        let mut attributes: Vec<u64> = Vec::new();
        for edge in &edges {
            if !attributes.contains(&edge.attr) {
                attributes.push(edge.attr);
            }
            if edge.value_lti == 0 {
                adjust(&mut self.frequencies.constant, (edge.attr, edge.value_constant), -1);
            } else {
                adjust(&mut self.frequencies.lti, (edge.attr, edge.value_lti), -1);
            }
        }
        for attr in attributes {
            adjust(&mut self.frequencies.attribute, attr, -1);
        }
        self.stats.edges -= edges.len() as u64;
    }

    /// Upstream `lti_activate` (without spreading): records an access when `add_access`, and
    /// returns the new activation, which the identifier's edges take while it has fewer than
    /// [`SmemSettings::thresh`] of them.
    pub fn activate(&mut self, lti: LtiId, add_access: bool, num_edges: u64) -> f64 {
        const TOUCHES: f64 = 1.0;
        let Some(record) = self.lti(lti) else { return ACT_LOW };
        let (mut prev_n, prev_t, prev_1) = (record.activations_total, record.activations_last, record.activations_first);
        let prohibited = record.prohibited;
        let time_now;
        if add_access {
            time_now = self.max_cycle;
            self.max_cycle += 1;
            // An access lifts a prohibition, first dropping the access it prohibited.
            if prohibited == Some(true) {
                prev_n -= self.history_column(lti, HISTORY_ENTRIES);
                self.history_remove(lti);
            }
            if prohibited.is_some() {
                self.lti_mut(lti).expect("checked above").prohibited = None;
            }
            if self.settings.activation_mode == ActivationMode::BaseLevel && self.settings.base_update == BaseUpdate::Incremental {
                for thresh in self.settings.base_incremental_threshes.clone() {
                    let time_diff = time_now as i64 - thresh;
                    if thresh > 0 && time_diff > 0 {
                        let to_update: Vec<LtiId> =
                            self.ltis.iter().filter(|(_, r)| r.activations_last as i64 == time_diff).map(|(&id, _)| LtiId(id)).collect();
                        for other in to_update {
                            let edges = self.edge_count(other);
                            self.activate(other, false, edges);
                        }
                    }
                }
            }
        } else {
            // Recomputing without an access drops a prohibited access from the history; the
            // identifier stays prohibited.
            if prohibited == Some(true) {
                prev_n -= self.history_column(lti, HISTORY_ENTRIES);
                self.history_remove(lti);
                self.lti_mut(lti).expect("checked above").prohibited = Some(false);
            }
            time_now = self.max_cycle;
        }
        self.stats.act_updates += 1;
        let record = self.lti_mut(lti).expect("checked above");
        record.activations_total = prev_n + if add_access { TOUCHES } else { 0.0 };
        record.activations_last = if add_access { time_now } else { prev_t };
        record.activations_first = if prev_n == 0.0 { if add_access { time_now } else { 0 } } else { prev_1 };

        let new_activation = match self.settings.activation_mode {
            ActivationMode::Recency => time_now as f64,
            ActivationMode::Frequency => prev_n + if add_access { 1.0 } else { 0.0 },
            ActivationMode::BaseLevel => {
                if add_access {
                    if prev_1 == 0 && prohibited.is_none() {
                        self.history_add(lti, time_now, TOUCHES);
                    } else {
                        self.history_push(lti, time_now, TOUCHES);
                    }
                }
                let step = u64::from(add_access);
                self.calc_base(lti, time_now + step, (prev_n + if add_access { TOUCHES } else { 0.0 }) as u64, prev_1)
            }
        };
        // With nothing to go on, upstream pretends the identifier was accessed once when the
        // agent was created.
        let new_base = if new_activation == ACT_LOW || new_activation == 0.0 {
            let base = libm::pow((self.max_cycle as i64 + self.settings.base_unused_age_offset) as f64, -self.settings.base_decay);
            libm::log(base / (1.0 + base))
        } else {
            new_activation
        };
        let edge_activation = if num_edges < self.settings.thresh { new_base } else { ACT_MAX };
        let record = self.lti_mut(lti).expect("checked above");
        record.activation = new_base;
        for edge in &mut record.edges {
            edge.activation = edge_activation;
        }
        new_base
    }

    /// Upstream `lti_calc_base`: the log odds of the identifier's decayed accesses at
    /// `time_now`, from its access history.
    ///
    /// Upstream reads the history row by column index (`t1`..`t10`, then `touch1`..`touch10`)
    /// and counts the entries until a zero column. A full history therefore runs on into the
    /// touch columns, so the estimate for accesses older than the history is never added: only
    /// the last ten accesses count. [`Self::history_column`] reproduces those reads.
    fn calc_base(&mut self, lti: LtiId, time_now: u64, mut n: u64, activations_first: u64) -> f64 {
        let d = self.settings.base_decay;
        let t_n = time_now.wrapping_sub(activations_first);
        if n == 0 {
            let record = self.lti(lti).expect("checked by the caller");
            n = record.activations_total as u64;
        }
        let column = |i: i64| self.history_column(lti, i);
        let mut available: i64 = 0;
        while column(available) as i64 != 0 {
            available += 1;
        }
        let t_k = time_now.wrapping_sub(column(available - 1) as i64 as u64);
        let mut sum = 0.0;
        let mut small_n = 0.0;
        let mut recent_time: i64 = 0;
        for i in 0..available {
            let touches = column(i + HISTORY_ENTRIES);
            small_n += touches;
            let time_diff = time_now as i64 - column(i) as i64;
            if i == 0 && n > 0 {
                recent_time = time_diff;
            }
            sum += touches * libm::pow(time_diff as f64, -d);
        }
        if n as f64 > small_n && available == HISTORY_ENTRIES {
            if t_n != t_k {
                let numerator = n.wrapping_sub(HISTORY_ENTRIES as u64) as f64 * (libm::pow(t_n as f64, 1.0 - d) - libm::pow(t_k as f64, 1.0 - d));
                let denominator = (1.0 - d) * t_n.wrapping_sub(t_k) as f64;
                sum += numerator / denominator;
            } else {
                sum += (n as f64 - small_n) * libm::pow(t_n as f64, -d);
            }
        }
        if sum <= 0.0 {
            return ACT_LOW;
        }
        let odds = libm::log(sum / (1.0 + sum));
        if recent_time != 0 && self.settings.base_inhibition {
            let inhibition = libm::pow(1.0 + libm::pow(recent_time as f64 / 10.0, -1.0), -1.0);
            return odds + libm::log(inhibition / (1.0 + inhibition));
        }
        odds
    }

    /// Column `i` of the identifier's `smem_activation_history` row as upstream reads it: the
    /// access times `t1`..`t10`, then the touches `touch1`..`touch10`, and 0 for a column out
    /// of range or a missing row (what SQLite returns).
    fn history_column(&self, lti: LtiId, i: i64) -> f64 {
        let Some(history) = self.lti(lti).and_then(|l| l.history.as_ref()) else { return 0.0 };
        match i {
            0..HISTORY_ENTRIES => history.times[i as usize] as f64,
            HISTORY_ENTRIES..HISTORY_COLUMNS => history.touches[(i - HISTORY_ENTRIES) as usize],
            _ => 0.0,
        }
    }

    /// Upstream `history_add`: starts the history with one access (an insert that fails if the
    /// row exists).
    fn history_add(&mut self, lti: LtiId, time: u64, touches: f64) {
        let record = self.lti_mut(lti).expect("checked by the caller");
        if record.history.is_none() {
            let mut history = History::default();
            history.times[0] = time;
            history.touches[0] = touches;
            record.history = Some(history);
        }
    }

    /// Upstream `history_push`: records an access, dropping the oldest (an update that does
    /// nothing without a row).
    fn history_push(&mut self, lti: LtiId, time: u64, touches: f64) {
        if let Some(history) = self.lti_mut(lti).and_then(|l| l.history.as_mut()) {
            history.times.rotate_right(1);
            history.touches.rotate_right(1);
            history.times[0] = time;
            history.touches[0] = touches;
        }
    }

    /// Upstream `history_remove`: forgets the latest access.
    fn history_remove(&mut self, lti: LtiId) {
        if let Some(history) = self.lti_mut(lti).and_then(|l| l.history.as_mut()) {
            history.times.rotate_left(1);
            history.touches.rotate_left(1);
            history.times[HISTORY_ENTRIES as usize - 1] = 0;
            history.touches[HISTORY_ENTRIES as usize - 1] = 0.0;
        }
    }

    /// Upstream `prohibit_set`: marks a stored identifier prohibited, with its latest access
    /// to be dropped, until its next access.
    pub fn prohibit(&mut self, lti: LtiId) {
        if let Some(record) = self.lti_mut(lti).filter(|r| r.prohibit_row) {
            record.prohibited = Some(true);
        }
    }

    /// Number of edges of an identifier.
    pub fn edge_count(&self, lti: LtiId) -> u64 {
        self.lti(lti).map_or(0, |l| l.edges.len() as u64)
    }
}

/// A number in a math query or a stored value, compared as C++ compares `int64_t` and
/// `double`: as integers when both are, otherwise as doubles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
}

impl Number {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(i) => i as f64,
            Self::Float(x) => x,
        }
    }

    fn lt(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a < b,
            _ => self.as_f64() < other.as_f64(),
        }
    }

    fn gt(self, other: Self) -> bool {
        other.lt(self)
    }

    fn le(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a <= b,
            _ => self.as_f64() <= other.as_f64(),
        }
    }

    fn ge(self, other: Self) -> bool {
        other.le(self)
    }
}

/// A math query on one cue attribute (upstream `MathQuery` and its subclasses).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathQuery {
    /// `^less`
    Less(Number),
    /// `^greater`
    Greater(Number),
    /// `^less-or-equal`
    LessOrEqual(Number),
    /// `^greater-or-equal`
    GreaterOrEqual(Number),
    /// `^max`: the best value committed so far and the one staged by the current candidate.
    Max { best: (f64, i64), staged: (f64, i64) },
    /// `^min`, as `Max`.
    Min { best: (f64, i64), staged: (f64, i64) },
}

impl MathQuery {
    /// A new `^max` query.
    pub fn max() -> Self {
        let low = (-f64::MAX, -i64::MAX);
        Self::Max { best: low, staged: low }
    }

    /// A new `^min` query.
    pub fn min() -> Self {
        let high = (f64::MAX, i64::MAX);
        Self::Min { best: high, staged: high }
    }

    /// Upstream `valueIsAcceptable`.
    fn accepts(&mut self, value: Number) -> bool {
        match self {
            Self::Less(t) => value.lt(*t),
            Self::Greater(t) => value.gt(*t),
            Self::LessOrEqual(t) => value.le(*t),
            Self::GreaterOrEqual(t) => value.ge(*t),
            Self::Max { best, staged } => {
                let ok = value.gt(Number::Float(best.0)) && value.gt(Number::Int(best.1));
                if ok {
                    match value {
                        Number::Float(x) if x > staged.0 => staged.0 = x,
                        Number::Int(i) if i > staged.1 => staged.1 = i,
                        _ => {}
                    }
                }
                ok
            }
            Self::Min { best, staged } => {
                let ok = value.lt(Number::Float(best.0)) && value.lt(Number::Int(best.1));
                if ok {
                    match value {
                        Number::Float(x) if x < staged.0 => staged.0 = x,
                        Number::Int(i) if i < staged.1 => staged.1 = i,
                        _ => {}
                    }
                }
                ok
            }
        }
    }

    fn commit(&mut self) {
        if let Self::Max { best, staged } | Self::Min { best, staged } = self {
            *best = *staged;
        }
    }

    fn rollback(&mut self) {
        if let Self::Max { best, staged } | Self::Min { best, staged } = self {
            *staged = *best;
        }
    }
}

/// What a cue element tests (upstream `smem_cue_element_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueKind {
    /// Any value of the attribute (upstream `attr_t`): an identifier value not linked to
    /// long-term memory, or a math query's attribute.
    Attr,
    /// A constant value, by hash id (upstream `value_const_t`).
    Constant(u64),
    /// A long-term identifier value (upstream `value_lti_t`).
    Lti(LtiId),
}

/// One element of a query cue (upstream `smem_weighted_cue_element`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueElement {
    /// Number of edges that match it: the most selective element generates the candidates.
    pub weight: u64,
    /// Attribute hash id.
    pub attr: u64,
    /// What it tests.
    pub kind: CueKind,
    /// `false` for a `^neg-query` element.
    pub positive: bool,
    /// A math query on the attribute's values.
    pub math: Option<MathQuery>,
}

/// `std::priority_queue` as libstdc++ implements it, so elements that compare equal come out
/// in the same order as upstream's. `less(a, b)` is the queue's comparator; the top is an
/// element no other is greater than.
struct StdHeap<T> {
    items: Vec<T>,
}

impl<T: Copy> StdHeap<T> {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn top(&self) -> Option<T> {
        self.items.first().copied()
    }

    /// libstdc++ `__push_heap`.
    fn sift_up(&mut self, mut hole: usize, top: usize, value: T, less: &impl Fn(&T, &T) -> bool) {
        while hole > top {
            let parent = (hole - 1) / 2;
            if !less(&self.items[parent], &value) {
                break;
            }
            self.items[hole] = self.items[parent];
            hole = parent;
        }
        self.items[hole] = value;
    }

    fn push(&mut self, value: T, less: &impl Fn(&T, &T) -> bool) {
        self.items.push(value);
        let hole = self.items.len() - 1;
        self.sift_up(hole, 0, value, less);
    }

    /// libstdc++ `pop_heap` (with `__adjust_heap`) and `pop_back`.
    fn pop(&mut self, less: &impl Fn(&T, &T) -> bool) -> Option<T> {
        let top = self.top()?;
        let last = self.items.pop().expect("not empty");
        let len = self.items.len();
        if len > 0 {
            let mut hole = 0;
            let mut child = 0;
            while child < len.saturating_sub(1) / 2 {
                child = 2 * (child + 1);
                if less(&self.items[child], &self.items[child - 1]) {
                    child -= 1;
                }
                self.items[hole] = self.items[child];
                hole = child;
            }
            if len.is_multiple_of(2) && child == (len - 2) / 2 {
                child = 2 * (child + 1);
                self.items[hole] = self.items[child - 1];
                hole = child - 1;
            }
            self.sift_up(hole, 0, last, less);
        }
        Some(top)
    }
}

/// Orders cue elements as upstream's `smem_prioritized_weighted_cue`: least weight first.
pub fn order_cue(elements: Vec<CueElement>) -> Vec<CueElement> {
    let less = |a: &CueElement, b: &CueElement| a.weight > b.weight;
    let mut heap = StdHeap::new();
    for element in elements {
        heap.push(element, &less);
    }
    core::iter::from_fn(|| heap.pop(&less)).collect()
}

impl SemanticMemory {
    /// The weight of a cue element: the number of edges with the attribute (and value), or
    /// `None` when upstream's frequency tables have no row for it.
    pub fn cue_weight(&self, attr: u64, kind: CueKind) -> Option<u64> {
        let find = |table: &[((u64, u64), u64)], key| table.iter().find(|(k, _)| *k == key).map(|(_, n)| *n);
        match kind {
            CueKind::Attr => self.frequencies.attribute.iter().find(|(a, _)| *a == attr).map(|(_, n)| *n),
            CueKind::Constant(value) => find(&self.frequencies.constant, (attr, value)),
            CueKind::Lti(lti) => find(&self.frequencies.lti, (attr, lti.0)),
        }
    }

    /// Upstream `web_*_all`: the edges an element matches, as (identifier, activation), in
    /// descending activation and, for equal activations, descending rowid.
    fn crawl(&self, element: &CueElement) -> Vec<(LtiId, f64)> {
        let mut rows: Vec<(LtiId, f64, u64)> = Vec::new();
        for (&id, lti) in &self.ltis {
            for e in lti.edges.iter().filter(|e| e.attr == element.attr) {
                let matches = match element.kind {
                    CueKind::Attr => true,
                    CueKind::Constant(value) => e.value_lti == 0 && e.value_constant == value,
                    CueKind::Lti(child) => e.value_constant == 0 && e.value_lti == child.0,
                };
                if matches {
                    rows.push((LtiId(id), e.activation, e.rowid));
                }
            }
        }
        rows.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.cmp(&a.2)));
        rows.into_iter().map(|(lti, act, _)| (lti, act)).collect()
    }

    /// Upstream `web_*_child`: whether an identifier has an edge matching an element.
    fn has_feature(&self, lti: LtiId, element: &CueElement) -> bool {
        self.lti(lti).is_some_and(|l| {
            l.edges.iter().any(|e| {
                e.attr == element.attr
                    && match element.kind {
                        CueKind::Attr => true,
                        CueKind::Constant(value) => e.value_constant == value,
                        CueKind::Lti(child) => e.value_constant == 0 && e.value_lti == child.0,
                    }
            })
        })
    }

    /// Upstream's math check: whether any numeric constant value of the attribute is acceptable.
    fn math_met(&self, lti: LtiId, element: &mut CueElement, number: &impl Fn(SymbolId) -> Option<Number>) -> bool {
        let Some(math) = element.math.as_mut() else { return false };
        let mut met = false;
        if let Some(record) = self.lti(lti) {
            for e in record.edges.iter().filter(|e| e.attr == element.attr && e.value_constant != 0) {
                if let Some(value) = number(self.rhash(e.value_constant)) {
                    met |= math.accepts(value);
                }
            }
        }
        met
    }

    /// Upstream `process_query` for a full query (one result): the identifier the cue
    /// retrieves, given the cue in [`order_cue`] order. With base-level activation and the naive
    /// update policy, first recomputes the activation of every candidate. Identifiers in `prohibit` are not
    /// retrieved; the one retrieved is added to it. `full_search` (a `^max` / `^min` math
    /// query) looks at every candidate instead of stopping at the first.
    pub fn query(&mut self, cue: &mut [CueElement], prohibit: &mut Vec<LtiId>, full_search: bool, number: impl Fn(SymbolId) -> Option<Number>) -> Option<LtiId> {
        let cand_index = cue.iter().position(|e| e.positive)?;
        if self.settings.activation_mode == ActivationMode::BaseLevel && self.settings.base_update == BaseUpdate::Naive {
            // Recompute every candidate's activation first, in upstream's `std::set` order.
            let to_update: BTreeSet<LtiId> = self.crawl(&cue[cand_index]).into_iter().map(|(lti, _)| lti).collect();
            for lti in to_update {
                let edges = self.edge_count(lti);
                self.activate(lti, false, edges);
            }
        }
        let rows = self.crawl(&cue[cand_index]);
        let mut next = 0;
        let parent_less = |a: &(f64, u64), b: &(f64, u64)| b.0 > a.0;
        let mut plentiful = StdHeap::new();
        while next < rows.len() && rows[next].1 == ACT_MAX {
            let lti = rows[next].0;
            plentiful.push((self.activation(lti).unwrap_or(ACT_LOW), lti.0), &parent_less);
            next += 1;
        }
        let mut king: Option<LtiId> = None;
        let mut kings: Vec<(f64, u64)> = Vec::new();
        let mut first_element = false;
        while (kings.is_empty() || full_search) && (next < rows.len() || plentiful.top().is_some()) {
            let use_db = match plentiful.top() {
                _ if next >= rows.len() => false,
                None => true,
                Some(top) => rows[next].1 > top.0,
            };
            let (cand, act) = if use_db {
                next += 1;
                rows[next - 1]
            } else {
                let (act, lti) = plentiful.pop(&parent_less).expect("checked above");
                (LtiId(lti), act)
            };
            if prohibit.contains(&cand) {
                continue;
            }
            let mut good = true;
            for (i, element) in cue.iter_mut().enumerate() {
                if i == cand_index && element.math.is_none() {
                    continue;
                }
                let has_feature = self.has_feature(cand, element);
                good = if element.math.is_some() && has_feature {
                    self.math_met(cand, element, &number)
                } else if element.positive {
                    has_feature
                } else {
                    !has_feature
                };
                if !good {
                    break;
                }
            }
            if good {
                king = Some(cand);
                first_element = true;
                kings.push((act, cand.0));
                prohibit.push(cand);
            }
            for element in cue.iter_mut() {
                if let Some(math) = element.math.as_mut() {
                    if good && first_element {
                        math.commit();
                    } else if first_element {
                        math.rollback();
                    }
                }
            }
        }
        if full_search && cue.iter().any(|e| e.math.is_some()) {
            return king;
        }
        // Upstream's `all_king_ids` set: the highest (activation, id).
        kings.into_iter().max_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))).map(|(_, lti)| LtiId(lti))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// The heap pops elements in comparator order, whatever order they were pushed in.
    #[test]
    fn std_heap_pops_in_comparator_order() {
        let less = |a: &u64, b: &u64| a > b;
        for pushes in [[5u64, 1, 4, 2, 3, 6, 0], [0, 1, 2, 3, 4, 5, 6], [6, 5, 4, 3, 2, 1, 0]] {
            let mut heap = StdHeap::new();
            for x in pushes {
                heap.push(x, &less);
            }
            let popped: Vec<u64> = core::iter::from_fn(|| heap.pop(&less)).collect();
            assert_eq!(popped, vec![0, 1, 2, 3, 4, 5, 6]);
        }
    }

    #[test]
    fn store_replaces_edges_and_keeps_counts() {
        let mut smem = SemanticMemory::new();
        let lti = smem.add_lti();
        let (a, b, x, y) = (SymbolId(100), SymbolId(101), SymbolId(102), SymbolId(103));
        smem.store(lti, &[(a, SmemValue::Constant(x)), (b, SmemValue::Constant(y)), (a, SmemValue::Constant(x))]);
        assert_eq!(smem.edges(lti), vec![(a, SmemValue::Constant(x)), (b, SmemValue::Constant(y))]);
        assert_eq!(smem.stats.edges, 2);
        smem.store(lti, &[(b, SmemValue::Constant(x))]);
        assert_eq!(smem.edges(lti), vec![(b, SmemValue::Constant(x))]);
        assert_eq!(smem.stats.edges, 1);
        let (ha, hb, hx) = (smem.hash(a).unwrap(), smem.hash(b).unwrap(), smem.hash(x).unwrap());
        assert_eq!(smem.cue_weight(ha, CueKind::Attr), Some(0));
        assert_eq!(smem.cue_weight(hb, CueKind::Constant(hx)), Some(1));
    }

    /// A base-level store with one stored identifier, never accessed.
    fn base_level() -> (SemanticMemory, LtiId) {
        let mut smem = SemanticMemory::new();
        SmemSetting::ActivationMode(ActivationMode::BaseLevel).apply(&mut smem.settings);
        let lti = smem.add_lti();
        smem.store(lti, &[(SymbolId(100), SmemValue::Constant(SymbolId(101)))]);
        (smem, lti)
    }

    fn log_odds(sum: f64) -> f64 {
        libm::log(sum / (1.0 + sum))
    }

    /// An identifier never accessed is estimated as accessed once when the agent started;
    /// accesses at times 1 and 2 give `ln(s / (1 + s))` with `s = (3 - 2)^-0.5 + (3 - 1)^-0.5`
    /// (upstream computes at one past the access time).
    #[test]
    fn base_level_activation_decays_each_access() {
        let (mut smem, lti) = base_level();
        assert_eq!(smem.activation(lti), Some(log_odds(1.0)));
        smem.activate(lti, true, 1);
        assert_eq!(smem.activation(lti), Some(log_odds(1.0)));
        smem.activate(lti, true, 1);
        let expected = log_odds(1.0 + libm::pow(2.0, -0.5));
        assert!((smem.activation(lti).unwrap() - expected).abs() < 1e-12);
        assert_eq!(smem.lti(lti).unwrap().edges[0].activation, smem.activation(lti).unwrap());
    }

    /// Upstream's history read runs on into the touch columns when all ten entries are used,
    /// so only the last ten accesses count.
    #[test]
    fn base_level_activation_counts_only_the_last_ten_accesses() {
        let (mut smem, lti) = base_level();
        for _ in 0..12 {
            smem.activate(lti, true, 1);
        }
        // Accesses at times 3..=12, computed at time 13.
        let sum: f64 = (3..=12).map(|t| libm::pow((13 - t) as f64, -0.5)).sum();
        assert!((smem.activation(lti).unwrap() - log_odds(sum)).abs() < 1e-12);
    }

    /// A prohibited identifier's latest access is dropped when it is next accessed.
    #[test]
    fn base_level_access_after_a_prohibit_drops_the_prohibited_access() {
        let (mut smem, lti) = base_level();
        smem.activate(lti, true, 1); // time 1
        smem.activate(lti, true, 1); // time 2
        smem.prohibit(lti);
        smem.activate(lti, true, 1); // time 3; the access at 2 is dropped
        let sum = libm::pow(1.0, -0.5) + libm::pow(3.0, -0.5);
        assert!((smem.activation(lti).unwrap() - log_odds(sum)).abs() < 1e-12);
        assert_eq!(smem.lti(lti).unwrap().activations_total, 2.0);
    }

    /// `base-incremental-threshes` toggles values in and out of the set, as upstream does.
    #[test]
    fn incremental_threshes_toggle() {
        let mut settings = SmemSettings::default();
        SmemSetting::parse("base-incremental-threshes", "5").unwrap().apply(&mut settings);
        SmemSetting::parse("base-incremental-threshes", "10").unwrap().apply(&mut settings);
        assert_eq!(settings.base_incremental_threshes, BTreeSet::from([5]));
    }
}
