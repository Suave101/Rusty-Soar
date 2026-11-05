//! Rete match network, ported from upstream `Core/SoarKernel/src/decision_process/rete.cpp`.
//!
//! The node types, token tree, test semantics and match-set bookkeeping follow upstream
//! (Doorenbos' Rete as extended by Soar): positive joins, negated conditions, conjunctive
//! negations (CN / CN-partner nodes), production nodes with tentative assertions and
//! retractions, descendant-first right activation, and population of new nodes from the
//! matches above them.
//!
//! Upstream optimizations that only change speed, not which matches exist, are omitted:
//! alpha/beta hash tables, left/right unlinking and merged memory/positive (MP) nodes.
//! Because upstream iterates hash buckets, the order matches are reported in within one
//! elaboration wave can differ from upstream; the set of matches is the same.
//!
//! Storage is fixed-capacity pools sized by [`ReteCapacity`] and allocated once, with
//! upstream's pointer-linked lists (tokens of a node, children of a token, WMEs of an alpha
//! memory, blocks of a negation) as index-linked lists. Nothing is allocated while matching,
//! and every loop is bounded by a pool size, which keeps the network amenable to Kani. A full
//! pool stops the network with a [`CapacityError`].
//!
//! Two layout rules keep Kani proofs tractable. CBMC can lose track of an enum's tag, after
//! which every branch on it is explored both ways and loops can no longer be bounded:
//!
//! - Data-carrying enums the network matches on are `#[repr(u8)]`. Without an explicit tag,
//!   Rust stores the tag in spare values of a field (a `Vec`'s capacity, a fieldless enum),
//!   and CBMC cannot read such a tag as a constant once the bytes have been on the heap or
//!   copied through another enum.
//! - A data-carrying enum is not kept inside another enum's variant where anything branches
//!   on it in a loop. CBMC stores a variant's data as raw bytes, and the tag of an enum read
//!   back from them is no longer a constant, even after copying it out (copying helps only
//!   for comparisons). Struct fields are tracked exactly, so such values go in a struct with a
//!   fieldless kind, as the agent's `FlatAction` does.

use crate::pool::{BoundedVec, HeapSlots, Layout, Pool};
use crate::symbol::{SymbolId, SymbolTable};
use crate::wm::WmeKey;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Symbol field selector inside a Working Memory Element (WME) triple (Id ^Attr Val).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Identifier field of the WME triple.
    Id,
    /// Attribute field of the WME triple.
    Attr,
    /// Value field of the WME triple.
    Val,
}

impl Field {
    /// Extracts the selected symbol handle from a WME triple.
    pub fn extract(&self, triple: &(SymbolId, SymbolId, SymbolId)) -> SymbolId {
        match self {
            Field::Id => triple.0,
            Field::Attr => triple.1,
            Field::Val => triple.2,
        }
    }
}

/// Variable join constraint across condition boundaries in the Beta Network.
///
/// Shorthand for an equality [`ReteTest`] against an earlier condition; `token_wme_index` is
/// the 0-based index of that condition. An index equal to the current condition's index refers
/// to the WME being matched, as upstream's `levels_up == 0`.
#[derive(Debug, Clone)]
pub struct VariableBinding {
    /// Index of the WME in the historic token chain to compare against.
    pub token_wme_index: usize,
    /// Field in the historic token WME to test.
    pub token_field: Field,
    /// Field in the incoming Alpha WME to test.
    pub wme_field: Field,
}

/// Constant field filter test executed inside Alpha Memory nodes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AlphaTest {
    /// Optional identifier requirement.
    pub id: Option<SymbolId>,
    /// Optional attribute requirement.
    pub attr: Option<SymbolId>,
    /// Optional value requirement.
    pub val: Option<SymbolId>,
}

impl AlphaTest {
    /// Tests whether a WME triple matches the alpha filter criteria.
    pub fn matches(&self, s: SymbolId, a: SymbolId, v: SymbolId) -> bool {
        self.id.is_none_or(|id| id == s)
            && self.attr.is_none_or(|attr| attr == a)
            && self.val.is_none_or(|val| val == v)
    }

    /// Upstream's alpha hash table index (`table_for_tests`): bit 0 id, bit 1 attr, bit 2 value.
    fn table_index(&self) -> u8 {
        u8::from(self.id.is_some()) | u8::from(self.attr.is_some()) << 1 | u8::from(self.val.is_some()) << 2
    }
}

/// Where a variable was bound: the condition level and field.
///
/// Level 1 is the first condition; the level of the condition being matched refers to the WME
/// being matched. Upstream stores the same location relative to the current node (`levels_up`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarLocation {
    /// Condition level the variable was bound at.
    pub depth: usize,
    /// Field of that level's WME holding the binding.
    pub field: Field,
}

