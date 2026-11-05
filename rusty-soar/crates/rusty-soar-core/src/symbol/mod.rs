use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// O(1) Copy handle representing a string or identifier in Working Memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// Symbol classification type.
///
/// `repr(u8)` gives the enum an explicit tag byte. Without it, Rust keeps the tag in spare
/// values of a `Vec`/`String` field, and CBMC cannot read such a tag back from heap memory as
/// a constant, which leaves Kani unable to bound the loops that depend on it.
#[derive(Debug, Clone, PartialEq)]
#[repr(u8)]
pub enum SymbolData {
    /// Symbolic string identifier (e.g., "^sensor-status", "offline")
    String(String),
    /// Integer scalar value
    Integer(i64),
    /// Floating-point scalar value
    Float(f64),
    /// State or Goal Identifier (e.g., S1, O3)
    Identifier {
        /// Letter prefix representing symbol class (e.g., 'S' for State, 'O' for Operator)
        prefix: char,
        /// Monotonically assigned numeric index for uniqueness
        number: u64,
    },
}

/// The kind of a symbol: which [`SymbolData`] variant it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolKind {
    /// [`SymbolData::String`]
    String,
    /// [`SymbolData::Integer`]
    Integer,
    /// [`SymbolData::Float`]
    Float,
    /// [`SymbolData::Identifier`]
    Identifier,
}

/// A symbol as a flat record: its kind and, except for a string's text, its value. Fields its
/// kind does not use are zero.
#[derive(Debug, Clone, Copy)]
struct Entry {
    kind: SymbolKind,
    integer: i64,
    float: f64,
    prefix: char,
    number: u64,
    /// For an identifier, upstream `id->LTI_ID`: the long-term identifier it is linked to in
    /// semantic memory, 0 for none.
    lti: u64,
}

impl Entry {
    fn of(data: &SymbolData) -> Self {
        let blank = Self { kind: SymbolKind::String, integer: 0, float: 0.0, prefix: '\0', number: 0, lti: 0 };
        match data {
            SymbolData::String(_) => blank,
            SymbolData::Integer(integer) => Self { kind: SymbolKind::Integer, integer: *integer, ..blank },
            SymbolData::Float(float) => Self { kind: SymbolKind::Float, float: *float, ..blank },
            SymbolData::Identifier { prefix, number } => {
                Self { kind: SymbolKind::Identifier, prefix: *prefix, number: *number, ..blank }
            }
        }
    }

    /// Whether two symbols that are not strings are equal, as `SymbolData`'s `==` decides.
    fn same_value(&self, other: &Self) -> bool {
        self.kind == other.kind
            && match self.kind {
                SymbolKind::String => false,
                SymbolKind::Integer => self.integer == other.integer,
                SymbolKind::Float => self.float == other.float,
                SymbolKind::Identifier => self.prefix == other.prefix && self.number == other.number,
            }
    }
}

macro_rules! predefined_symbols {
    ($($field:ident: $text:literal,)*) => {
        /// Symbols the architecture itself uses, made once when the table is created (upstream
        /// `create_predefined_symbols`), so the decision cycle never looks them up by name.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[allow(missing_docs)]
        pub struct Predefined {
            $(pub $field: SymbolId,)*
        }

        impl Predefined {
            const TEXTS: &'static [&'static str] = &[$($text,)*];

            /// The symbols in [`Self::TEXTS`] order, numbered from zero.
            fn numbered() -> Self {
                let mut next = 0;
                let mut number = || {
                    next += 1;
                    SymbolId(next - 1)
                };
                Self { $($field: number(),)* }
            }
        }
    };
}

// The predefined strings the port uses, in upstream's order. (Upstream also predefines the
// epmem command vocabulary, which comes with that module.) `constant` is used by no code here,
// but being predefined it makes `(make-constant-symbol)` start at `constant1`.
predefined_symbols! {
    crlf: "\n",
    state: "state",
    operator: "operator",
    superstate: "superstate",
    io: "io",
    attribute: "attribute",
    impasse: "impasse",
    choices: "choices",
    none: "none",
    constraint_failure: "constraint-failure",
    no_change: "no-change",
    multiple: "multiple",
    item_count: "item-count",
    non_numeric_count: "non-numeric-count",
    constant: "constant",
    conflict: "conflict",
    tie: "tie",
    item: "item",
    non_numeric: "non-numeric",
    quiescence: "quiescence",
    t: "t",
    nil: "nil",
    type_: "type",
    name: "name",
    input_link: "input-link",
    output_link: "output-link",
    reward_link: "reward-link",
    reward: "reward",
    value: "value",
    epmem: "epmem",
    smem: "smem",
    command: "command",
    result: "result",
    present_id: "present-id",
    memory_id: "memory-id",
    no_memory: "no-memory",
    match_score: "match-score",
    cue_size: "cue-size",
    normalized_match_score: "normalized-match-score",
    match_cardinality: "match-cardinality",
    graph_match: "graph-match",
    mapping: "mapping",
    node: "node",
    cue: "cue",
    next: "next",
    previous: "previous",
    before: "before",
    after: "after",
    retrieved: "retrieved",
    depth_retrieved: "depth-retrieved",
    status: "status",
    success: "success",
    failure: "failure",
    bad_cmd: "bad-cmd",
    depth: "depth",
    store_new: "store-new",
    link_to_new_ltm: "link-to-new-ltm",
    link_to_ltm: "link-to-ltm",
    retrieve: "retrieve",
    query: "query",
    neg_query: "neg-query",
    prohibit: "prohibit",
    store: "store",
    math_query: "math-query",
    yes: "yes",
    no: "no",
    less: "less",
    greater: "greater",
    less_or_equal: "less-or-equal",
    greater_or_equal: "greater-or-equal",
    max: "max",
    min: "min",
}

