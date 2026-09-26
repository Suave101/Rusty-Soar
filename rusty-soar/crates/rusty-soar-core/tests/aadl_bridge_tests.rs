use rusty_soar_core::aadl_bridge::{FlightTelemetryFFI, SoarAadlAgent};

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
    assert_eq!(cmd_nominal.operator_id, 10, "Should select Cruise Operator (10)");
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
    assert_eq!(cmd_failure.operator_id, 99, "Should select Emergency Landing Operator (99)");
    assert_eq!(cmd_failure.target_altitude_ft, 0.0, "Target altitude should be reset to ground level");
}