use slotmap::{new_key_type, SlotMap};
use crate::symbol::SymbolId;

new_key_type! {
    /// Generational 64-bit key used to reference WMEs without raw pointers.
    pub struct WmeKey;
}

/// Memory lifecycle support classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportType {
    /// Instantiation Support: Retracts automatically when creating rule LHS becomes false.
    ISupport,
    /// Operator Support: Persists until explicitly removed or overwritten by another operator.
    OSupport,
}

/// Working Memory Element triple: (Identifier ^Attribute Value)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wme {
    /// Root Identifier (e.g., S1)
    pub id: SymbolId,
    /// Attribute edge (e.g., ^sensor-status)
    pub attribute: SymbolId,
    /// Value node or scalar (e.g., offline)
    pub value: SymbolId,
    /// Monotonically increasing sequence ID for fine-grained retraction tracking
    pub timetag: u64,
    /// Memory persistence type
    pub support: SupportType,
}

/// Zero-GC generational arena for Working Memory Element storage.
pub struct WmeArena {
    storage: SlotMap<WmeKey, Wme>,
    next_timetag: u64,
}

impl WmeArena {
    /// Initializes an empty WME Arena.
    pub fn new() -> Self {
        Self {
            storage: SlotMap::with_key(),
            next_timetag: 1,
        }
    }

    /// Inserts a new WME triple into the arena, returning a safe generational `WmeKey`.
    pub fn insert(&mut self, id: SymbolId, attribute: SymbolId, value: SymbolId, support: SupportType) -> WmeKey {
        let timetag = self.next_timetag;
        self.next_timetag += 1;

        let wme = Wme {
            id,
            attribute,
            value,
            timetag,
            support,
        };

        self.storage.insert(wme)
    }

    /// Removes a WME using its key. Stale key references immediately become invalid in O(1).
    pub fn remove(&mut self, key: WmeKey) -> Option<Wme> {
        self.storage.remove(key)
    }

    /// Immutable lookup for a WME by generational key.
    pub fn get(&self, key: WmeKey) -> Option<&Wme> {
        self.storage.get(key)
    }

    /// Returns the total number of active WMEs currently in Working Memory.
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Returns true if Working Memory contains no active WMEs.
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
}

impl Default for WmeArena {
    fn default() -> Self {
        Self::new()
    }
}