/// Comparison performed by a relational [`ReteTest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// `=` (symbol identity).
    Equal,
    /// `<>`
    NotEqual,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=`
    LessOrEqual,
    /// `>=`
    GreaterOrEqual,
    /// `<=>` (same symbol type).
    SameType,
    /// `$$`: equal to the referent, which isn't an identifier.
    ConstantMatch,
    /// `@`: both are identifiers linked to the same long-term identifier, or, against an
    /// integer constant, the field is linked to the long-term identifier with that number.
    SmemLink,
    /// `!@`: the negation of [`Relation::SmemLink`].
    SmemLinkNot,
}

/// Right-hand side of a relational test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Referent {
    /// A constant symbol.
    Constant(SymbolId),
    /// A variable bound earlier in the condition list.
    Variable(VarLocation),
}

/// A test performed at a join node in addition to the alpha memory's constant tests.
/// Mirrors upstream's `rete_test` types. Explicitly tagged for Kani (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum ReteTest {
    /// `field relation referent`, e.g. `^value > 5` or `^value <> <x>`.
    Relational {
        /// Field of the incoming WME.
        field: Field,
        /// Comparison to apply.
        relation: Relation,
        /// Value compared against.
        referent: Referent,
    },
    /// `<< a b c >>`: the field equals one of the listed constants.
    Disjunction {
        /// Field of the incoming WME.
        field: Field,
        /// Accepted constants.
        values: Vec<SymbolId>,
    },
    /// The WME's identifier is a goal (`state` keyword).
    IdIsGoal,
    /// The WME's identifier is an impasse (`impasse` keyword).
    IdIsImpasse,
    /// `@+` (`linked`) or `@-`: whether the field is linked to a long-term identifier.
    SmemLinked {
        /// Field of the incoming WME.
        field: Field,
        /// Whether the test is `@+`.
        linked: bool,
    },
}

/// Tests for one condition: alpha (constant equality) tests plus join tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionTests {
    /// Constant equality tests shared through an alpha memory.
    pub alpha: AlphaTest,
    /// Whether the condition tests an acceptable-preference WME (`^operator <o> +`).
    pub acceptable: bool,
    /// Remaining tests, evaluated at the join node.
    pub tests: Vec<ReteTest>,
}

/// One left-hand-side condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// A condition that must match a WME.
    Positive(ConditionTests),
    /// `-( ... )`: no WME may match.
    Negative(ConditionTests),
    /// `-{ ... }`: the conjunction of the subconditions must have no match.
    ConjunctiveNegation(Vec<Condition>),
}

/// A positive or negative condition's tests, with the join tests borrowed rather than owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatTests<'a> {
    /// Constant equality tests shared through an alpha memory.
    pub alpha: AlphaTest,
    /// Whether the condition tests an acceptable-preference WME (`^operator <o> +`).
    pub acceptable: bool,
    /// Remaining tests, evaluated at the join node.
    pub tests: &'a [ReteTest],
}

impl<'a> From<&'a ConditionTests> for FlatTests<'a> {
    fn from(tests: &'a ConditionTests) -> Self {
        FlatTests { alpha: tests.alpha, acceptable: tests.acceptable, tests: &tests.tests }
    }
}

/// One entry of a flattened condition list. A conjunctive negation is a marker followed by its
/// subconditions, so no type contains itself: Kani cannot bound the drop code of a recursive
/// type such as [`Condition`], so proofs build conditions in this form. The join tests are
/// borrowed so that proofs can keep them off the heap, where Kani cannot read a [`Referent`]
/// inside a [`ReteTest`] back as a constant. Explicitly tagged for Kani (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlatCondition<'a> {
    /// A condition that must match a WME.
    Positive(FlatTests<'a>),
    /// `-( ... )`: no WME may match.
    Negative(FlatTests<'a>),
    /// `-{ ... }` over the next `len` entries (nested markers and their entries included).
    ConjunctiveNegation { len: usize },
}

/// Flattens a condition list into the form [`ReteNetwork::add_production_flat`] takes.
pub fn flatten_conditions(conditions: &[Condition]) -> Vec<FlatCondition<'_>> {
    let mut flat = Vec::new();
    for condition in conditions {
        match condition {
            Condition::Positive(tests) => flat.push(FlatCondition::Positive(tests.into())),
            Condition::Negative(tests) => flat.push(FlatCondition::Negative(tests.into())),
            Condition::ConjunctiveNegation(sub) => {
                let sub = flatten_conditions(sub);
                flat.push(FlatCondition::ConjunctiveNegation { len: sub.len() });
                flat.extend(sub);
            }
        }
    }
    flat
}

impl Condition {
    /// Converts the legacy `(AlphaTest, bindings)` form; `index` is the 0-based condition index.
    pub fn from_bindings(alpha: AlphaTest, bindings: Vec<VariableBinding>) -> Self {
        Condition::Positive(ConditionTests {
            alpha,
            acceptable: false,
            tests: bindings
                .into_iter()
                .map(|binding| ReteTest::Relational {
                    field: binding.wme_field,
                    relation: Relation::Equal,
                    referent: Referent::Variable(VarLocation {
                        depth: binding.token_wme_index + 1,
                        field: binding.token_field,
                    }),
                })
                .collect(),
        })
    }
}

/// Recorded Working Memory entry with key tracking for clean retraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WmeRecord {
    /// Arena key handle for generational integrity.
    pub key: WmeKey,
    /// Raw symbol identifier tuple (Id, Attr, Val).
    pub triple: (SymbolId, SymbolId, SymbolId),
}

/// Index handle referencing an Alpha Memory node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AlphaMemoryId(
    /// Raw arena index.
    pub usize,
);

/// Index handle referencing a beta network node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(
    /// Raw arena index.
    pub usize,
);

/// Index handle referencing a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId(
    /// Raw arena index.
    pub usize,
);

/// Index handle referencing a registered Production rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductionId(
    /// Raw arena index.
    pub usize,
);

/// Handle for a fired match, used to report its retraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchId(
    /// Monotonic counter value.
    pub usize,
);


// ======================================================================
// Fixed-capacity storage
// ======================================================================

/// Most condition levels (including negated ones and conjunctive-negation subconditions) a
/// production may have.
pub const MAX_CONDITION_DEPTH: usize = 32;

type Triple = (SymbolId, SymbolId, SymbolId);

/// A Rete pool that ran out of room. The network stops changing once this happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityError {
    /// [`ReteCapacity::wmes`]
    Wmes,
    /// [`ReteCapacity::alpha_memories`]
    AlphaMemories,
    /// [`ReteCapacity::alpha_entries`]
    AlphaEntries,
    /// [`ReteCapacity::nodes`]
    Nodes,
    /// [`ReteCapacity::tests`]
    Tests,
    /// [`ReteCapacity::disjunction_values`]
    DisjunctionValues,
    /// [`ReteCapacity::tokens`]
    Tokens,
    /// [`ReteCapacity::blocks`]
    Blocks,
    /// [`ReteCapacity::productions`]
    Productions,
    /// [`ReteCapacity::matches`]
    Matches,
    /// A production deeper than [`MAX_CONDITION_DEPTH`].
    ConditionDepth,
    /// A [`FlatCondition::ConjunctiveNegation`] whose length runs past the end of its list.
    MalformedConditions,
}

impl CapacityError {
    /// The variant's name, as `Debug` prints it, without the formatting machinery (which Kani
    /// has to unroll on every path that might report an error).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Wmes => "Wmes",
            Self::AlphaMemories => "AlphaMemories",
            Self::AlphaEntries => "AlphaEntries",
            Self::Nodes => "Nodes",
            Self::Tests => "Tests",
            Self::DisjunctionValues => "DisjunctionValues",
            Self::Tokens => "Tokens",
            Self::Blocks => "Blocks",
            Self::Productions => "Productions",
            Self::Matches => "Matches",
            Self::ConditionDepth => "ConditionDepth",
            Self::MalformedConditions => "MalformedConditions",
        }
    }
}

/// Sizes of the Rete's pools, fixed when the network is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReteCapacity {
    /// WMEs in the network.
    pub wmes: usize,
    /// Alpha memories (distinct constant tests).
    pub alpha_memories: usize,
    /// WME entries across all alpha memories.
    pub alpha_entries: usize,
    /// Beta nodes, including the top node.
    pub nodes: usize,
    /// Join tests across all nodes.
    pub tests: usize,
    /// Constants across all disjunction tests.
    pub disjunction_values: usize,
    /// Tokens (partial and complete matches), including the top token.
    pub tokens: usize,
    /// Negation and conjunctive-negation blocks.
    pub blocks: usize,
    /// Productions.
    pub productions: usize,
    /// Matches pending assertion plus fired matches not yet retracted.
    pub matches: usize,
}

impl Default for ReteCapacity {
    fn default() -> Self {
        Self {
            wmes: 4096,
            alpha_memories: 1024,
            alpha_entries: 16384,
            nodes: 4096,
            tests: 8192,
            disjunction_values: 2048,
            tokens: 32768,
            blocks: 8192,
            productions: 1024,
            matches: 4096,
        }
    }
}

impl ReteCapacity {
    /// The smallest pools: four of everything. Loops over them need a Kani unwind bound of 5.
    pub const fn four() -> Self {
        Self {
            wmes: 4,
            alpha_memories: 4,
            alpha_entries: 4,
            nodes: 4,
            tests: 4,
            disjunction_values: 4,
            tokens: 4,
            blocks: 4,
            productions: 4,
            matches: 4,
        }
    }

    /// Small pools for bounded model checking.
    pub const fn tiny() -> Self {
        Self {
            wmes: 4,
            alpha_memories: 4,
            alpha_entries: 8,
            nodes: 8,
            tests: 4,
            disjunction_values: 2,
            tokens: 8,
            blocks: 4,
            productions: 2,
            matches: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    start: u32,
    len: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum StoredTest {
    Relational { field: Field, relation: Relation, referent: Referent },
    Disjunction { field: Field, values: Range },
    IdIsGoal,
    IdIsImpasse,
    SmemLinked { field: Field, linked: bool },
}

#[derive(Debug, Clone, Copy)]
struct AlphaMemory {
    test: AlphaTest,
    acceptable: bool,
    first_entry: Option<u32>,
    last_entry: Option<u32>,
    /// Positive and negative nodes to right-activate, descendants before ancestors.
    first_successor: Option<NodeId>,
}

#[derive(Debug, Clone, Copy)]
struct AlphaEntry {
    wme: WmeKey,
    next: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    /// Dummy top node; holds the single empty token.
    Top,
    Positive { alpha: AlphaMemoryId, tests: Range },
    Negative { alpha: AlphaMemoryId, tests: Range },
    Ncc { partner: NodeId },
    NccPartner { ncc: NodeId },
    Production(ProductionId),
}

#[derive(Debug, Clone, Copy)]
struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    /// Condition level of this node's tokens.
    depth: u32,
    first_token: Option<TokenId>,
    last_token: Option<TokenId>,
    /// For positive/negative nodes: nearest ancestor using the same alpha memory.
    nearest_ancestor_with_same_am: Option<NodeId>,
    next_from_alpha: Option<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocker {
    Wme(WmeKey),
    NccResult(TokenId),
}

#[derive(Debug, Clone, Copy)]
struct Block {
    blocked: TokenId,
    by: Blocker,
    next_in_token: Option<u32>,
    next_in_wme: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    node: NodeId,
    parent: Option<TokenId>,
    wme: Option<WmeKey>,
    first_child: Option<TokenId>,
    last_child: Option<TokenId>,
    prev_sibling: Option<TokenId>,
    next_sibling: Option<TokenId>,
    prev_in_node: Option<TokenId>,
    next_in_node: Option<TokenId>,
    prev_from_wme: Option<TokenId>,
    next_from_wme: Option<TokenId>,
    /// Negative and NCC tokens: what currently blocks the match (upstream `negrm_tokens`).
    first_block: Option<u32>,
    /// NCC-partner result tokens: the NCC token they block (upstream `a.neg.left_token`).
    owner: Option<TokenId>,
}

#[derive(Debug, Clone, Copy)]
struct ReteWme {
    record: WmeRecord,
    acceptable: bool,
    /// Tokens whose own WME is this one.
    first_token: Option<TokenId>,
    /// Blocks this WME makes on negative-node tokens.
    first_block: Option<u32>,
}

/// A complete match of a production, as reported to the agent.
#[derive(Debug, Clone)]
pub struct Instantiation {
    /// Match handle; a later retraction reports the same id.
    pub id: MatchId,
    /// Production that matched.
    pub production: ProductionId,
    /// Production rule identifier label.
    pub rule_name: &'static str,
    /// WMEs matched by the positive conditions, in condition order.
    pub matched_wmes: Vec<WmeRecord>,
    /// The WME at each condition level (`levels[d - 1]` for level `d`; `None` for negations),
    /// used to resolve RHS variable bindings.
    pub levels: Vec<Option<Triple>>,
}

/// A complete match that has not fired yet.
#[derive(Debug, Clone)]
pub struct PendingMatch {
    /// Production-node token identifying the match.
    pub token: TokenId,
    /// Production that matched.
    pub production: ProductionId,
    /// Production name.
    pub rule_name: &'static str,
    /// WMEs matched by the positive conditions, in condition order.
    pub matched_wmes: Vec<WmeRecord>,
    /// The WME at each condition level (`None` for negations).
    pub levels: Vec<Option<Triple>>,
}

/// The WME at each condition level of a match, in a fixed-size array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatchLevels {
    depth: usize,
    levels: [Option<(WmeKey, Triple)>; MAX_CONDITION_DEPTH],
}

impl MatchLevels {
    fn same_wmes(&self, other: &MatchLevels) -> bool {
        self.depth == other.depth
            && self.levels[..self.depth].iter().zip(&other.levels[..other.depth]).all(|(a, b)| a.map(|x| x.0) == b.map(|x| x.0))
    }

    fn matched_wmes(&self) -> Vec<WmeRecord> {
        self.levels[..self.depth].iter().flatten().map(|(key, triple)| WmeRecord { key: *key, triple: *triple }).collect()
    }

    fn triples(&self) -> Vec<Option<Triple>> {
        self.levels[..self.depth].iter().map(|l| l.map(|(_, t)| t)).collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct FiredMatch {
    id: MatchId,
    production: ProductionId,
    /// Production-node token currently supporting the match; `None` once retraction is pending.
    token: Option<TokenId>,
    levels: MatchLevels,
}

/// Index-based, deterministic RETE match network engine with fixed-capacity storage.
///
/// `S` chooses where the pools' slots live: [`HeapSlots`] (the default) for agents, or
/// [`InlineSlots`](crate::pool::InlineSlots) for Kani proofs, or a [`Layout`] choosing per
/// pool. The algorithm is the same.
#[derive(Debug)]
pub struct ReteNetwork<S: Layout = HeapSlots> {
    capacity: ReteCapacity,
    alpha_memories: BoundedVec<AlphaMemory, S::AlphaMemories>,
    alpha_entries: Pool<AlphaEntry, S::AlphaEntries>,
    nodes: BoundedVec<Node, S::Nodes>,
    tests: BoundedVec<StoredTest, S::Tests>,
    disjunction_values: BoundedVec<SymbolId, S::DisjunctionValues>,
    tokens: Pool<Token, S::Tokens>,
    blocks: Pool<Block, S::Blocks>,
    wmes: Pool<ReteWme, S::ReteWmes>,
    /// Registered production names and their production nodes.
    productions: BoundedVec<(&'static str, NodeId), S::Productions>,
    /// New matches not yet fired (upstream `tentative_assertions`), oldest first.
    assertions: BoundedVec<(ProductionId, TokenId), S::Matches>,
    /// Fired matches; those with `token == None` are pending retraction.
    fired: BoundedVec<FiredMatch, S::Matches>,
    next_match_id: usize,
    overflow: Option<CapacityError>,
}

impl Default for ReteNetwork {
    fn default() -> Self {
        Self::with_capacity(ReteCapacity::default())
    }
}

const TOP: NodeId = NodeId(0);
const TOP_TOKEN: TokenId = TokenId(0);

impl ReteNetwork {
    /// Creates a new, empty RETE match network with the default capacity.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S: Layout> ReteNetwork<S> {

    /// Creates an empty network whose pools have the given sizes.
    pub fn with_capacity(capacity: ReteCapacity) -> Self {
        let mut network = Self {
            capacity,
            alpha_memories: BoundedVec::with_capacity(capacity.alpha_memories),
            alpha_entries: Pool::with_capacity(capacity.alpha_entries),
            nodes: BoundedVec::with_capacity(capacity.nodes.max(1)),
            tests: BoundedVec::with_capacity(capacity.tests),
            disjunction_values: BoundedVec::with_capacity(capacity.disjunction_values),
            tokens: Pool::with_capacity(capacity.tokens.max(1)),
            blocks: Pool::with_capacity(capacity.blocks),
            wmes: Pool::with_capacity(capacity.wmes),
            productions: BoundedVec::with_capacity(capacity.productions),
            assertions: BoundedVec::with_capacity(capacity.matches),
            fired: BoundedVec::with_capacity(capacity.matches),
            next_match_id: 0,
            overflow: None,
        };
        let _ = network.nodes.push(Node {
            kind: NodeKind::Top,
            parent: None,
            first_child: None,
            next_sibling: None,
            depth: 0,
            first_token: Some(TOP_TOKEN),
            last_token: Some(TOP_TOKEN),
            nearest_ancestor_with_same_am: None,
            next_from_alpha: None,
        });
        let _ = network.tokens.insert(Token {
            node: TOP,
            parent: None,
            wme: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            prev_in_node: None,
            next_in_node: None,
            prev_from_wme: None,
            next_from_wme: None,
            first_block: None,
            owner: None,
        });
        network
    }

    /// The pool sizes this network was created with.
    pub fn capacity(&self) -> ReteCapacity {
        self.capacity
    }

    /// The pool that ran out of room, if any. Once set, the network no longer changes.
    pub fn overflow(&self) -> Option<CapacityError> {
        self.overflow
    }

    fn fail(&mut self, error: CapacityError) {
        self.overflow.get_or_insert(error);
    }

    fn result(&self) -> Result<(), CapacityError> {
        self.overflow.map_or(Ok(()), Err)
    }

    // ------------------------------------------------------------------
    // Production addition (upstream SECTIONS 8-9)
    // ------------------------------------------------------------------

    /// Adds a production from the legacy `(AlphaTest, bindings)` condition form.
    pub fn add_rule(
        &mut self,
        name: &'static str,
        conditions: Vec<(AlphaTest, Vec<VariableBinding>)>,
        symbols: &SymbolTable,
    ) -> Result<ProductionId, CapacityError> {
        let conditions = conditions
            .into_iter()
            .map(|(alpha, bindings)| Condition::from_bindings(alpha, bindings))
            .collect();
        self.add_production(name, conditions, symbols)
    }

    /// Adds a production, sharing existing nodes where tests are identical, and queues
    /// assertions for matches already present in working memory (upstream
    /// `add_production_to_rete`).
    pub fn add_production(
        &mut self,
        name: &'static str,
        conditions: Vec<Condition>,
        symbols: &SymbolTable,
    ) -> Result<ProductionId, CapacityError> {
        self.add_production_flat(name, &flatten_conditions(&conditions), symbols)
    }

    /// [`ReteNetwork::add_production`] for a flattened condition list.
    pub fn add_production_flat(
        &mut self,
        name: &'static str,
        conditions: &[FlatCondition],
        symbols: &SymbolTable,
    ) -> Result<ProductionId, CapacityError> {
        self.result()?;
        match condition_depth(conditions) {
            None => return Err(CapacityError::MalformedConditions),
            Some(depth) if depth > MAX_CONDITION_DEPTH => return Err(CapacityError::ConditionDepth),
            Some(_) => {}
        }
        let production = ProductionId(self.productions.len());
        let Some(bottom) = self.build_network_for_conditions(TOP, conditions, symbols) else {
            return Err(self.overflow.unwrap_or(CapacityError::Nodes));
        };
        let Some(p_node) = self.make_node(bottom, NodeKind::Production(production)) else {
            return Err(self.overflow.unwrap_or(CapacityError::Nodes));
        };
        if self.productions.push((name, p_node)).is_err() {
            self.fail(CapacityError::Productions);
            return Err(CapacityError::Productions);
        }
        self.update_node_with_matches_from_above(p_node, symbols);
        self.result().map(|_| production)
    }

    /// `conditions` has been checked by [`condition_depth`], so marker lengths are in range.
    fn build_network_for_conditions(&mut self, mut current: NodeId, conditions: &[FlatCondition], symbols: &SymbolTable) -> Option<NodeId> {
        let mut i = 0;
        while i < conditions.len() {
            // The alpha test is copied out before it is compared, for Kani (see the module docs).
            current = match &conditions[i] {
                FlatCondition::Positive(tests) => {
                    let alpha_test = tests.alpha;
                    let alpha = self.find_or_make_alpha_mem(&alpha_test, tests.acceptable)?;
                    self.share_or_make_node(current, alpha, tests.tests, true, symbols)?
                }
                FlatCondition::Negative(tests) => {
                    let alpha_test = tests.alpha;
                    let alpha = self.find_or_make_alpha_mem(&alpha_test, tests.acceptable)?;
                    self.share_or_make_node(current, alpha, tests.tests, false, symbols)?
                }
                FlatCondition::ConjunctiveNegation { len } => {
                    let subconditions = &conditions[i + 1..i + 1 + len];
                    i += len;
                    self.make_ncc_nodes(current, subconditions, symbols)?
                }
            };
            i += 1;
        }
        Some(current)
    }

    fn children(&self, node: NodeId) -> ChildIter<'_, S> {
        ChildIter { network: self, next: self.nodes[node.0].first_child }
    }

    fn same_tests(&self, range: Range, tests: &[ReteTest]) -> bool {
        range.len as usize == tests.len()
            && tests.iter().enumerate().all(|(i, test)| match (self.tests[range.start as usize + i], test) {
                (StoredTest::Relational { field, relation, referent }, ReteTest::Relational { field: f, relation: r, referent: x }) => {
                    // Copied before comparing, for Kani (see the module docs).
                    let (f, r, x) = (*f, *r, *x);
                    field == f && relation == r && referent == x
                }
                (StoredTest::Disjunction { field, values }, ReteTest::Disjunction { field: f, values: v }) => {
                    let f = *f;
                    field == f
                        && values.len as usize == v.len()
                        && v.iter().enumerate().all(|(j, s)| self.disjunction_values[values.start as usize + j] == *s)
                }
                (StoredTest::IdIsGoal, ReteTest::IdIsGoal) | (StoredTest::IdIsImpasse, ReteTest::IdIsImpasse) => true,
                (StoredTest::SmemLinked { field, linked }, ReteTest::SmemLinked { field: f, linked: l }) => {
                    let (f, l) = (*f, *l);
                    field == f && linked == l
                }
                _ => false,
            })
    }

    fn store_tests(&mut self, tests: &[ReteTest]) -> Option<Range> {
        let start = self.tests.len() as u32;
        for test in tests {
            let stored = match test {
                ReteTest::Relational { field, relation, referent } => StoredTest::Relational { field: *field, relation: *relation, referent: *referent },
                ReteTest::Disjunction { field, values } => {
                    let values_start = self.disjunction_values.len() as u32;
                    for value in values {
                        if self.disjunction_values.push(*value).is_err() {
                            self.fail(CapacityError::DisjunctionValues);
                            return None;
                        }
                    }
                    StoredTest::Disjunction { field: *field, values: Range { start: values_start, len: values.len() as u32 } }
                }
                ReteTest::IdIsGoal => StoredTest::IdIsGoal,
                ReteTest::IdIsImpasse => StoredTest::IdIsImpasse,
                ReteTest::SmemLinked { field, linked } => StoredTest::SmemLinked { field: *field, linked: *linked },
            };
            if self.tests.push(stored).is_err() {
                self.fail(CapacityError::Tests);
                return None;
            }
        }
        Some(Range { start, len: tests.len() as u32 })
    }

    fn share_or_make_node(
        &mut self,
        parent: NodeId,
        alpha: AlphaMemoryId,
        tests: &[ReteTest],
        positive: bool,
        symbols: &SymbolTable,
    ) -> Option<NodeId> {
        let existing = self.children(parent).find(|&child| match self.nodes[child.0].kind {
            NodeKind::Positive { alpha: a, tests: t } if positive => a == alpha && self.same_tests(t, tests),
            NodeKind::Negative { alpha: a, tests: t } if !positive => a == alpha && self.same_tests(t, tests),
            _ => false,
        });
        if existing.is_some() {
            return existing;
        }
        let tests = self.store_tests(tests)?;
        let kind = if positive { NodeKind::Positive { alpha, tests } } else { NodeKind::Negative { alpha, tests } };
        let node = self.make_node(parent, kind)?;
        self.update_node_with_matches_from_above(node, symbols);
        Some(node)
    }

    fn push_node(&mut self, node: Node) -> Option<NodeId> {
        let id = NodeId(self.nodes.len());
        if self.nodes.push(node).is_err() {
            self.fail(CapacityError::Nodes);
            return None;
        }
        Some(id)
    }

    fn append_child(&mut self, parent: NodeId, child: NodeId) {
        match self.children(parent).last() {
            Some(last) => self.nodes[last.0].next_sibling = Some(child),
            None => self.nodes[parent.0].first_child = Some(child),
        }
    }

    fn make_node(&mut self, parent: NodeId, kind: NodeKind) -> Option<NodeId> {
        let alpha = match kind {
            NodeKind::Positive { alpha, .. } | NodeKind::Negative { alpha, .. } => Some(alpha),
            _ => None,
        };
        let nearest_ancestor_with_same_am = alpha.and_then(|am| self.nearest_ancestor_with_same_am(parent, am));
        let id = self.push_node(Node {
            kind,
            parent: Some(parent),
            first_child: None,
            next_sibling: None,
            depth: self.nodes[parent.0].depth + 1,
            first_token: None,
            last_token: None,
            nearest_ancestor_with_same_am,
            next_from_alpha: None,
        })?;
        self.append_child(parent, id);
        if let Some(am) = alpha {
            self.relink_to_right_mem(id, am);
        }
        Some(id)
    }

    /// Upstream `make_new_cn_node`: builds the subnetwork, then the CN node beside it (after
    /// the subnetwork's top node in the parent's children) and the partner below it.
    fn make_ncc_nodes(&mut self, parent: NodeId, subconditions: &[FlatCondition], symbols: &SymbolTable) -> Option<NodeId> {
        let bottom = self.build_network_for_conditions(parent, subconditions, symbols)?;
        let ncc = NodeId(self.nodes.len());
        let partner = NodeId(self.nodes.len() + 1);
        let depth = self.nodes[parent.0].depth + 1;
        let blank = |kind, parent, depth| Node {
            kind,
            parent: Some(parent),
            first_child: None,
            next_sibling: None,
            depth,
            first_token: None,
            last_token: None,
            nearest_ancestor_with_same_am: None,
            next_from_alpha: None,
        };
        self.push_node(blank(NodeKind::Ncc { partner }, parent, depth))?;
        let bottom_depth = self.nodes[bottom.0].depth;
        self.push_node(blank(NodeKind::NccPartner { ncc }, bottom, bottom_depth))?;
        self.append_child(parent, ncc);
        self.append_child(bottom, partner);

        self.update_node_with_matches_from_above(partner, symbols);
        self.update_node_with_matches_from_above(ncc, symbols);
        Some(ncc)
    }

    fn nearest_ancestor_with_same_am(&self, mut node: NodeId, am: AlphaMemoryId) -> Option<NodeId> {
        loop {
            let current = &self.nodes[node.0];
            match current.kind {
                NodeKind::Positive { alpha, .. } | NodeKind::Negative { alpha, .. } if alpha == am => return Some(node),
                // Upstream skips over an NCC's subnetwork by jumping to the CN partner's parent.
                NodeKind::Ncc { partner } => node = self.nodes[partner.0].parent?,
                _ => node = current.parent?,
            }
        }
    }

    /// Upstream `relink_to_right_mem`: insert just before the nearest ancestor using the same
    /// alpha memory, else at the tail, so descendants are right-activated before ancestors.
    /// Activating an ancestor first would let a WME join with itself twice.
    fn relink_to_right_mem(&mut self, node: NodeId, am: AlphaMemoryId) {
        let ancestor = self.nodes[node.0].nearest_ancestor_with_same_am;
        let mut previous: Option<NodeId> = None;
        let mut current = self.alpha_memories[am.0].first_successor;
        while let Some(c) = current {
            if Some(c) == ancestor {
                break;
            }
            previous = Some(c);
            current = self.nodes[c.0].next_from_alpha;
        }
        // `current` is the ancestor (insert before it) or `None` (append at the tail).
        self.nodes[node.0].next_from_alpha = current;
        match previous {
            Some(p) => self.nodes[p.0].next_from_alpha = Some(node),
            None => self.alpha_memories[am.0].first_successor = Some(node),
        }
    }

    fn find_or_make_alpha_mem(&mut self, test: &AlphaTest, acceptable: bool) -> Option<AlphaMemoryId> {
        if let Some(index) = self.alpha_memories.iter().position(|am| am.test == *test && am.acceptable == acceptable) {
            return Some(AlphaMemoryId(index));
        }
        let am = AlphaMemoryId(self.alpha_memories.len());
        let memory = AlphaMemory { test: *test, acceptable, first_entry: None, last_entry: None, first_successor: None };
        if self.alpha_memories.push(memory).is_err() {
            self.fail(CapacityError::AlphaMemories);
            return None;
        }
        let existing: Vec<WmeKey> = self
            .wmes
            .iter()
            .filter(|(_, w)| w.acceptable == acceptable && test.matches(w.record.triple.0, w.record.triple.1, w.record.triple.2))
            .map(|(_, w)| w.record.key)
            .collect();
        for key in existing {
            self.append_alpha_entry(am, key)?;
        }
        Some(am)
    }

    fn append_alpha_entry(&mut self, am: AlphaMemoryId, wme: WmeKey) -> Option<()> {
        let Some(entry) = self.alpha_entries.insert(AlphaEntry { wme, next: None }) else {
            self.fail(CapacityError::AlphaEntries);
            return None;
        };
        let entry = entry as u32;
        match self.alpha_memories[am.0].last_entry {
            Some(last) => {
                if let Some(e) = self.alpha_entries.get_mut(last as usize) {
                    e.next = Some(entry);
                }
            }
            None => self.alpha_memories[am.0].first_entry = Some(entry),
        }
        self.alpha_memories[am.0].last_entry = Some(entry);
        Some(())
    }

    /// Upstream `update_node_with_matches_from_above`: left-activates a new node with every
    /// match its parent passes down.
    fn update_node_with_matches_from_above(&mut self, node: NodeId, symbols: &SymbolTable) {
        let Some(parent) = self.nodes[node.0].parent else { return };
        let mut current = self.nodes[parent.0].first_token;
        while let Some(token) = current {
            current = self.tokens.get(token.0).and_then(|t| t.next_in_node);
            if self.tokens.get(token.0).is_some_and(|t| t.first_block.is_none()) {
                self.left_activate(node, token, symbols);
            }
        }
    }

    // ------------------------------------------------------------------
    // Working memory changes (upstream SECTION 3)
    // ------------------------------------------------------------------

    /// Adds a WME to the Rete (upstream `add_wme_to_rete`).
    pub fn add_wme(
        &mut self,
        key: WmeKey,
        triple: Triple,
        acceptable: bool,
        symbols: &SymbolTable,
    ) -> Result<(), CapacityError> {
        self.result()?;
        let record = WmeRecord { key, triple };
        if self.wmes.insert(ReteWme { record, acceptable, first_token: None, first_block: None }).is_none() {
            self.fail(CapacityError::Wmes);
            return self.result();
        }

        // Upstream probes its 8 alpha hash tables (per acceptable flag) in table order; at most
        // one alpha memory per table can match.
        for table in 0..8u8 {
            let Some(am) = self
                .alpha_memories
                .iter()
                .position(|am| am.acceptable == acceptable && am.test.table_index() == table && am.test.matches(triple.0, triple.1, triple.2))
            else {
                continue;
            };
            let am = AlphaMemoryId(am);
            // Insert into the memory before activating its successors, as upstream does.
            if self.append_alpha_entry(am, key).is_none() {
                return self.result();
            }
            let mut successor = self.alpha_memories[am.0].first_successor;
            while let Some(node) = successor {
                successor = self.nodes[node.0].next_from_alpha;
                self.right_activate(node, key, symbols);
            }
        }
        self.result()
    }

    /// Removes a WME from the Rete (upstream `remove_wme_from_rete`).
    pub fn remove_wme(&mut self, key: WmeKey, symbols: &SymbolTable) {
        let Some(index) = self.wme_index(key) else { return };
        for am in 0..self.alpha_memories.len() {
            self.remove_alpha_entry(AlphaMemoryId(am), key);
        }

        // Tree-based removal of every token built on this WME.
        while let Some(token) = self.wmes.get(index).and_then(|w| w.first_token) {
            self.remove_token_and_subtree(token, symbols);
        }

        // Negative-node tokens it was blocking: unblock, and pass on when no blocker remains.
        while let Some(block) = self.wmes.get(index).and_then(|w| w.first_block) {
            let Some(b) = self.blocks.get(block as usize).copied() else { break };
            if let Some(w) = self.wmes.get_mut(index) {
                w.first_block = b.next_in_wme;
            }
            self.unlink_block_from_token(b.blocked, block);
            self.blocks.remove(block as usize);
            let unblocked = self.tokens.get(b.blocked.0).is_some_and(|t| t.first_block.is_none());
            if unblocked {
                let node = self.tokens.get(b.blocked.0).map_or(TOP, |t| t.node);
                self.left_activate_children(node, b.blocked, symbols);
            }
        }

        self.wmes.remove(index);
    }

    fn remove_alpha_entry(&mut self, am: AlphaMemoryId, key: WmeKey) {
        let mut previous: Option<u32> = None;
        let mut current = self.alpha_memories[am.0].first_entry;
        while let Some(c) = current {
            let Some(entry) = self.alpha_entries.get(c as usize).copied() else { return };
            if entry.wme == key {
                match previous {
                    Some(p) => {
                        if let Some(e) = self.alpha_entries.get_mut(p as usize) {
                            e.next = entry.next;
                        }
                    }
                    None => self.alpha_memories[am.0].first_entry = entry.next,
                }
                if self.alpha_memories[am.0].last_entry == Some(c) {
                    self.alpha_memories[am.0].last_entry = previous;
                }
                self.alpha_entries.remove(c as usize);
                return;
            }
            previous = Some(c);
            current = entry.next;
        }
    }

    // ------------------------------------------------------------------
    // Node activation (upstream SECTIONS 12-15)
    // ------------------------------------------------------------------

    fn right_activate(&mut self, node: NodeId, wme: WmeKey, symbols: &SymbolTable) {
        match self.nodes[node.0].kind {
            NodeKind::Positive { tests, .. } => {
                let parent = self.nodes[node.0].parent.unwrap_or(TOP);
                let mut current = self.nodes[parent.0].first_token;
                while let Some(token) = current {
                    current = self.tokens.get(token.0).and_then(|t| t.next_in_node);
                    let passes_down = self.tokens.get(token.0).is_some_and(|t| t.first_block.is_none());
                    if passes_down && self.passes_tests(tests, node, token, wme, symbols) {
                        self.make_positive_token(node, token, wme, symbols);
                    }
                }
            }
            NodeKind::Negative { tests, .. } => {
                let mut current = self.nodes[node.0].first_token;
                while let Some(token) = current {
                    current = self.tokens.get(token.0).and_then(|t| t.next_in_node);
                    let parent = self.tokens.get(token.0).and_then(|t| t.parent).unwrap_or(TOP_TOKEN);
                    if self.passes_tests(tests, node, parent, wme, symbols) {
                        self.add_block(token, Blocker::Wme(wme));
                        while let Some(child) = self.tokens.get(token.0).and_then(|t| t.first_child) {
                            self.remove_token_and_subtree(child, symbols);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn left_activate_children(&mut self, node: NodeId, token: TokenId, symbols: &SymbolTable) {
        let mut child = self.nodes[node.0].first_child;
        while let Some(c) = child {
            child = self.nodes[c.0].next_sibling;
            self.left_activate(c, token, symbols);
        }
    }

    /// Left activation of `node` with a match `parent_token` from its parent node.
    fn left_activate(&mut self, node: NodeId, parent_token: TokenId, symbols: &SymbolTable) {
        if self.overflow.is_some() {
            return;
        }
        match self.nodes[node.0].kind {
            NodeKind::Top => {}
            NodeKind::Positive { alpha, tests } => {
                let mut entries = self.alpha_memories[alpha.0].first_entry;
                while let Some(e) = entries {
                    let Some(entry) = self.alpha_entries.get(e as usize).copied() else { break };
                    entries = entry.next;
                    if self.passes_tests(tests, node, parent_token, entry.wme, symbols) {
                        self.make_positive_token(node, parent_token, entry.wme, symbols);
                    }
                }
            }
            NodeKind::Negative { alpha, tests } => {
                let Some(token) = self.new_token(node, Some(parent_token), None) else { return };
                let mut entries = self.alpha_memories[alpha.0].first_entry;
                while let Some(e) = entries {
                    let Some(entry) = self.alpha_entries.get(e as usize).copied() else { break };
                    entries = entry.next;
                    if self.passes_tests(tests, node, parent_token, entry.wme, symbols) {
                        self.add_block(token, Blocker::Wme(entry.wme));
                    }
                }
                if self.tokens.get(token.0).is_some_and(|t| t.first_block.is_none()) {
                    self.left_activate_children(node, token, symbols);
                }
            }
            NodeKind::Ncc { .. } => {
                // The partner may have created this token already, in which case it's blocked.
                if self.find_child_token(node, parent_token).is_some() {
                    return;
                }
                let Some(token) = self.new_token(node, Some(parent_token), None) else { return };
                self.left_activate_children(node, token, symbols);
            }
            NodeKind::NccPartner { ncc } => {
                let Some(result) = self.new_token(node, Some(parent_token), None) else { return };
                // Walk up from the subnetwork's bottom token to the NCC's parent level.
                let owner_depth = self.nodes[ncc.0].depth - 1;
                let mut owner_parent = parent_token;
                while self.nodes[self.tokens.get(owner_parent.0).map_or(TOP, |t| t.node).0].depth > owner_depth {
                    owner_parent = self.tokens.get(owner_parent.0).and_then(|t| t.parent).unwrap_or(TOP_TOKEN);
                }
                let owner = match self.find_child_token(ncc, owner_parent) {
                    Some(owner) => owner,
                    None => match self.new_token(ncc, Some(owner_parent), None) {
                        Some(owner) => owner,
                        None => return,
                    },
                };
                if let Some(t) = self.tokens.get_mut(result.0) {
                    t.owner = Some(owner);
                }
                self.add_block(owner, Blocker::NccResult(result));
                while let Some(child) = self.tokens.get(owner.0).and_then(|t| t.first_child) {
                    self.remove_token_and_subtree(child, symbols);
                }
            }
            NodeKind::Production(production) => {
                let Some(token) = self.new_token(node, Some(parent_token), None) else { return };
                self.p_node_left_addition(production, token);
            }
        }
    }

    fn make_positive_token(&mut self, node: NodeId, parent: TokenId, wme: WmeKey, symbols: &SymbolTable) {
        if let Some(token) = self.new_token(node, Some(parent), Some(wme)) {
            self.left_activate_children(node, token, symbols);
        }
    }

    fn add_block(&mut self, token: TokenId, by: Blocker) {
        let wme_index = match by {
            Blocker::Wme(w) => self.wme_index(w),
            Blocker::NccResult(_) => None,
        };
        let next_in_token = self.tokens.get(token.0).and_then(|t| t.first_block);
        let next_in_wme = wme_index.and_then(|i| self.wmes.get(i)).and_then(|w| w.first_block);
        let Some(block) = self.blocks.insert(Block { blocked: token, by, next_in_token, next_in_wme }) else {
            self.fail(CapacityError::Blocks);
            return;
        };
        if let Some(t) = self.tokens.get_mut(token.0) {
            t.first_block = Some(block as u32);
        }
        if let Some(w) = wme_index.and_then(|i| self.wmes.get_mut(i)) {
            w.first_block = Some(block as u32);
        }
    }

    fn find_child_token(&self, node: NodeId, parent: TokenId) -> Option<TokenId> {
        let mut current = self.nodes[node.0].first_token;
        while let Some(t) = current {
            let token = self.tokens.get(t.0)?;
            if token.parent == Some(parent) {
                return Some(t);
            }
            current = token.next_in_node;
        }
        None
    }
}

impl<S: Layout> ReteNetwork<S> {
    // ------------------------------------------------------------------
    // Match set changes (upstream SECTION 2 and 15)
    // ------------------------------------------------------------------

    /// Upstream `p_node_left_addition`: a new match either cancels a pending retraction of
    /// the same match (same WMEs), or becomes a tentative assertion.
    fn p_node_left_addition(&mut self, production: ProductionId, token: TokenId) {
        let levels = self.match_levels(token);
        if let Some(fired) = self
            .fired
            .iter_mut()
            .find(|f| f.production == production && f.token.is_none() && f.levels.same_wmes(&levels))
        {
            fired.token = Some(token);
            return;
        }
        if self.assertions.push((production, token)).is_err() {
            self.fail(CapacityError::Matches);
        }
    }

    /// Upstream `p_node_left_removal`: an unfired match is dropped; a fired one is retracted.
    fn p_node_left_removal(&mut self, production: ProductionId, token: TokenId) {
        if let Some(index) = self.assertions.iter().position(|(_, t)| *t == token) {
            self.assertions.remove(index);
            return;
        }
        if let Some(fired) = self.fired.iter_mut().find(|f| f.production == production && f.token == Some(token)) {
            fired.token = None;
        }
    }

    /// Returns whether any assertions or retractions are pending
    /// (upstream `any_assertions_or_retractions_ready`).
    pub fn any_assertions_or_retractions_ready(&self) -> bool {
        !self.assertions.is_empty() || self.fired.iter().any(|f| f.token.is_none())
    }

    /// Number of new matches waiting to fire.
    pub fn pending_assertions(&self) -> usize {
        self.assertions.len()
    }

    /// Matches not yet fired, newest first (upstream keeps its assertion lists newest first).
    pub fn pending_matches(&self) -> Vec<PendingMatch> {
        self.assertions
            .iter()
            .rev()
            .map(|&(production, token)| {
                let levels = self.match_levels(token);
                PendingMatch {
                    token,
                    production,
                    rule_name: self.productions[production.0].0,
                    matched_wmes: levels.matched_wmes(),
                    levels: levels.triples(),
                }
            })
            .collect()
    }

    /// Fires one pending match: it becomes an instantiation whose retraction is reported later.
    pub fn fire_match(&mut self, token: TokenId) -> Option<Instantiation> {
        let index = self.assertions.iter().position(|(_, t)| *t == token)?;
        let (production, token) = self.assertions.remove(index);
        let id = MatchId(self.next_match_id);
        self.next_match_id += 1;
        let levels = self.match_levels(token);
        if self.fired.push(FiredMatch { id, production, token: Some(token), levels }).is_err() {
            self.fail(CapacityError::Matches);
        }
        Some(Instantiation {
            id,
            production,
            rule_name: self.productions[production.0].0,
            matched_wmes: levels.matched_wmes(),
            levels: levels.triples(),
        })
    }

    /// Fires every pending assertion: returns the new instantiations and records them so
    /// their retraction is reported later.
    pub fn take_assertions(&mut self) -> Vec<Instantiation> {
        let tokens: Vec<TokenId> = self.assertions.iter().map(|(_, t)| *t).collect();
        tokens.into_iter().filter_map(|t| self.fire_match(t)).collect()
    }

    /// Fired matches that no longer hold and await retraction.
    pub fn retracting_matches(&self) -> Vec<MatchId> {
        self.fired.iter().filter(|f| f.token.is_none()).map(|f| f.id).collect()
    }

    /// Retracts one fired match that no longer holds (upstream `get_next_retraction`).
    pub fn take_retraction(&mut self, id: MatchId) -> Option<Instantiation> {
        let index = self.fired.iter().position(|f| f.id == id && f.token.is_none())?;
        let fired = self.fired.remove(index);
        Some(Instantiation {
            id: fired.id,
            production: fired.production,
            rule_name: self.productions[fired.production.0].0,
            matched_wmes: fired.levels.matched_wmes(),
            levels: fired.levels.triples(),
        })
    }

    /// Returns fired matches that no longer hold (upstream `get_next_retraction`).
    pub fn take_retractions(&mut self) -> Vec<Instantiation> {
        self.retracting_matches().into_iter().filter_map(|id| self.take_retraction(id)).collect()
    }

    /// Drops all pending assertions and fired-match records, e.g. on agent reinitialization.
    pub fn clear_instantiations(&mut self) {
        self.assertions.clear();
        self.fired.clear();
    }

    // ------------------------------------------------------------------
    // Token removal (upstream `remove_token_and_subtree`)
    // ------------------------------------------------------------------

    fn remove_token_and_subtree(&mut self, token: TokenId, symbols: &SymbolTable) {
        while let Some(child) = self.tokens.get(token.0).and_then(|t| t.first_child) {
            self.remove_token_and_subtree(child, symbols);
        }
        let Some(t) = self.free_token(token) else { return };

        match self.nodes[t.node.0].kind {
            NodeKind::Production(production) => self.p_node_left_removal(production, token),
            NodeKind::Negative { .. } => {
                let mut block = t.first_block;
                while let Some(b) = block {
                    let Some(entry) = self.blocks.get(b as usize).copied() else { break };
                    block = entry.next_in_token;
                    if let Blocker::Wme(wme) = entry.by {
                        self.unlink_block_from_wme(wme, b);
                    }
                    self.blocks.remove(b as usize);
                }
            }
            NodeKind::Ncc { .. } => {
                // The NCC's results go with it (upstream frees its negrm tokens).
                let mut block = t.first_block;
                while let Some(b) = block {
                    let Some(entry) = self.blocks.remove(b as usize) else { break };
                    block = entry.next_in_token;
                    if let Blocker::NccResult(result) = entry.by {
                        self.free_token(result);
                    }
                }
            }
            NodeKind::NccPartner { ncc } => {
                if let Some(owner) = t.owner.filter(|o| self.tokens.get(o.0).is_some()) {
                    let block = self.block_list(owner).find(|&b| self.blocks.get(b as usize).is_some_and(|e| e.by == Blocker::NccResult(token)));
                    if let Some(b) = block {
                        self.unlink_block_from_token(owner, b);
                        self.blocks.remove(b as usize);
                    }
                    if self.tokens.get(owner.0).is_some_and(|o| o.first_block.is_none()) {
                        self.left_activate_children(ncc, owner, symbols);
                    }
                }
            }
            NodeKind::Top | NodeKind::Positive { .. } => {}
        }
    }

    fn block_list(&self, token: TokenId) -> BlockIter<'_, S> {
        BlockIter { network: self, next: self.tokens.get(token.0).and_then(|t| t.first_block) }
    }

    /// Unlinks a block (still in the pool) from its blocked token's list.
    fn unlink_block_from_token(&mut self, token: TokenId, block: u32) {
        let after = self.blocks.get(block as usize).and_then(|b| b.next_in_token);
        let mut previous: Option<u32> = None;
        let mut current = self.tokens.get(token.0).and_then(|t| t.first_block);
        while let Some(c) = current {
            if c == block {
                match previous {
                    Some(p) => {
                        if let Some(b) = self.blocks.get_mut(p as usize) {
                            b.next_in_token = after;
                        }
                    }
                    None => {
                        if let Some(t) = self.tokens.get_mut(token.0) {
                            t.first_block = after;
                        }
                    }
                }
                return;
            }
            previous = Some(c);
            current = self.blocks.get(c as usize).and_then(|b| b.next_in_token);
        }
    }

    /// Unlinks a block (still in the pool) from its WME's list.
    fn unlink_block_from_wme(&mut self, wme: WmeKey, block: u32) {
        let Some(index) = self.wme_index(wme) else { return };
        let after = self.blocks.get(block as usize).and_then(|b| b.next_in_wme);
        let mut previous: Option<u32> = None;
        let mut current = self.wmes.get(index).and_then(|w| w.first_block);
        while let Some(c) = current {
            if c == block {
                match previous {
                    Some(p) => {
                        if let Some(b) = self.blocks.get_mut(p as usize) {
                            b.next_in_wme = after;
                        }
                    }
                    None => {
                        if let Some(w) = self.wmes.get_mut(index) {
                            w.first_block = after;
                        }
                    }
                }
                return;
            }
            previous = Some(c);
            current = self.blocks.get(c as usize).and_then(|b| b.next_in_wme);
        }
    }

    // ------------------------------------------------------------------
    // Test evaluation (upstream SECTION 11)
    // ------------------------------------------------------------------

    /// Evaluates join tests for `wme` arriving at `node`, whose parent match is `parent_token`.
    fn passes_tests(&self, tests: Range, node: NodeId, parent_token: TokenId, wme: WmeKey, symbols: &SymbolTable) -> bool {
        let Some(index) = self.wme_index(wme) else { return false };
        let triple = self.wmes.get(index).map_or((SymbolId(0), SymbolId(0), SymbolId(0)), |w| w.record.triple);
        let depth = self.nodes[node.0].depth as usize;
        (0..tests.len as usize).all(|i| match self.tests[tests.start as usize + i] {
            StoredTest::Relational { field, relation, referent } => {
                let left = field.extract(&triple);
                let right = match referent {
                    Referent::Constant(symbol) => symbol,
                    Referent::Variable(location) if location.depth == depth => location.field.extract(&triple),
                    Referent::Variable(location) => match self.wme_at_depth(parent_token, location.depth) {
                        Some(triple) => location.field.extract(&triple),
                        None => return false,
                    },
                };
                match relation {
                    Relation::Equal => left == right,
                    Relation::NotEqual => left != right,
                    Relation::Less => symbols.compare(left, right) == Ordering::Less,
                    Relation::Greater => symbols.compare(left, right) == Ordering::Greater,
                    Relation::LessOrEqual => symbols.compare(left, right) != Ordering::Greater,
                    Relation::GreaterOrEqual => symbols.compare(left, right) != Ordering::Less,
                    Relation::SameType => symbols.same_type(left, right),
                    Relation::ConstantMatch => left == right && !symbols.is_identifier(left),
                    Relation::SmemLink | Relation::SmemLinkNot => {
                        let lti = symbols.lti(left);
                        // Against a constant, upstream compares with the integer's value;
                        // against a variable, with the other identifier's link.
                        let same = lti != 0
                            && match referent {
                                Referent::Constant(_) => symbols.integer(right) == Some(lti as i64),
                                Referent::Variable(_) => symbols.lti(right) == lti,
                            };
                        same == (relation == Relation::SmemLink)
                    }
                }
            }
            StoredTest::Disjunction { field, values } => {
                let value = field.extract(&triple);
                (0..values.len as usize).any(|j| self.disjunction_values[values.start as usize + j] == value)
            }
            StoredTest::IdIsGoal => symbols.is_goal(triple.0),
            StoredTest::IdIsImpasse => symbols.is_impasse(triple.0),
            StoredTest::SmemLinked { field, linked } => (symbols.lti(field.extract(&triple)) != 0) == linked,
        })
    }

    /// The WME matched at condition level `depth`, walking up from `token`.
    fn wme_at_depth(&self, mut token: TokenId, depth: usize) -> Option<Triple> {
        loop {
            let t = self.tokens.get(token.0)?;
            let node = &self.nodes[t.node.0];
            let node_depth = node.depth as usize;
            if node_depth == depth && !matches!(node.kind, NodeKind::NccPartner { .. }) {
                let wme = t.wme?;
                return self.wme_index(wme).and_then(|i| self.wmes.get(i)).map(|w| w.record.triple);
            }
            if node_depth < depth {
                return None;
            }
            token = t.parent?;
        }
    }

    /// WME per condition level for the match ending at a production-node token.
    fn match_levels(&self, token: TokenId) -> MatchLevels {
        let mut levels = MatchLevels { depth: 0, levels: [None; MAX_CONDITION_DEPTH] };
        let mut current = self.tokens.get(token.0).and_then(|t| t.parent);
        while let Some(id) = current {
            let Some(t) = self.tokens.get(id.0) else { break };
            if t.node == TOP {
                break;
            }
            let depth = self.nodes[t.node.0].depth as usize;
            if depth == 0 || depth > MAX_CONDITION_DEPTH {
                break;
            }
            levels.depth = levels.depth.max(depth);
            levels.levels[depth - 1] =
                t.wme.and_then(|w| self.wme_index(w).and_then(|i| self.wmes.get(i)).map(|r| (w, r.record.triple)));
            current = t.parent;
        }
        levels
    }

    // ------------------------------------------------------------------
    // Arena helpers
    // ------------------------------------------------------------------

    fn new_token(&mut self, node: NodeId, parent: Option<TokenId>, wme: Option<WmeKey>) -> Option<TokenId> {
        let wme_index = wme.and_then(|w| self.wme_index(w));
        let prev_in_node = self.nodes[node.0].last_token;
        let prev_sibling = parent.and_then(|p| self.tokens.get(p.0)).and_then(|p| p.last_child);
        let next_from_wme = wme_index.and_then(|i| self.wmes.get(i)).and_then(|w| w.first_token);
        let token = Token {
            node,
            parent,
            wme,
            first_child: None,
            last_child: None,
            prev_sibling,
            next_sibling: None,
            prev_in_node,
            next_in_node: None,
            prev_from_wme: None,
            next_from_wme,
            first_block: None,
            owner: None,
        };
        let Some(index) = self.tokens.insert(token) else {
            self.fail(CapacityError::Tokens);
            return None;
        };
        let id = TokenId(index);
        // Append to the node's token list.
        match prev_in_node.and_then(|p| self.tokens.get_mut(p.0)) {
            Some(p) => p.next_in_node = Some(id),
            None => self.nodes[node.0].first_token = Some(id),
        }
        self.nodes[node.0].last_token = Some(id);
        // Append to the parent's children.
        if let Some(parent) = parent {
            match prev_sibling.and_then(|s| self.tokens.get_mut(s.0)) {
                Some(s) => s.next_sibling = Some(id),
                None => {
                    if let Some(p) = self.tokens.get_mut(parent.0) {
                        p.first_child = Some(id);
                    }
                }
            }
            if let Some(p) = self.tokens.get_mut(parent.0) {
                p.last_child = Some(id);
            }
        }
        // Push onto the WME's token list.
        if let Some(i) = wme_index {
            if let Some(next) = next_from_wme.and_then(|n| self.tokens.get_mut(n.0)) {
                next.prev_from_wme = Some(id);
            }
            if let Some(w) = self.wmes.get_mut(i) {
                w.first_token = Some(id);
            }
        }
        Some(id)
    }

    /// Unlinks a token from its node, parent and WME and frees its slot.
    fn free_token(&mut self, token: TokenId) -> Option<Token> {
        let t = self.tokens.remove(token.0)?;
        match t.prev_in_node.and_then(|p| self.tokens.get_mut(p.0)) {
            Some(p) => p.next_in_node = t.next_in_node,
            None => self.nodes[t.node.0].first_token = t.next_in_node,
        }
        match t.next_in_node.and_then(|n| self.tokens.get_mut(n.0)) {
            Some(n) => n.prev_in_node = t.prev_in_node,
            None => self.nodes[t.node.0].last_token = t.prev_in_node,
        }
        if let Some(parent) = t.parent {
            match t.prev_sibling.and_then(|s| self.tokens.get_mut(s.0)) {
                Some(s) => s.next_sibling = t.next_sibling,
                None => {
                    if let Some(p) = self.tokens.get_mut(parent.0) {
                        p.first_child = t.next_sibling;
                    }
                }
            }
            match t.next_sibling.and_then(|s| self.tokens.get_mut(s.0)) {
                Some(s) => s.prev_sibling = t.prev_sibling,
                None => {
                    if let Some(p) = self.tokens.get_mut(parent.0) {
                        p.last_child = t.prev_sibling;
                    }
                }
            }
        }
        if let Some(wme) = t.wme {
            match t.prev_from_wme.and_then(|p| self.tokens.get_mut(p.0)) {
                Some(p) => p.next_from_wme = t.next_from_wme,
                None => {
                    if let Some(w) = self.wme_index(wme).and_then(|i| self.wmes.get_mut(i)) {
                        w.first_token = t.next_from_wme;
                    }
                }
            }
            if let Some(n) = t.next_from_wme.and_then(|n| self.tokens.get_mut(n.0)) {
                n.prev_from_wme = t.prev_from_wme;
            }
        }
        Some(t)
    }

    fn wme_index(&self, key: WmeKey) -> Option<usize> {
        self.wmes.iter().find(|(_, w)| w.record.key == key).map(|(i, _)| i)
    }
}

/// Deepest condition level a flat condition list reaches, or `None` when a conjunctive
/// negation's length runs past the end of its list.
fn condition_depth(conditions: &[FlatCondition]) -> Option<usize> {
    let mut deepest = 0;
    let mut level = 0;
    let mut i = 0;
    while i < conditions.len() {
        level += 1;
        match conditions[i] {
            FlatCondition::ConjunctiveNegation { len } => {
                let end = i.checked_add(1)?.checked_add(len)?;
                let sub = conditions.get(i + 1..end)?;
                // A conjunctive negation's subconditions start at its own level.
                deepest = deepest.max(level - 1 + condition_depth(sub)?.max(1));
                i = end;
            }
            _ => {
                deepest = deepest.max(level);
                i += 1;
            }
        }
    }
    Some(deepest)
}

struct ChildIter<'a, S: Layout> {
    network: &'a ReteNetwork<S>,
    next: Option<NodeId>,
}

impl<S: Layout> Iterator for ChildIter<'_, S> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.network.nodes[current.0].next_sibling;
        Some(current)
    }
}

struct BlockIter<'a, S: Layout> {
    network: &'a ReteNetwork<S>,
    next: Option<u32>,
}

impl<S: Layout> Iterator for BlockIter<'_, S> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let current = self.next?;
        self.next = self.network.blocks.get(current as usize).and_then(|b| b.next_in_token);
        Some(current)
    }
}
