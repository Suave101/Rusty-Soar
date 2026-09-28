use alloc::string::String;
use alloc::vec::Vec;

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

/// Deterministic, bounded intern pool converting arbitrary symbols to 32-bit integer handles.
/// Eliminates hash map bit-blasting overhead for formal verification and airborne certification.
#[derive(Debug, Default)]
pub struct SymbolTable {
    symbols: Vec<SymbolData>,
}

impl SymbolTable {
    /// Creates a new, empty Symbol Table.
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
        }
    }

    /// Interns a string symbol into a lightweight `SymbolId` using deterministic linear lookup.
    pub fn intern_str(&mut self, s: &str) -> SymbolId {
        if let Some((idx, _)) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, data)| match data {
                SymbolData::String(existing) => existing == s,
                _ => false,
            })
        {
            return SymbolId(idx as u32);
        }

        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(SymbolData::String(String::from(s)));
        id
    }

    /// Interns a state identifier (e.g., 'S', 1 for S1).
    pub fn intern_id(&mut self, prefix: char, number: u64) -> SymbolId {
        if let Some((idx, _)) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, data)| match data {
                SymbolData::Identifier {
                    prefix: p,
                    number: n,
                } => *p == prefix && *n == number,
                _ => false,
            })
        {
            return SymbolId(idx as u32);
        }

        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(SymbolData::Identifier { prefix, number });
        id
    }

    /// Resolves a `SymbolId` back to its raw symbol data.
    pub fn resolve(&self, id: SymbolId) -> Option<&SymbolData> {
        self.symbols.get(id.0 as usize)
    }
}
