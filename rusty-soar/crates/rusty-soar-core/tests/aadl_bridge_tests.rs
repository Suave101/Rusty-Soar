use rusty_soar_core::aadl_bridge::{
    AgreeAnnex, AgreeVal, FlightTelemetryFFI, SoarAadlAgent, SymbolEnvironment,
};

struct TestEnvironment;

impl SymbolEnvironment for TestEnvironment {
    fn lookup(&self, name: &str) -> Option<AgreeVal> {
        match name {
            "sensor_valid" => Some(AgreeVal::Bool(true)),
            "airspeed_kts" => Some(AgreeVal::Float(120.0)),
            "altitude_ft" => Some(AgreeVal::Float(2500.0)),
            _ => None,
        }
    }
}

struct FullContractEnvironment;

impl SymbolEnvironment for FullContractEnvironment {
    fn lookup(&self, name: &str) -> Option<AgreeVal> {
        match name.rsplit('.').next().unwrap_or(name) {
            "sensor_valid" => Some(AgreeVal::Bool(true)),
            "airspeed_kts" => Some(AgreeVal::Float(220.0)),
            "altitude_ft" => Some(AgreeVal::Float(10000.0)),
            "engine_status" => Some(AgreeVal::Int(0)),
            "operator_id" => Some(AgreeVal::Int(10)),
            "target_altitude_ft" => Some(AgreeVal::Float(10000.0)),
            "is_valid" => Some(AgreeVal::Bool(true)),
            "OPERATOR_CRUISE_ID" => Some(AgreeVal::Int(10)),
            "OPERATOR_EMERGENCY_LAND_ID" => Some(AgreeVal::Int(99)),
            "ENGINE_STATUS_NORMAL" => Some(AgreeVal::Int(0)),
            "ENGINE_STATUS_CRITICAL" => Some(AgreeVal::Int(2)),
            _ => None,
        }
    }
}

#[test]
fn test_aadl_bridge_nominal_and_failure_response() {
    let mut agent = SoarAadlAgent::new();

    // Test Case 1: Nominal Flight Telemetry
    let nominal_input = FlightTelemetryFFI {
        airspeed_kts: 120.0,
        altitude_ft: 2500.0,
        engine_status: 0, // Normal
        sensor_valid: true,
    };

    let cmd_nominal = agent.step(&nominal_input);
    assert!(cmd_nominal.is_valid);
    assert_eq!(
        cmd_nominal.operator_id, 10,
        "Should select Cruise Operator (10)"
    );
    assert_eq!(cmd_nominal.target_altitude_ft, 2500.0);

    // Test Case 2: Critical Engine Failure Telemetry
    let failure_input = FlightTelemetryFFI {
        airspeed_kts: 110.0,
        altitude_ft: 2500.0,
        engine_status: 2, // Critical Failure
        sensor_valid: true,
    };

    let cmd_failure = agent.step(&failure_input);
    assert!(cmd_failure.is_valid);
    assert_eq!(
        cmd_failure.operator_id, 99,
        "Should select Emergency Landing Operator (99)"
    );
    assert_eq!(
        cmd_failure.target_altitude_ft, 0.0,
        "Target altitude should be reset to ground level"
    );
}

#[test]
fn test_agree_comparisons_and_boolean_operators() {
    let contract = AgreeAnnex::parse_aadl_file(
        r#"
                package Test
                public
                    system Controller
                        annex agree {**
                            assume "input" : sensor_valid == true and airspeed_kts >= 0.0;
                            guarantee "altitude" : altitude_ft <= 50000.0;
                        **};
                    end Controller;
                end Test;
                "#,
    )
    .expect("contract should parse");

    contract
        .verify_assumes(&TestEnvironment)
        .expect("assumption should hold");
    contract
        .verify_guarantees(&TestEnvironment)
        .expect("guarantee should hold");
}

#[test]
fn test_checked_in_aadl_contract_parses_and_verifies() {
    let source = include_str!("../../../../aadl/RustySoarAgent.aadl");
    let contract = AgreeAnnex::parse_aadl_file(source).expect("AADL contract should parse");
    for assumption in &contract.assumes {
        AgreeAnnex::eval_expr(&assumption.expr, &FullContractEnvironment)
            .unwrap_or_else(|error| panic!("assumption {} failed: {}", assumption.tag, error));
    }
    contract
        .verify_assumes(&FullContractEnvironment)
        .expect("nominal assumptions should hold");
    contract
        .verify_guarantees(&FullContractEnvironment)
        .expect("nominal guarantees should hold");
}
