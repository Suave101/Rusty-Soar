use rusty_soar_core::aadl_bridge::agree_parser::{AgreeAnnex, AgreeVal, SymbolEnvironment};
use rusty_soar_core::aadl_bridge::{FlightTelemetryFFI, SoarAadlAgent};

struct SoarRuntimeEnvironment<'a> {
    telemetry: &'a FlightTelemetryFFI,
    command_op_id: u32,
    command_target_alt: f32,
    command_is_valid: bool,
}

impl<'a> SymbolEnvironment for SoarRuntimeEnvironment<'a> {
    fn lookup(&self, var_name: &str) -> Option<AgreeVal> {
        match var_name {
            "sensor_valid" => Some(AgreeVal::Bool(self.telemetry.sensor_valid)),
            "engine_status" => Some(AgreeVal::Int(self.telemetry.engine_status as i64)),
            "operator_id" => Some(AgreeVal::Int(self.command_op_id as i64)),
            "target_altitude_ft" => Some(AgreeVal::Float(self.command_target_alt)),
            "is_valid" => Some(AgreeVal::Bool(self.command_is_valid)),
            _ => None,
        }
    }
}

#[test]
fn test_dynamic_aadl_agree_verification() {
    let aadl_file_content = r#"
    package SoarAgentCore
    public
      system SoarCognitiveArchitecture
        annex agree {**
          assume "A01_SENSOR": sensor_valid = true;
          guarantee "G01_VALIDITY": is_valid = true;
          guarantee "G02_EMERGENCY": (engine_status = 2) => (operator_id = 99);
        **};
      end SoarCognitiveArchitecture;
    end SoarAgentCore;
    "#;

    // 1. Dynamically parse the AADL AGREE Annex
    let agree_contract = AgreeAnnex::parse_aadl_file(aadl_file_content).unwrap();

    // 2. Initialize Soar Agent and test inputs
    let mut agent = SoarAadlAgent::new();
    let telemetry = FlightTelemetryFFI {
        airspeed_kts: 150.0,
        altitude_ft: 3000.0,
        engine_status: 2, // Emergency state
        sensor_valid: true,
    };

    // 3. Step Soar agent decision cycle
    let cmd = agent.step(&telemetry);

    // 4. Bind runtime results to environment
    let env = SoarRuntimeEnvironment {
        telemetry: &telemetry,
        command_op_id: cmd.operator_id,
        command_target_alt: cmd.target_altitude_ft,
        command_is_valid: cmd.is_valid,
    };

    // 5. Verify that Soar output satisfies dynamically parsed AADL guarantees
    let result = agree_contract.verify_guarantees(&env);
    assert!(
        result.is_ok(),
        "Soar Agent engine state failed dynamic AADL contract verification: {:?}",
        result
    );
}
