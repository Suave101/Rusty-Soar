#![no_std]

extern crate alloc;

/// Agent implementation coordinating the Soar decision cycle and memory modules.
pub mod agent;
/// Episodic memory system for temporal snapshot recording and retrieval.
pub mod epmem;
/// Architectural impasse types and substate handling.
pub mod impasse;
/// Explanation-Based Learning (Chunking) for rule synthesis.
pub mod learning;
/// Preference resolution semantics for candidate operator selection.
pub mod preference;
/// Production rule structures and action definitions.
pub mod production;
/// Index-based RETE pattern matching engine.
pub mod rete;
/// Reinforcement Learning (RL) mechanism for reward-based numeric preference updates.
pub mod rl;
/// Semantic memory system for long-term factual knowledge storage and retrieval.
pub mod smem;
/// Global symbol interning table and identifier system.
pub mod symbol;
/// Truth Maintenance System (TMS) for tracking dependencies and I-support retractions.
pub mod tms;
/// Working memory arena and element management.
pub mod wm;
/// Formal verification bridge proving hardware equivalence between AADL/AGREE contracts and the Rust implementation.
pub mod aadl_bridge;
/// Parser for soar scripts
pub mod soar_parser;

use aadl_bridge::{FlightTelemetryFFI, SoarCommandFFI};
use soar_parser::{SoarScript, SoarValue};

/// Embedded Soar rules baked directly into the binary at compile time.
pub static BAKED_SOAR_SCRIPT_SRC: &str = include_str!(env!("SOAR_RULES_FILE"));

#[no_mangle]
pub extern "C" fn soar_evaluate(telemetry: FlightTelemetryFFI) -> SoarCommandFFI {
    // Parse baked rules directly from static memory
    let script = SoarScript::parse(BAKED_SOAR_SCRIPT_SRC).unwrap_or_default();

    let mut matched_op_id = 0u32;
    let mut matched_target_alt = telemetry.altitude_ft;

    for prod in &script.productions {
        let mut matches = true;
        for cond in &prod.conditions {
            if cond.attribute == "engine_status" {
                if let SoarValue::Int(expected_status) = cond.value {
                    if telemetry.engine_status as i64 != expected_status {
                        matches = false;
                    }
                }
            }
        }

        if matches && !prod.actions.is_empty() {
            let action = &prod.actions[0];
            matched_op_id = action.operator_id;
            matched_target_alt = action.target_altitude_ft;
            break;
        }
    }

    SoarCommandFFI {
        operator_id: matched_op_id,
        target_altitude_ft: matched_target_alt,
        is_valid: telemetry.sensor_valid,
    }
}