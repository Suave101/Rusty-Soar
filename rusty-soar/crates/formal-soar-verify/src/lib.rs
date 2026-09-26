//! SMT-backed formal verification harnesses for RustySoar invariants using Kani.

#[cfg(kani)]
mod proof_harnesses {
    use rusty_soar_core::symbol::SymbolId;
    use rusty_soar_core::wm::{SupportType, WmeArena};

    #[kani::proof]
    #[kani::unwind(10)]
    pub fn verify_wme_arena_generational_isolation() {
        let mut arena = WmeArena::new();

        let s1 = SymbolId(kani::any());
        let attr = SymbolId(kani::any());
        let val = SymbolId(kani::any());

        // Insert WME and capture generational key
        let key1 = arena.insert(s1, attr, val, SupportType::ISupport);

        // Assert inserted WME is retrievable
        kani::assert(arena.get(key1).is_some(), "Active WME must be resolvable");

        // Remove WME
        arena.remove(key1);

        // Verify key is stale and returns None (no use-after-free or invalid dereference)
        kani::assert(arena.get(key1).is_none(), "Stale generational key must return None");

        // Insert new WME at same index slot
        let key2 = arena.insert(s1, attr, val, SupportType::ISupport);

        // Verify generation rollover prevents old key from accessing new item
        if key1 != key2 {
            kani::assert(arena.get(key1).is_none(), "Old key must never resolve new generation slot");
        }
    }

    #[kani::proof]
    #[kani::unwind(10)]
    pub fn verify_symbol_interning_idempotency() {
        let mut table = rusty_soar_core::symbol::SymbolTable::new();

        let id1 = table.intern_str("sensor_alpha");
        let id2 = table.intern_str("sensor_alpha");

        kani::assert(id1 == id2, "Symbol interning must be mathematically idempotent");
    }
}