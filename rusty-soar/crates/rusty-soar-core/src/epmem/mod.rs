//! Episodic memory's store, ported from upstream `episodic_memory/` with its SQLite database
//! replaced by in-memory tables (upstream's default database is in memory too).
//!
//! As upstream does, the store records only changes. A *wme row* stands for one
//! (parent, attribute, value) structure and has an id; each row keeps the time ranges it was in
//! working memory. Upstream's "now", "point" and interval-tree tables are that one list here:
//! the tree only makes it faster to search.
//!
//! The agent side (walking working memory, commands on the `^epmem` link, recall into working
//! memory) is in `agent/epmem.rs`.

pub(crate) mod query;

use crate::symbol::SymbolId;
use crate::wm::WmeKey;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

/// Upstream `EPMEM_NODEID_ROOT`: the node of the top state.
pub const NODE_ROOT: u64 = 0;

/// Upstream `EPMEM_HASH_ACCEPTABLE`: the attribute of an acceptable-preference WME with an
/// identifier value, whatever its real attribute; it reads back as `operator*`.
pub const HASH_ACCEPTABLE: u64 = 1;

/// When episodes are stored and commands answered (upstream `phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpmemPhase {
    /// After the output phase.
    #[default]
    Output,
    /// After the decision phase.
    Selection,
}

/// What causes an episode to be stored (upstream `trigger`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpmemTrigger {
    /// Every decision cycle.
    #[default]
    Dc,
    /// A new WME on the output link.
    Output,
    /// Nothing.
    None,
}

/// Overrides the trigger once (upstream `force`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpmemForce {
    /// Follow the trigger.
    #[default]
    Off,
    /// Store at the next chance.
    Remember,
    /// Don't store at the next chance.
    Ignore,
}

/// The order the graph match binds literals in (upstream `graph-match-ordering`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GmOrdering {
    /// Upstream sorts by address; the port by creation.
    #[default]
    Undefined,
    /// The cue's own order.
    Dfs,
    /// Most constrained: fewest candidate matches first.
    Mcv,
}

/// How a retrieval treats long-term identifiers' structure (upstream `merge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpmemMerge {
    /// Only what the episode says.
    #[default]
    None,
    /// The same in the port: upstream's difference is for recalled identifiers that take no
    /// children, and this version always allows them.
    Add,
}

/// Episodic memory settings (upstream `epmem_param_container`), with upstream's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct EpmemSettings {
    /// `learning`: episodic memory runs only when this is on.
    pub learning: bool,
    /// `phase`.
    pub phase: EpmemPhase,
    /// `trigger`.
    pub trigger: EpmemTrigger,
    /// `force`.
    pub force: EpmemForce,
    /// `exclusions`: attributes not recorded, with the structure under them.
    pub exclusions: BTreeSet<String>,
    /// `merge`.
    pub merge: EpmemMerge,
    /// `graph-match`: whether a query checks that a perfect match has the cue's structure.
    pub graph_match: bool,
    /// `graph-match-ordering`.
    pub gm_ordering: GmOrdering,
    /// `balance`: below 1, a query's cue WMEs count by their working-memory activation.
    pub balance: f64,
}

impl Default for EpmemSettings {
    fn default() -> Self {
        Self {
            learning: false,
            phase: EpmemPhase::Output,
            trigger: EpmemTrigger::Dc,
            force: EpmemForce::Off,
            exclusions: BTreeSet::from([String::from("epmem"), String::from("smem")]),
            merge: EpmemMerge::None,
            graph_match: true,
            gm_ordering: GmOrdering::Undefined,
            balance: 1.0,
        }
    }
}

/// One `epmem --set` (or `--enable` / `--disable`) setting.
#[derive(Debug, Clone, PartialEq)]
pub enum EpmemSetting {
    /// `learning`
    Learning(bool),
    /// `phase`
    Phase(EpmemPhase),
    /// `trigger`
    Trigger(EpmemTrigger),
    /// `force`
    Force(EpmemForce),
    /// `exclusions`: adds the attribute to the set, or removes it if present, as upstream does.
    Exclusion(String),
    /// `merge`
    Merge(EpmemMerge),
    /// `graph-match`
    GraphMatch(bool),
    /// `graph-match-ordering`
    GmOrdering(GmOrdering),
    /// `balance`, from 0 to 1.
    Balance(f64),
}

