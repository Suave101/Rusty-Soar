#![no_std]

extern crate alloc;

/// Formal verification bridge proving hardware equivalence between AADL/AGREE contracts and the Rust implementation.
pub mod aadl_bridge;
/// Agent implementation coordinating the Soar decision cycle and memory modules.
pub mod agent;
/// Episodic memory: the store and cue-based search.
pub mod epmem;
/// Architectural impasse types and substate handling.
pub mod impasse;
/// Explanation-Based Learning (Chunking) for rule synthesis.
pub mod learning;
/// Preference resolution semantics for candidate operator selection.
pub mod preference;
/// Compilation of parsed productions into Rete conditions and RHS actions.
pub mod production;
/// Fixed-capacity pools used by the engine's storage.
pub mod pool;
/// Index-based RETE pattern matching engine.
pub mod rete;
/// Reinforcement Learning (RL) mechanism for reward-based numeric preference updates.
pub mod rl;
/// Semantic memory system for long-term factual knowledge storage and retrieval.
pub mod smem;
/// Mersenne Twister random numbers, matching upstream's `MTRand`.
pub mod soar_rand;
/// Parser for soar scripts
pub mod soar_parser;
/// Global symbol interning table and identifier system.
pub mod symbol;
/// Truth Maintenance System (TMS) for tracking dependencies and I-support retractions.
pub mod tms;
/// Working memory arena and element management.
pub mod wm;
/// Working-memory activation: settings, reference histories and decay.
pub mod wma;

use aadl_bridge::{FlightTelemetryFFI, SoarCommandFFI};
use agent::SoarAgent;
use preference::DecisionResult;
use soar_parser::SoarScript;
use symbol::{SymbolData, SymbolId};

/// Embedded Soar rules baked directly into the binary at compile time.
pub static BAKED_SOAR_SCRIPT_SRC: &str = include_str!(env!("SOAR_RULES_FILE"));

/// Runs one decision cycle of a Soar agent loaded with `script` on the given telemetry and
/// reports the selected operator's `^operator_id` and `^target_altitude_ft`.
///
/// The telemetry fields are placed both on `^io.input-link.telemetry` and directly on the top
/// state, so rules may test either. The command is valid when the sensors are valid and an
/// operator with a non-zero `^operator_id` was selected.
pub fn evaluate_script(script: &SoarScript, telemetry: &FlightTelemetryFFI) -> SoarCommandFFI {
    let invalid = SoarCommandFFI {
        operator_id: 0,
        target_altitude_ft: telemetry.altitude_ft,
        is_valid: false,
    };

    let mut agent = SoarAgent::new();
    let (state, input_link) = agent.create_top_state();
    if agent.install_soar_script(script, state).is_err() {
        return invalid;
    }

    let sensor_valid = agent.symbols.intern_str(if telemetry.sensor_valid { "true" } else { "false" });
    let engine_status = agent.symbols.intern_int(i64::from(telemetry.engine_status));
    let airspeed = agent.symbols.intern_float(f64::from(telemetry.airspeed_kts));
    let altitude = agent.symbols.intern_float(f64::from(telemetry.altitude_ft));
    let telemetry_id = agent.symbols.make_new_identifier('T');
    let telemetry_attr = agent.symbols.intern_str("telemetry");
    agent.insert_wme(input_link, telemetry_attr, telemetry_id);
    for (name, value) in [
        ("sensor_valid", sensor_valid),
        ("engine_status", engine_status),
        ("airspeed_kts", airspeed),
        ("altitude_ft", altitude),
    ] {
        let attr = agent.symbols.intern_str(name);
        agent.insert_wme(telemetry_id, attr, value);
        agent.insert_wme(state, attr, value);
    }

    let DecisionResult::Selected(operator) = agent.run_decision_cycle(state) else {
        return invalid;
    };
    let operator_attr = |agent: &SoarAgent, name: &str| -> Option<SymbolId> {
        agent.wm.iter().find_map(|w| {
            let is_attr = matches!(agent.symbols.resolve(w.attr), Some(SymbolData::String(s)) if s == name);
            (w.id == operator && is_attr).then_some(w.val)
        })
    };
    let operator_id = match operator_attr(&agent, "operator_id").and_then(|v| agent.symbols.resolve(v)) {
        Some(SymbolData::Integer(i)) => u32::try_from(*i).unwrap_or(0),
        _ => 0,
    };
    let target_altitude_ft = match operator_attr(&agent, "target_altitude_ft").and_then(|v| agent.symbols.resolve(v)) {
        Some(SymbolData::Integer(i)) => *i as f32,
        Some(SymbolData::Float(x)) => *x as f32,
        _ => telemetry.altitude_ft,
    };

    SoarCommandFFI {
        operator_id,
        target_altitude_ft,
        is_valid: telemetry.sensor_valid && operator_id != 0,
    }
}

#[no_mangle]
pub extern "C" fn soar_evaluate(telemetry: FlightTelemetryFFI) -> SoarCommandFFI {
    match SoarScript::parse(BAKED_SOAR_SCRIPT_SRC) {
        Ok(script) => evaluate_script(&script, &telemetry),
        Err(_) => SoarCommandFFI {
            operator_id: 0,
            target_altitude_ft: telemetry.altitude_ft,
            is_valid: false,
        },
    }
}
