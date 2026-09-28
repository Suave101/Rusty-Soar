#![cfg(kani)]

use rusty_soar_core::aadl_bridge::{
    AgreeAnnex, AgreeVal, FlightTelemetryFFI, SoarAadlAgent, SymbolEnvironment,
};

struct KaniSymbolEnv<'a> {
    telemetry: &'a FlightTelemetryFFI,
    command_op_id: u32,
    command_is_valid: bool,
}

impl<'a> SymbolEnvironment for KaniSymbolEnv<'a> {
    fn lookup(&self, var_name: &str) -> Option<AgreeVal> {
        match var_name {
            "sensor_valid" => Some(AgreeVal::Bool(self.telemetry.sensor_valid)),
            "engine_status" => Some(AgreeVal::Int(self.telemetry.engine_status as i64)),
            "operator_id" => Some(AgreeVal::Int(self.command_op_id as i64)),
            "is_valid" => Some(AgreeVal::Bool(self.command_is_valid)),
            _ => None,
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
pub fn verify_aadl_agree_contract_equivalence() {
    // 1. Generate symbolic input telemetry
    let telemetry = FlightTelemetryFFI {
        airspeed_kts: kani::any(),
        altitude_ft: kani::any(),
        engine_status: kani::any(),
        sensor_valid: kani::any(),
    };

    // 2. Enforce AGREE Assumption: assume "A01_SENSOR": sensor_valid = true
    kani::assume(telemetry.sensor_valid == true);

    // 3. Step Soar agent decision cycle
    let mut agent = SoarAadlAgent::new();
    let cmd = agent.step(&telemetry);

    // 4. Construct verification environment
    let env = KaniSymbolEnv {
        telemetry: &telemetry,
        command_op_id: cmd.operator_id,
        command_is_valid: cmd.is_valid,
    };

    // 5. Assert AGREE Guarantee Compliance
    // G01_VALIDITY: is_valid = true
    assert!(env.command_is_valid == true);

    // G02_EMERGENCY: (engine_status = 2) => (operator_id = 99)
    if env.telemetry.engine_status == 2 {
        assert_eq!(env.command_op_id, 99);
    }
}
