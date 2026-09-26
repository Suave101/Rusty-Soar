//! C-ABI FFI Bridge implementing AADL/AGREE contract bindings for rusty-soar.

use crate::agent::{Action, SoarAgent};
use crate::preference::{Preference, PreferenceType};
use crate::rete::AlphaTest;
use crate::symbol::SymbolId;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FlightTelemetryFFI {
    pub airspeed_kts: f32,
    pub altitude_ft: f32,
    pub engine_status: u8, // 0: Normal, 1: Degradation, 2: Critical
    pub sensor_valid: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct OperatorCommandFFI {
    pub operator_id: u32,
    pub target_heading_deg: f32,
    pub target_altitude_ft: f32,
    pub is_valid: bool,
}

/// FFI State Container for AADL Periodic Executable Execution
pub struct SoarAadlAgent {
    pub agent: SoarAgent,
    pub state_id: SymbolId,
    pub attr_engine: SymbolId,
    pub val_normal: SymbolId,
    pub val_critical: SymbolId,
    pub op_cruise: SymbolId,
    pub op_emergency: SymbolId,
}

impl SoarAadlAgent {
    pub fn new() -> Self {
        let mut agent = SoarAgent::new();

        let state_id = agent.symbols.intern_id('S', 1);
        let attr_engine = agent.symbols.intern_str("engine_status");
        let val_normal = agent.symbols.intern_str("normal");
        let val_critical = agent.symbols.intern_str("critical");

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

        // Production Rule 2: Propose Emergency Land Operator under critical engine state
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
            alloc::vec![Action::Prefer(Preference {
                state: state_id,
                operator: op_emergency,
                preference_type: PreferenceType::Acceptable,
            })],
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

    /// Step execution matching the AADL 10ms periodic compute frame
    pub fn step(&mut self, input: &FlightTelemetryFFI) -> OperatorCommandFFI {
        if !input.sensor_valid {
            return OperatorCommandFFI {
                operator_id: 0,
                target_heading_deg: 0.0,
                target_altitude_ft: 0.0,
                is_valid: false,
            };
        }

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

// Exported C Functions for AADL / OSATE Runtime Execution
#[no_mangle]
pub extern "C" fn soar_aadl_init() -> *mut SoarAadlAgent {
    let agent = alloc::boxed::Box::new(SoarAadlAgent::new());
    alloc::boxed::Box::into_raw(agent)
}

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