#![no_std]

extern crate alloc;

#[cfg(kani)]
mod proof_harnesses {
    use rusty_soar_core::aadl_bridge::{FlightTelemetryFFI, SoarAadlAgent};
    use rusty_soar_core::preference::{
        resolve_preferences, resolve_preferences_with_rl, DecisionResult, Preference, PreferenceType,
    };
    use rusty_soar_core::rl::ReinforcementLearning;
    use rusty_soar_core::symbol::{SymbolId, SymbolTable};

    #[kani::proof]
    #[kani::unwind(10)]
    fn verify_symbol_id_interning_idempotency() {
        let mut table = SymbolTable::new();
        let s1 = table.intern_id('S', 1);
        let s2 = table.intern_id('S', 1);
        kani::assert(s1 == s2, "Interning identical ID symbols must yield identical SymbolId");
        core::mem::forget(table);
    }

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

    /// Proof of Equivalence against AADL/AGREE System Contract.
    /// Proves that under symbolic inputs matching AGREE assumptions,
    /// `SoarAadlAgent::step()` strictly satisfies all AGREE contract guarantees.
    #[kani::proof]
    #[kani::unwind(10)]
    fn verify_aadl_agree_contract_equivalence() {
        let mut aadl_agent = SoarAadlAgent::new();

        let engine_status: u8 = kani::any();
        let sensor_valid: bool = kani::any();
        let airspeed_kts: f32 = kani::any();
        let altitude_ft: f32 = kani::any();

        // AGREE Assumptions
        kani::assume(sensor_valid == true);
        kani::assume(airspeed_kts >= 0.0 && airspeed_kts <= 300.0);
        kani::assume(altitude_ft >= 0.0 && altitude_ft <= 50000.0);
        kani::assume(engine_status == 0 || engine_status == 2);

        let telemetry = FlightTelemetryFFI {
            airspeed_kts,
            altitude_ft,
            engine_status,
            sensor_valid,
        };

        let cmd = aadl_agent.step(&telemetry);

        // AGREE Guarantees
        kani::assert(cmd.is_valid, "Command must be marked valid when sensor is valid");

        if engine_status == 2 {
            kani::assert(
                cmd.operator_id == 99,
                "AGREE Guarantee Violation: Engine failure MUST select Emergency Landing (99)",
            );
            kani::assert(
                cmd.target_altitude_ft == 0.0,
                "AGREE Guarantee Violation: Emergency landing target altitude MUST be 0.0",
            );
        } else if engine_status == 0 {
            kani::assert(
                cmd.operator_id == 10,
                "AGREE Guarantee Violation: Nominal engine MUST select Cruise (10)",
            );
        }

        core::mem::forget(aadl_agent);
    }
}