impl EpmemSetting {
    /// Parses `epmem --set <name> <value>`.
    pub fn parse(name: &str, value: &str) -> Result<Self, String> {
        match (name, value) {
            ("learning", "on") => Ok(Self::Learning(true)),
            ("learning", "off") => Ok(Self::Learning(false)),
            ("phase", "output") => Ok(Self::Phase(EpmemPhase::Output)),
            ("phase", "selection") => Ok(Self::Phase(EpmemPhase::Selection)),
            ("trigger", "dc") => Ok(Self::Trigger(EpmemTrigger::Dc)),
            ("trigger", "output") => Ok(Self::Trigger(EpmemTrigger::Output)),
            ("trigger", "none") => Ok(Self::Trigger(EpmemTrigger::None)),
            ("force", "off") => Ok(Self::Force(EpmemForce::Off)),
            ("force", "remember") => Ok(Self::Force(EpmemForce::Remember)),
            ("force", "ignore") => Ok(Self::Force(EpmemForce::Ignore)),
            ("exclusions", attr) => Ok(Self::Exclusion(attr.into())),
            ("balance", x) => match x.parse::<f64>() {
                Ok(x) if (0.0..=1.0).contains(&x) => Ok(Self::Balance(x)),
                _ => Err(alloc::format!("Invalid value for {}", name)),
            },
            ("merge", "none") => Ok(Self::Merge(EpmemMerge::None)),
            ("merge", "add") => Ok(Self::Merge(EpmemMerge::Add)),
            ("graph-match", "on") => Ok(Self::GraphMatch(true)),
            ("graph-match", "off") => Ok(Self::GraphMatch(false)),
            ("graph-match-ordering", "undefined") => Ok(Self::GmOrdering(GmOrdering::Undefined)),
            ("graph-match-ordering", "dfs") => Ok(Self::GmOrdering(GmOrdering::Dfs)),
            ("graph-match-ordering", "mcv") => Ok(Self::GmOrdering(GmOrdering::Mcv)),
            ("learning" | "phase" | "trigger" | "force" | "merge" | "graph-match" | "graph-match-ordering", _) => Err(alloc::format!("Invalid value for {}", name)),
            _ => Err(alloc::format!("Unsupported epmem setting {}", name)),
        }
    }

    /// Applies the setting.
    pub fn apply(self, settings: &mut EpmemSettings) {
        match self {
            Self::Learning(x) => settings.learning = x,
            Self::Phase(x) => settings.phase = x,
            Self::Trigger(x) => settings.trigger = x,
            Self::Force(x) => settings.force = x,
            Self::Exclusion(x) => {
                if !settings.exclusions.remove(&x) {
                    settings.exclusions.insert(x);
                }
            }
            Self::Merge(x) => settings.merge = x,
            Self::GraphMatch(x) => settings.graph_match = x,
            Self::GmOrdering(x) => settings.gm_ordering = x,
            Self::Balance(x) => settings.balance = x,
        }
    }
}

/// Episodic memory statistics (upstream `epmem_stat_container`) that the port keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EpmemStats {
    /// `ncbr`: `retrieve` commands.
    pub ncbr: u64,
    /// `nexts`: `next` commands.
    pub nexts: u64,
    /// `prevs`: `previous` commands.
    pub prevs: u64,
    /// `ncb_wmes`: WMEs in the last retrieval.
    pub ncb_wmes: u64,
    /// `cbr`: `query` commands.
    pub cbr: u64,
}

/// A row of `epmem_wmes_constant`: a WME with a constant value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConstantRow {
    pub(crate) parent: u64,
    pub(crate) attr: u64,
    pub(crate) value: SymbolId,
    pub(crate) value_hash: u64,
    /// Upstream `epmem_node_mins`: when the current range started.
    pub(crate) min: u64,
    /// Upstream `epmem_node_maxes`: whether the last thing done was to close a range.
    pub(crate) closed: bool,
}

/// A row of `epmem_wmes_identifier`: a WME with an identifier value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdentifierRow {
    pub(crate) parent: u64,
    pub(crate) attr: u64,
    pub(crate) child: u64,
    pub(crate) min: u64,
    pub(crate) closed: bool,
}

/// A time range a row was in working memory; `lti` is the long-term identifier its value was
/// linked to then, 0 for none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Range {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) lti: u64,
}

/// A row's range still open (upstream's "now" tables).
pub(crate) const OPEN: u64 = u64::MAX;

/// Upstream `epmem_id_pool`: (child node, identifier row) pairs free for reuse.
pub(crate) type IdPool = VecDeque<(u64, u64)>;

