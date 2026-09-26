use alloc::string::String;
use alloc::vec::Vec;
use hashbrown::HashMap;

/// O(1) Copy handle representing a string or identifier in Working Memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// Symbol classification type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolData {
    /// Symbolic string identifier (e.g., "^sensor-status", "offline")
    String(String),
    /// Integer scalar value
    Integer(i64),
    /// State or Goal Identifier (e.g., S1, O3)
    Identifier {
        /// Letter prefix representing symbol class (e.g., 'S' for State, 'O' for Operator)
        prefix: char,
        /// Monotonically assigned numeric index for uniqueness
        number: u64,
    },
}

/// Global intern pool converting arbitrary symbols to 32-bit integer handles.
#[derive(Debug, Default)]
pub struct SymbolTable {
    map: HashMap<String, SymbolId>,
    symbols: Vec<SymbolData>,
}

impl SymbolTable {
    /// Creates a new, empty Symbol Table.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            symbols: Vec::new(),
        }
    }

    /// Interns a string symbol into a lightweight `SymbolId`.
    pub fn intern_str(&mut self, s: &str) -> SymbolId {
        if let Some(&id) = self.map.get(s) {
            return id;
        }

        let id = SymbolId(self.symbols.len() as u32);
        let data = SymbolData::String(String::from(s));
        self.symbols.push(data);
        self.map.insert(String::from(s), id);
        id
    }

    /// Interns a state identifier (e.g., 'S', 1 for S1).
    pub fn intern_id(&mut self, prefix: char, number: u64) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(SymbolData::Identifier { prefix, number });
        id
    }

    /// Resolves a `SymbolId` back to its raw symbol data.
    pub fn resolve(&self, id: SymbolId) -> Option<&SymbolData> {
        self.symbols.get(id.0 as usize)
    }
}