#![no_std]

extern crate alloc;

/// Formal verification bridge proving hardware equivalence between AADL/AGREE contracts and the Rust implementation.
pub mod aadl_bridge;
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
/// Parser for soar scripts
pub mod soar_parser;
/// Global symbol interning table and identifier system.
pub mod symbol;
/// Truth Maintenance System (TMS) for tracking dependencies and I-support retractions.
pub mod tms;
/// Working memory arena and element management.
pub mod wm;

use aadl_bridge::{FlightTelemetryFFI, SoarCommandFFI};
use soar_parser::{SoarCondition, SoarScript, SoarValue};

/// Embedded Soar rules baked directly into the binary at compile time.
pub static BAKED_SOAR_SCRIPT_SRC: &str = include_str!(env!("SOAR_RULES_FILE"));

/// Checks a parsed rule condition against the current telemetry input.
pub fn soar_condition_matches(condition: &SoarCondition, telemetry: &FlightTelemetryFFI) -> bool {
    let attribute = condition
        .attribute
        .rsplit('.')
        .next()
        .unwrap_or(&condition.attribute);

    match (attribute, &condition.value) {
        ("sensor_valid", SoarValue::Bool(expected)) => telemetry.sensor_valid == *expected,
        ("engine_status", SoarValue::Int(expected)) => telemetry.engine_status as i64 == *expected,
        ("airspeed_kts", SoarValue::Float(expected)) => telemetry.airspeed_kts == *expected,
        ("altitude_ft", SoarValue::Float(expected)) => telemetry.altitude_ft == *expected,
        ("engine_status", SoarValue::Symbol(expected)) => {
            (telemetry.engine_status == 0 && expected == "normal")
                || (telemetry.engine_status == 2 && expected == "critical")
        }
        (_, SoarValue::Disjunction(_)) => false,
        (_, SoarValue::Arithmetic { .. }) => false,
        _ => false,
    }
}

#[no_mangle]
pub extern "C" fn soar_evaluate(telemetry: FlightTelemetryFFI) -> SoarCommandFFI {
    let script = match SoarScript::parse(BAKED_SOAR_SCRIPT_SRC) {
        Ok(script) => script,
        Err(_) => {
            return SoarCommandFFI {
                operator_id: 0,
                target_altitude_ft: telemetry.altitude_ft,
                is_valid: false,
            };
        }
    };
    let mut matched_op_id = 0u32;
    let mut matched_target_alt = telemetry.altitude_ft;
    let mut matched_rule = false;

    for prod in &script.productions {
        let matches = prod
            .conditions
            .iter()
            .all(|condition| soar_condition_matches(condition, &telemetry));

        if matches && !prod.actions.is_empty() {
            let action = &prod.actions[0];
            matched_op_id = action.operator_id;
            matched_target_alt = action.target_altitude_ft;
            matched_rule = true;
            break;
        }
    }

    SoarCommandFFI {
        operator_id: matched_op_id,
        target_altitude_ft: matched_target_alt,
        is_valid: telemetry.sensor_valid && matched_rule && matched_op_id != 0,
    }
}