/// The episodic store. Field names follow upstream's `EpMem_Manager`.
#[derive(Debug, Default)]
pub struct EpisodicMemory {
    /// Settings.
    pub settings: EpmemSettings,
    /// Statistics.
    pub stats: EpmemStats,
    /// Whether the store has been set up, on the first use after `learning` is turned on
    /// (upstream connects its database then). Until then, nothing is tracked.
    pub(crate) connected: bool,
    /// Upstream `time`: the number the next episode will get.
    pub(crate) time: u64,
    /// Upstream `next_id`: the next identifier node.
    pub(crate) next_id: u64,
    /// Symbols by hash id (`hashes[i]` has id `i + 2`, as id 1 is [`HASH_ACCEPTABLE`]).
    pub(crate) hashes: Vec<SymbolId>,
    /// Stored episodes, ascending.
    pub(crate) episodes: Vec<u64>,
    /// `epmem_wmes_constant`; `constants[i]` has id `i + 1`.
    pub(crate) constants: Vec<ConstantRow>,
    /// Finds a constant row by (parent, attribute, value).
    pub(crate) constant_ids: BTreeMap<(u64, u64, SymbolId), u64>,
    /// `epmem_wmes_identifier`; `identifiers[i]` has id `i + 1`.
    pub(crate) identifiers: Vec<IdentifierRow>,
    /// Each constant row's ranges.
    pub(crate) constant_ranges: Vec<Vec<Range>>,
    /// Each identifier row's ranges.
    pub(crate) identifier_ranges: Vec<Vec<Range>>,
    /// Upstream `id->epmem_id`: nodes of identifiers now known.
    pub(crate) nodes: BTreeMap<SymbolId, u64>,
    /// Upstream `w->epmem_id`: rows of WMEs now encoded.
    pub(crate) wme_rows: BTreeMap<WmeKey, u64>,
    /// Upstream `epmem_id_repository`: for a parent and attribute, a pool.
    pub(crate) repository: BTreeMap<(u64, u64), usize>,
    /// The pools.
    pub(crate) pools: Vec<IdPool>,
    /// Upstream `epmem_id_ref_counts`: WMEs in working memory holding each node.
    pub(crate) ref_counts: BTreeMap<u64, BTreeSet<WmeKey>>,
    /// Upstream `epmem_id_replacement`: where an identifier row returns when its WME goes.
    pub(crate) replacement: BTreeMap<u64, (usize, u64)>,
    /// Upstream `epmem_wme_adds`: known identifiers that gained WMEs.
    pub(crate) wme_adds: BTreeSet<SymbolId>,
    /// Upstream `epmem_id_removes`.
    pub(crate) id_removes: VecDeque<SymbolId>,
    /// Upstream `epmem_node_removals`: constant rows and whether to close their ranges.
    pub(crate) node_removals: BTreeMap<u64, bool>,
    /// Upstream `epmem_edge_removals`: identifier rows, with the value's long-term identifier,
    /// and whether to close their ranges.
    pub(crate) edge_removals: BTreeMap<(u64, u64), bool>,
}

impl EpisodicMemory {
    /// An empty store with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of episodes stored.
    pub fn len(&self) -> usize {
        self.episodes.len()
    }

    /// Whether no episode is stored.
    pub fn is_empty(&self) -> bool {
        self.episodes.is_empty()
    }

    /// The number the next episode will get.
    pub fn time(&self) -> u64 {
        self.time
    }

    /// Upstream's `epmem_init_db` for a new database. `top` is the top state.
    pub(crate) fn connect(&mut self, top: SymbolId) {
        self.connected = true;
        self.time = 1;
        self.next_id = 1;
        self.nodes.insert(top, NODE_ROOT);
        self.wme_adds.insert(top);
    }

    /// Upstream `epmem_temporal_hash`: the hash id of a constant, new if it has none.
    pub(crate) fn hash(&mut self, symbol: SymbolId) -> u64 {
        let found = self.hashes.iter().position(|s| *s == symbol);
        let index = found.unwrap_or_else(|| {
            self.hashes.push(symbol);
            self.hashes.len() - 1
        });
        index as u64 + 2
    }

    /// The constant with a hash id; `None` for [`HASH_ACCEPTABLE`].
    pub(crate) fn reverse_hash(&self, hash: u64) -> Option<SymbolId> {
        self.hashes.get(hash.checked_sub(2)? as usize).copied()
    }

    /// The pool of a parent and attribute, made if there is none.
    pub(crate) fn pool_or_add(&mut self, parent: u64, attr: u64) -> usize {
        let pools = &mut self.pools;
        *self.repository.entry((parent, attr)).or_insert_with(|| {
            pools.push(IdPool::new());
            pools.len() - 1
        })
    }

    /// Upstream `epmem_next_episode`.
    pub(crate) fn next_episode(&self, memory: u64) -> Option<u64> {
        self.episodes.iter().find(|e| **e > memory).copied()
    }

    /// Upstream `epmem_previous_episode`.
    pub(crate) fn previous_episode(&self, memory: u64) -> Option<u64> {
        self.episodes.iter().rev().find(|e| **e < memory).copied()
    }

    /// Upstream `get_wmes_with_identifier_values`: (parent, attribute, child, long-term
    /// identifier) of the episode's identifier rows, by parent and child.
    pub(crate) fn identifiers_at(&self, memory: u64) -> Vec<(u64, u64, u64, u64)> {
        let mut rows: Vec<(u64, u64, u64, u64)> = Vec::new();
        for (row, ranges) in self.identifiers.iter().zip(&self.identifier_ranges) {
            if let Some(r) = ranges.iter().find(|r| r.start <= memory && memory <= r.end) {
                rows.push((row.parent, row.attr, row.child, r.lti));
            }
        }
        rows.sort_by_key(|r| (r.0, r.2));
        rows
    }

    /// Upstream `get_wmes_with_constant_values`: (parent, attribute, value) of the episode's
    /// constant rows, by row.
    pub(crate) fn constants_at(&self, memory: u64) -> Vec<(u64, u64, SymbolId)> {
        let in_episode = |ranges: &Vec<Range>| ranges.iter().any(|r| r.start <= memory && memory <= r.end);
        self.constants.iter().zip(&self.constant_ranges).filter(|(_, ranges)| in_episode(ranges)).map(|(c, _)| (c.parent, c.attr, c.value)).collect()
    }
}
