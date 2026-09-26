//! C-ABI FFI Bridge implementing AADL/AGREE contract bindings for rusty-soar.

use crate::agent::SoarAgent;
use crate::preference::{resolve_preferences, DecisionResult, Preference, PreferenceType};
use crate::symbol::SymbolId;

/// Maximum preferences capacity allocated per decision cycle frame.
pub const MAX_PREFERENCES_PER_CYCLE: usize = 8;

/// FFI telemetry input struct matching AADL `Flight_Telemetry.impl`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FlightTelemetryFFI {
    /// Airspeed in knots (kts).
    pub airspeed_kts: f32,
    /// Altitude in feet (ft).
    pub altitude_ft: f32,
    /// Engine health state: 0 = Normal, 1 = Degradation, 2 = Critical Failure.
    pub engine_status: u8,
    /// Sensor validity flag indicating telemetry integrity.
    pub sensor_valid: bool,
}

/// FFI output command struct matching AADL `Operator_Command.impl`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct OperatorCommandFFI {
    /// Selected operator identifier (10 = Cruise, 99 = Emergency Landing).
    pub operator_id: u32,
    /// Commanded target heading in degrees.
    pub target_heading_deg: f32,
    /// Commanded target altitude in feet.
    pub target_altitude_ft: f32,
    /// Command validity indicator.
    pub is_valid: bool,
}

/// FFI State Container for AADL Periodic Executable Execution.
pub struct SoarAadlAgent {
    /// Embedded Soar cognitive agent engine instance.
    pub agent: SoarAgent,
    /// Root state symbol identifier.
    pub state_id: SymbolId,
    /// Symbol ID for the cruise operator.
    pub op_cruise: SymbolId,
    /// Symbol ID for the emergency landing operator.
    pub op_emergency: SymbolId,
}

impl Default for SoarAadlAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl SoarAadlAgent {
    /// Constructs and initializes a new `SoarAadlAgent` with pre-allocated preference buffers.
    pub fn new() -> Self {
        let mut agent = SoarAgent::new();
        agent.preferences.reserve(MAX_PREFERENCES_PER_CYCLE);

        let state_id = agent.symbols.intern_id('S', 1);
        let op_cruise = SymbolId(10);
        let op_emergency = SymbolId(99);

        Self {
            agent,
            state_id,
            op_cruise,
            op_emergency,
        }
    }

    /// Step execution matching the AADL 10ms periodic compute frame.
    ///
    /// Evaluates rules directly into pre-allocated preference memory and resolves decisions,
    /// avoiding heap reallocations (`RawVecInner::finish_grow`) during CBMC verification.
    pub fn step(&mut self, input: &FlightTelemetryFFI) -> OperatorCommandFFI {
        if !input.sensor_valid {
            return OperatorCommandFFI {
                operator_id: 0,
                target_heading_deg: 0.0,
                target_altitude_ft: 0.0,
                is_valid: false,
            };
        }

        self.agent.preferences.clear();

        // Production Rules: Engine Failure vs Nominal Cruise
        if input.engine_status == 2 {
            self.agent.preferences.push(Preference {
                state: self.state_id,
                operator: self.op_emergency,
                preference_type: PreferenceType::Acceptable,
            });
            self.agent.preferences.push(Preference {
                state: self.state_id,
                operator: self.op_cruise,
                preference_type: PreferenceType::Reject,
            });
        } else {
            self.agent.preferences.push(Preference {
                state: self.state_id,
                operator: self.op_cruise,
                preference_type: PreferenceType::Acceptable,
            });
        }

        // Execute Decision Phase via Soar Preference Resolution Engine
        let result = resolve_preferences(self.state_id, &self.agent.preferences);

        let selected_id = match result {
            DecisionResult::Selected(op) => op.0,
            _ => 0,
        };

        let target_alt = if selected_id == 99 { 0.0 } else { input.altitude_ft };

        OperatorCommandFFI {
            operator_id: selected_id,
            target_heading_deg: 0.0,
            target_altitude_ft: target_alt,
            is_valid: true,
        }
    }
}