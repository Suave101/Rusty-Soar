//! SMT-backed formal verification harnesses for RustySoar invariants using Kani.

#[cfg(kani)]
mod proof_harnesses {
    use rusty_soar_core::symbol::{SymbolId, SymbolTable};
    use rusty_soar_core::wm::{SupportType, WmeArena};

    #[kani::proof]
    #[kani::unwind(10)]
    pub fn verify_wme_arena_generational_isolation() {
        let mut arena = WmeArena::new();

        let s1 = SymbolId(kani::any());
        let attr = SymbolId(kani::any());
        let val = SymbolId(kani::any());

        let key1 = arena.insert(s1, attr, val, SupportType::ISupport);
        kani::assert(arena.get(key1).is_some(), "Active WME must be resolvable");

        arena.remove(key1);
        kani::assert(arena.get(key1).is_none(), "Stale generational key must return None");

        let key2 = arena.insert(s1, attr, val, SupportType::ISupport);
        if key1 != key2 {
            kani::assert(
                arena.get(key1).is_none(),
                "Old key must never resolve new generation slot",
            );
        }
    }

    #[kani::proof]
    #[kani::unwind(5)] // Linear array iteration only requires unwinding to harness step count
    pub fn verify_symbol_interning_idempotency() {
        let mut table = SymbolTable::new();

        let id1 = table.intern_str("a");
        let id2 = table.intern_str("a");

        kani::assert(id1 == id2, "Symbol interning must be idempotent");
    }
}