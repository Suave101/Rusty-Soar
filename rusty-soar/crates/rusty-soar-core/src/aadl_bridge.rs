//! C-ABI FFI Bridge implementing AADL/AGREE contract bindings for rusty-soar.

use crate::agent::{Action, SoarAgent};
use crate::preference::{Preference, PreferenceType};
use crate::rete::AlphaTest;
use crate::symbol::SymbolId;

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
    /// Attribute symbol identifier for engine status.
    pub attr_engine: SymbolId,
    /// Value symbol identifier for normal engine status.
    pub val_normal: SymbolId,
    /// Value symbol identifier for critical engine status.
    pub val_critical: SymbolId,
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
    /// Constructs and initializes a new `SoarAadlAgent` with predefined production rules.
    pub fn new() -> Self {
        let mut agent = SoarAgent::new();

        let state_id = agent.symbols.intern_id('S', 1);
        let attr_engine = agent.symbols.intern_id('A', 1);
        let val_normal = agent.symbols.intern_id('V', 1);
        let val_critical = agent.symbols.intern_id('V', 2);

        let op_cruise = SymbolId(10);
        let op_emergency = SymbolId(99);

        // Production Rule 1: Propose Cruise Operator under normal engine state
        agent.add_rule(
            "propose_cruise",
            alloc::vec![(
                AlphaTest {
                    id: Some(state_id),
                    attr: Some(attr_engine),
                    val: Some(val_normal),
                },
                alloc::vec![],
            )],
            alloc::vec![Action::Prefer(Preference {
                state: state_id,
                operator: op_cruise,
                preference_type: PreferenceType::Acceptable,
            })],
        );

        // Production Rule 2: Propose Emergency Land Operator & Reject Cruise under critical engine state
        agent.add_rule(
            "propose_emergency_landing",
            alloc::vec![(
                AlphaTest {
                    id: Some(state_id),
                    attr: Some(attr_engine),
                    val: Some(val_critical),
                },
                alloc::vec![],
            )],
            alloc::vec![
                Action::Prefer(Preference {
                    state: state_id,
                    operator: op_emergency,
                    preference_type: PreferenceType::Acceptable,
                }),
                Action::Prefer(Preference {
                    state: state_id,
                    operator: op_cruise,
                    preference_type: PreferenceType::Reject,
                }),
            ],
        );

        Self {
            agent,
            state_id,
            attr_engine,
            val_normal,
            val_critical,
            op_cruise,
            op_emergency,
        }
    }

    /// Step execution matching the AADL 10ms periodic compute frame.
    ///
    /// Evaluates input flight telemetry, updates working memory, executes decision phase,
    /// and generates the corresponding output operator command.
    pub fn step(&mut self, input: &FlightTelemetryFFI) -> OperatorCommandFFI {
        if !input.sensor_valid {
            return OperatorCommandFFI {
                operator_id: 0,
                target_heading_deg: 0.0,
                target_altitude_ft: 0.0,
                is_valid: false,
            };
        }

        // Reset preferences and decision outputs for a clean frame execution
        self.agent.preferences.clear();
        self.agent.selected_operator = None;

        // Insert input telemetry WMEs
        let status_val = if input.engine_status == 2 {
            self.val_critical
        } else {
            self.val_normal
        };

        let wme_key = self.agent.insert_wme(self.state_id, self.attr_engine, status_val);

        // Execute Elaboration + Decision Phases within the 2ms AGREE budget
        self.agent.run_decision_cycle(self.state_id);

        let selected = self.agent.selected_operator.unwrap_or(SymbolId(0));

        // Cleanup cycle WMEs
        self.agent.remove_wme(wme_key);

        let target_alt = if selected.0 == 99 { 0.0 } else { input.altitude_ft };

        OperatorCommandFFI {
            operator_id: selected.0,
            target_heading_deg: 0.0,
            target_altitude_ft: target_alt,
            is_valid: true,
        }
    }
}

/// Allocates and initializes a new `SoarAadlAgent` handle over C-ABI.
///
/// # Safety
/// Caller is responsible for eventually freeing the handle via `soar_aadl_free`.
#[no_mangle]
pub extern "C" fn soar_aadl_init() -> *mut SoarAadlAgent {
    let agent = alloc::boxed::Box::new(SoarAadlAgent::new());
    alloc::boxed::Box::into_raw(agent)
}

/// Executes a single periodic step for an initialized `SoarAadlAgent` over C-ABI.
///
/// # Safety
/// `handle`, `input`, and `output` must be valid, non-null pointers.
#[no_mangle]
pub extern "C" fn soar_aadl_step(
    handle: *mut SoarAadlAgent,
    input: *const FlightTelemetryFFI,
    output: *mut OperatorCommandFFI,
) {
    if handle.is_null() || input.is_null() || output.is_null() {
        return;
    }

    unsafe {
        let agent = &mut *handle;
        let in_data = &*input;
        let out_data = agent.step(in_data);
        *output = out_data;
    }
}

/// Frees an allocated `SoarAadlAgent` handle over C-ABI.
///
/// # Safety
/// `handle` must have been returned by `soar_aadl_init` and not previously freed.
#[no_mangle]
pub extern "C" fn soar_aadl_free(handle: *mut SoarAadlAgent) {
    if !handle.is_null() {
        unsafe {
            let _ = alloc::boxed::Box::from_raw(handle);
        }
    }
}