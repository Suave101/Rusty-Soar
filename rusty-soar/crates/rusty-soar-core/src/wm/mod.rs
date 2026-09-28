//! Working Memory Arena for element storage and support management.

use alloc::vec::Vec;
use crate::symbol::SymbolId;

/// Support classification for Working Memory assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportType {
    /// Instantiation support tied to rule match justifications.
    ISupport,
    /// Operator support persisting beyond rule match retracts.
    OSupport,
}

/// Unique handle key referencing a Working Memory Element in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WmeKey(pub usize);

/// Fundamental Working Memory Element (WME) representing an (Identifier, Attribute, Value) triple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wme {
    /// Handle key in arena.
    pub key: WmeKey,
    /// Triple subject identifier symbol.
    pub id: SymbolId,
    /// Triple predicate attribute symbol.
    pub attr: SymbolId,
    /// Triple object value symbol.
    pub val: SymbolId,
    /// Architectural support type.
    pub support: SupportType,
}

/// Memory arena managing active WME instances.
#[derive(Debug, Default)]
pub struct WmeArena {
    storage: Vec<Option<Wme>>,
    count: usize,
}

impl WmeArena {
    /// Creates a new empty `WmeArena`.
    pub fn new() -> Self {
        Self {
            storage: Vec::new(),
            count: 0,
        }
    }

    /// Inserts a new WME triple into the arena, returning its handle key.
    pub fn insert(
        &mut self,
        id: SymbolId,
        attr: SymbolId,
        val: SymbolId,
        support: SupportType,
    ) -> WmeKey {
        if let Some(existing) = self
            .storage
            .iter()
            .flatten()
            .find(|wme| wme.id == id && wme.attr == attr && wme.val == val)
        {
            return existing.key;
        }

        let key = WmeKey(self.storage.len());
        let wme = Wme {
            key,
            id,
            attr,
            val,
            support,
        };
        self.storage.push(Some(wme));
        self.count += 1;
        key
    }

    /// Retracts a WME from the arena by handle key.
    pub fn remove(&mut self, key: WmeKey) -> Option<Wme> {
        if key.0 < self.storage.len() {
            let removed = self.storage[key.0].take();
            if removed.is_some() {
                self.count -= 1;
            }
            removed
        } else {
            None
        }
    }

    /// Retrieves a reference to a WME by handle key.
    pub fn get(&self, key: WmeKey) -> Option<&Wme> {
        self.storage.get(key.0)?.as_ref()
    }

    /// Finds an active WME by its identifier, attribute, and value triple.
    pub fn find(&self, id: SymbolId, attr: SymbolId, val: SymbolId) -> Option<WmeKey> {
        self.storage
            .iter()
            .flatten()
            .find(|wme| wme.id == id && wme.attr == attr && wme.val == val)
            .map(|wme| wme.key)
    }

    /// Collects all active WME keys matching a given identifier symbol.
    pub fn wmes_by_id(&self, id: SymbolId) -> Vec<WmeKey> {
        self.storage
            .iter()
            .filter_map(|opt| {
                if let Some(wme) = opt {
                    if wme.id == id {
                        return Some(wme.key);
                    }
                }
                None
            })
            .collect()
    }

    /// Returns the total active WME count in memory.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if the arena contains no active WMEs.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterates over active WMEs in deterministic insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Wme> {
        self.storage.iter().filter_map(|wme| wme.as_ref())
    }
}