/// Deterministic, bounded intern pool converting arbitrary symbols to 32-bit integer handles.
/// Eliminates hash map bit-blasting overhead for formal verification and airborne certification.
#[derive(Debug)]
pub struct SymbolTable {
    /// The architecture's own symbols.
    pub predefined: Predefined,
    /// Every symbol's data, for [`Self::resolve`].
    symbols: Vec<SymbolData>,
    /// The same symbols as flat records. Lookups and the decision cycle's questions about a
    /// symbol (its kind, whether it is an identifier) read these, so Kani never has to read a
    /// `SymbolData` (whose `String` variant it cannot follow on the heap) to answer them.
    entries: Vec<Entry>,
    /// Identifiers flagged as goals (upstream `isa_goal`), tested by `state` conditions.
    goals: Vec<SymbolId>,
    /// Identifiers flagged as impasses (upstream `isa_impasse`), tested by `impasse` conditions.
    impasses: Vec<SymbolId>,
    /// Last number handed out per identifier letter by `make_new_identifier`.
    id_counters: [u64; 26],
}

impl Default for SymbolTable {
    fn default() -> Self {
        // The predefined texts are distinct, so they are pushed without a lookup.
        let symbols: Vec<SymbolData> = Predefined::TEXTS.iter().map(|text| SymbolData::String(String::from(*text))).collect();
        Self {
            predefined: Predefined::numbered(),
            entries: symbols.iter().map(Entry::of).collect(),
            symbols,
            goals: Vec::new(),
            impasses: Vec::new(),
            id_counters: [0; 26],
        }
    }
}

impl SymbolTable {
    /// Creates a Symbol Table holding only the predefined symbols.
    pub fn new() -> Self {
        Self::default()
    }

    fn find_or_push(&mut self, data: SymbolData) -> SymbolId {
        // Matches upstream's symbol lookup: equal values share one symbol. Float lookup
        // uses `==`, so NaN never matches and 0.0 / -0.0 share a symbol, as upstream.
        let entry = Entry::of(&data);
        let found = match &data {
            // Only strings need the data itself, and only that of other strings.
            SymbolData::String(_) => {
                self.entries.iter().zip(&self.symbols).position(|(e, existing)| e.kind == SymbolKind::String && *existing == data)
            }
            _ => self.entries.iter().position(|e| e.same_value(&entry)),
        };
        if let Some(idx) = found {
            return SymbolId(idx as u32);
        }
        self.push(data, entry)
    }

    fn push(&mut self, data: SymbolData, entry: Entry) -> SymbolId {
        self.symbols.push(data);
        self.entries.push(entry);
        SymbolId(self.entries.len() as u32 - 1)
    }

    /// A symbol's kind.
    pub fn kind(&self, id: SymbolId) -> Option<SymbolKind> {
        self.entries.get(id.0 as usize).map(|e| e.kind)
    }

    /// Whether a symbol is an identifier.
    pub fn is_identifier(&self, id: SymbolId) -> bool {
        self.kind(id) == Some(SymbolKind::Identifier)
    }

    /// The long-term identifier an identifier is linked to (upstream `id->LTI_ID`), 0 for none
    /// and for other symbols.
    pub fn lti(&self, id: SymbolId) -> u64 {
        self.entries.get(id.0 as usize).map_or(0, |e| e.lti)
    }

    /// Links an identifier to a long-term identifier, or unlinks it with 0.
    pub fn set_lti(&mut self, id: SymbolId, lti: u64) {
        if let Some(e) = self.entries.get_mut(id.0 as usize).filter(|e| e.kind == SymbolKind::Identifier) {
            e.lti = lti;
        }
    }

    /// An integer symbol's value, or `None` for other symbols.
    pub fn integer(&self, id: SymbolId) -> Option<i64> {
        self.entries.get(id.0 as usize).filter(|e| e.kind == SymbolKind::Integer).map(|e| e.integer)
    }

    /// A numeric symbol's value as a float, or `None` for other symbols.
    pub fn number(&self, id: SymbolId) -> Option<f64> {
        let e = self.entries.get(id.0 as usize)?;
        match e.kind {
            SymbolKind::Integer => Some(e.integer as f64),
            SymbolKind::Float => Some(e.float),
            SymbolKind::String | SymbolKind::Identifier => None,
        }
    }

