#![no_std]

extern crate alloc;

#[cfg(kani)]
mod proof_harnesses {
    use rusty_soar_core::preference::{
        resolve_preferences, resolve_preferences_with_rl, DecisionResult, Preference, PreferenceType,
    };
    use rusty_soar_core::rl::ReinforcementLearning;
    use rusty_soar_core::symbol::{SymbolId, SymbolTable};

    /// Invariant 1: ID symbol interning is deterministic and idempotent.
    /// Uses `core::mem::forget` and `#[kani::unwind]` to bypass drop glue loop unwinding.
    #[kani::proof]
    #[kani::unwind(10)]
    fn verify_symbol_id_interning_idempotency() {
        let mut table = SymbolTable::new();
        let s1 = table.intern_id('S', 1);
        let s2 = table.intern_id('S', 1);
        kani::assert(s1 == s2, "Interning identical ID symbols must yield identical SymbolId");
        core::mem::forget(table);
    }

    /// Invariant 2: Explicit Reject preferences strictly override Acceptable preferences.
    #[kani::proof]
    #[kani::unwind(10)]
    fn verify_reject_preference_override() {
        let state = SymbolId(1);
        let op_a = SymbolId(10);
        let op_b = SymbolId(20);

        let prefs = [
            Preference {
                state,
                operator: op_a,
                preference_type: PreferenceType::Acceptable,
            },
            Preference {
                state,
                operator: op_b,
                preference_type: PreferenceType::Acceptable,
            },
            Preference {
                state,
                operator: op_b,
                preference_type: PreferenceType::Reject,
            },
        ];

        let result = resolve_preferences(state, &prefs);

        kani::assert(
            result == DecisionResult::Selected(op_a),
            "Rejected operator B must be excluded, leaving operator A selected",
        );
        core::mem::forget(result);
    }

    /// Invariant 3: Equal Q-values under acceptable preferences must yield a TieImpasse.
    #[kani::proof]
    #[kani::unwind(10)]
    fn verify_equal_q_value_tie_impasse() {
        let state = SymbolId(1);
        let op_a = SymbolId(10);
        let op_b = SymbolId(20);

        let prefs = [
            Preference {
                state,
                operator: op_a,
                preference_type: PreferenceType::Acceptable,
            },
            Preference {
                state,
                operator: op_b,
                preference_type: PreferenceType::Acceptable,
            },
        ];

        let rl = ReinforcementLearning::default();
        let result = resolve_preferences_with_rl(state, &prefs, &rl);

        kani::assert(
            matches!(result, DecisionResult::TieImpasse(_)),
            "Equal Q-values for candidate operators must generate a TieImpasse",
        );
        core::mem::forget(rl);
        core::mem::forget(result);
    }
}