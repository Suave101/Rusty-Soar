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
    /// An input WME (upstream `add_input_wme`): no preference supports it, and only input
    /// removes it.
    Input,
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
    /// Whether this is an acceptable-preference WME such as `(S1 ^operator O1 +)`, which is
    /// distinct from the WME `(S1 ^operator O1)` with the same triple.
    pub acceptable: bool,
}

/// Default number of WME slots, matching the Rete's default WME capacity.
pub const DEFAULT_WME_CAPACITY: usize = 4096;

/// Memory arena managing active WME instances in a fixed number of slots, allocated once.
///
/// A key encodes its slot and that slot's generation (`slot + capacity * generation`), so a
/// key kept after its WME is removed never refers to a later WME reusing the slot.
#[derive(Debug)]
pub struct WmeArena {
    slots: Vec<(usize, Option<Wme>)>,
    free: Vec<u32>,
    count: usize,
}

impl Default for WmeArena {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_WME_CAPACITY)
    }
}

impl WmeArena {
    /// Creates a new empty `WmeArena` with the default capacity.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty arena with `capacity` slots.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        slots.resize_with(capacity, || (0, None));
        Self { slots, free: (0..capacity as u32).rev().collect(), count: 0 }
    }

    /// Number of slots.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn slot_of(&self, key: WmeKey) -> Option<usize> {
        let capacity = self.slots.len();
        if capacity == 0 {
            return None;
        }
        let slot = key.0 % capacity;
        (self.slots[slot].0 == key.0 / capacity).then_some(slot)
    }

    /// Inserts a new WME triple into the arena, returning its handle key, or `None` when
    /// every slot is in use.
    pub fn insert(
        &mut self,
        id: SymbolId,
        attr: SymbolId,
        val: SymbolId,
        support: SupportType,
    ) -> Option<WmeKey> {
        self.insert_with_flag(id, attr, val, support, false)
    }

    /// Inserts a WME, or an acceptable-preference WME when `acceptable` is set. Returns the
    /// existing key if the WME is already present, or `None` when every slot is in use.
    pub fn insert_with_flag(
        &mut self,
        id: SymbolId,
        attr: SymbolId,
        val: SymbolId,
        support: SupportType,
        acceptable: bool,
    ) -> Option<WmeKey> {
        if let Some(existing) = self.find_with_flag(id, attr, val, acceptable) {
            return Some(existing);
        }
        self.insert_distinct(id, attr, val, support, acceptable)
    }

    /// Inserts a WME even when one with the same triple exists, as upstream does for input
    /// WMEs. Returns `None` when every slot is in use.
    pub fn insert_distinct(
        &mut self,
        id: SymbolId,
        attr: SymbolId,
        val: SymbolId,
        support: SupportType,
        acceptable: bool,
    ) -> Option<WmeKey> {
        let slot = self.free.pop()? as usize;
        let key = WmeKey(slot + self.slots.len() * self.slots[slot].0);
        self.slots[slot].1 = Some(Wme {
            key,
            id,
            attr,
            val,
            support,
            acceptable,
        });
        self.count += 1;
        Some(key)
    }

    /// Retracts a WME from the arena by handle key.
    pub fn remove(&mut self, key: WmeKey) -> Option<Wme> {
        let slot = self.slot_of(key)?;
        let removed = self.slots[slot].1.take()?;
        self.slots[slot].0 = self.slots[slot].0.wrapping_add(1);
        self.free.push(slot as u32);
        self.count -= 1;
        Some(removed)
    }

    /// Retrieves a reference to a WME by handle key.
    pub fn get(&self, key: WmeKey) -> Option<&Wme> {
        self.slots[self.slot_of(key)?].1.as_ref()
    }

    /// Finds an active (non-acceptable) WME by its identifier, attribute, and value triple.
    pub fn find(&self, id: SymbolId, attr: SymbolId, val: SymbolId) -> Option<WmeKey> {
        self.find_with_flag(id, attr, val, false)
    }

    /// Finds an active WME with the given triple and acceptable flag.
    pub fn find_with_flag(&self, id: SymbolId, attr: SymbolId, val: SymbolId, acceptable: bool) -> Option<WmeKey> {
        self.iter()
            .find(|wme| wme.id == id && wme.attr == attr && wme.val == val && wme.acceptable == acceptable)
            .map(|wme| wme.key)
    }

    /// Collects all active WME keys matching a given identifier symbol.
    pub fn wmes_by_id(&self, id: SymbolId) -> Vec<WmeKey> {
        self.iter().filter(|wme| wme.id == id).map(|wme| wme.key).collect()
    }

    /// Returns the total active WME count in memory.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if the arena contains no active WMEs.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterates over active WMEs in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Wme> {
        self.slots.iter().filter_map(|(_, wme)| wme.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_slots_get_new_keys() {
        let mut wm = WmeArena::with_capacity(1);
        let a = wm.insert(SymbolId(1), SymbolId(2), SymbolId(3), SupportType::ISupport).unwrap();
        assert!(wm.insert(SymbolId(4), SymbolId(5), SymbolId(6), SupportType::ISupport).is_none(), "full");
        wm.remove(a);
        let b = wm.insert(SymbolId(4), SymbolId(5), SymbolId(6), SupportType::ISupport).unwrap();
        assert_ne!(a, b);
        assert!(wm.get(a).is_none(), "the old key does not see the new WME");
        assert_eq!(wm.get(b).map(|w| w.val), Some(SymbolId(6)));
    }
}