    /// Interns a string symbol into a lightweight `SymbolId` using deterministic linear lookup.
    pub fn intern_str(&mut self, s: &str) -> SymbolId {
        self.find_or_push(SymbolData::String(String::from(s)))
    }

    /// The string symbol for `s`, if it has been interned.
    pub fn lookup_str(&self, s: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .position(|existing| matches!(existing, SymbolData::String(text) if text == s))
            .map(|idx| SymbolId(idx as u32))
    }

    /// Interns an integer constant symbol.
    pub fn intern_int(&mut self, value: i64) -> SymbolId {
        self.find_or_push(SymbolData::Integer(value))
    }

    /// Interns a floating-point constant symbol.
    pub fn intern_float(&mut self, value: f64) -> SymbolId {
        self.find_or_push(SymbolData::Float(value))
    }

    /// Interns a state identifier (e.g., 'S', 1 for S1).
    pub fn intern_id(&mut self, prefix: char, number: u64) -> SymbolId {
        self.find_or_push(SymbolData::Identifier { prefix, number })
    }

    /// Creates a fresh identifier with the given letter (upstream `make_new_identifier`):
    /// numbers count up per letter, skipping any identifier already interned.
    pub fn make_new_identifier(&mut self, letter: char) -> SymbolId {
        let prefix = if letter.is_ascii_alphabetic() { letter.to_ascii_uppercase() } else { 'I' };
        let counter = &mut self.id_counters[(prefix as u8 - b'A') as usize];
        loop {
            *counter += 1;
            let number = *counter;
            let data = SymbolData::Identifier { prefix, number };
            let entry = Entry::of(&data);
            if !self.entries.iter().any(|e| e.same_value(&entry)) {
                return self.push(data, entry);
            }
        }
    }

    /// Resolves a `SymbolId` back to its raw symbol data.
    pub fn resolve(&self, id: SymbolId) -> Option<&SymbolData> {
        self.symbols.get(id.0 as usize)
    }

    /// Flags an identifier as a goal (state), as upstream `create_new_context` does.
    pub fn mark_goal(&mut self, id: SymbolId) {
        if !self.goals.contains(&id) {
            self.goals.push(id);
        }
    }

    /// Clears the goal flag when a state is removed from the goal stack.
    pub fn unmark_goal(&mut self, id: SymbolId) {
        self.goals.retain(|goal| *goal != id);
    }

    /// Returns whether an identifier is a goal (upstream `isa_goal`).
    pub fn is_goal(&self, id: SymbolId) -> bool {
        self.goals.contains(&id)
    }

    /// Flags an identifier as an impasse (upstream `isa_impasse`).
    pub fn mark_impasse(&mut self, id: SymbolId) {
        if !self.impasses.contains(&id) {
            self.impasses.push(id);
        }
    }

    /// Returns whether an identifier is an impasse (upstream `isa_impasse`).
    pub fn is_impasse(&self, id: SymbolId) -> bool {
        self.impasses.contains(&id)
    }

    /// Returns whether two symbols have the same type, for the `<=>` test.
    pub fn same_type(&self, a: SymbolId, b: SymbolId) -> bool {
        matches!((self.kind(a), self.kind(b)), (Some(a), Some(b)) if a == b)
    }

    /// Orders two symbols for relational tests, ported from upstream `compare_symbols`
    /// (`rete.cpp`). Like upstream, symbols of incomparable types compare as `Less`, an int
    /// compared with a float compares numerically, and a NaN compares as `Equal`.
    pub fn compare(&self, a: SymbolId, b: SymbolId) -> Ordering {
        fn numcmp<T: PartialOrd>(x: T, y: T) -> Ordering {
            if x < y {
                Ordering::Less
            } else if x > y {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }

        let (Some(x), Some(y)) = (self.entries.get(a.0 as usize), self.entries.get(b.0 as usize)) else {
            return Ordering::Less;
        };
        match (x.kind, y.kind) {
            (SymbolKind::Integer, SymbolKind::Integer) => x.integer.cmp(&y.integer),
            (SymbolKind::Integer, SymbolKind::Float) => numcmp(x.integer as f64, y.float),
            (SymbolKind::Float, SymbolKind::Integer) => numcmp(x.float, y.integer as f64),
            (SymbolKind::Float, SymbolKind::Float) => numcmp(x.float, y.float),
            (SymbolKind::String, SymbolKind::String) => match (self.resolve(a), self.resolve(b)) {
                (Some(SymbolData::String(x)), Some(SymbolData::String(y))) => x.as_bytes().cmp(y.as_bytes()),
                _ => Ordering::Less,
            },
            (SymbolKind::Identifier, SymbolKind::Identifier) => {
                if x.prefix == y.prefix {
                    x.number.cmp(&y.number)
                } else {
                    x.prefix.cmp(&y.prefix)
                }
            }
            _ => Ordering::Less,
        }
    }
}
