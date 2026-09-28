use std::env;
use std::fs;
use std::process::exit;

use rusty_soar_core::aadl_bridge::{
    AgreeAnnex, AgreeVal, FlightTelemetryFFI, SoarCommandFFI, SymbolEnvironment,
};
use rusty_soar_core::soar_parser::{SoarScript, SoarValue};

struct CliSymbolEnv<'a> {
    telemetry: &'a FlightTelemetryFFI,
    command: &'a SoarCommandFFI,
}

impl<'a> SymbolEnvironment for CliSymbolEnv<'a> {
    fn lookup(&self, var_name: &str) -> Option<AgreeVal> {
        match var_name {
            "sensor_valid" => Some(AgreeVal::Bool(self.telemetry.sensor_valid)),
            "engine_status" => Some(AgreeVal::Int(self.telemetry.engine_status as i64)),
            "altitude_ft" => Some(AgreeVal::Float(self.telemetry.altitude_ft)),
            "operator_id" => Some(AgreeVal::Int(self.command.operator_id as i64)),
            "target_altitude_ft" => Some(AgreeVal::Float(self.command.target_altitude_ft)),
            "is_valid" => Some(AgreeVal::Bool(self.command.is_valid)),
            _ => None,
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Rusty-Soar Script Verification CLI");
        println!("Usage: soar-cli <rules.soar> <contract.aadl>");
        exit(1);
    }

    let soar_path = &args[1];
    let aadl_path = &args[2];

    println!("====================================================");
    println!(" Rusty-Soar Verification Engine");
    println!("====================================================");

    // 1. Read & Parse Soar Script
    println!("[1/4] Reading Soar script from: {}", soar_path);
    let soar_source = fs::read_to_string(soar_path).unwrap_or_else(|e| {
        eprintln!("Error reading Soar file: {}", e);
        exit(1);
    });

    let soar_script = SoarScript::parse(&soar_source).unwrap_or_else(|e| {
        eprintln!("Parse Error in Soar Script: {}", e);
        exit(1);
    });

    println!(
        "      Successfully parsed {} Soar production rules:",
        soar_script.productions.len()
    );
    for prod in &soar_script.productions {
        println!("       - Rule: {}", prod.name);
    }

    // 2. Read & Parse AADL Contract Specification
    println!("\n[2/4] Reading AADL AGREE spec from: {}", aadl_path);
    let aadl_source = fs::read_to_string(aadl_path).unwrap_or_else(|e| {
        eprintln!("Error reading AADL file: {}", e);
        exit(1);
    });

    let agree_contract = AgreeAnnex::parse_aadl_file(&aadl_source).unwrap_or_else(|e| {
        eprintln!("Parse Error in AADL File: {}", e);
        exit(1);
    });

    println!("      Parsed AGREE Contract:");
    println!("       - Assumes: {}", agree_contract.assumes.len());
    println!("       - Guarantees: {}", agree_contract.guarantees.len());

    // 3. Dynamic Evaluation Scenarios
    println!("\n[3/4] Running dynamic rule execution and verification...");

    let scenarios = vec![
        ("Nominal Cruise", FlightTelemetryFFI {
            airspeed_kts: 220.0,
            altitude_ft: 10000.0,
            engine_status: 0,
            sensor_valid: true,
        }),
        ("Emergency Fault", FlightTelemetryFFI {
            airspeed_kts: 180.0,
            altitude_ft: 15000.0,
            engine_status: 2,
            sensor_valid: true,
        }),
    ];

    let mut total_passed = 0;

    for (name, telemetry) in &scenarios {
        // Execute Parsed Script
        let mut matched_op_id = 0u32;
        let mut matched_target_alt = telemetry.altitude_ft;

        for prod in &soar_script.productions {
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

        let command = SoarCommandFFI {
            operator_id: matched_op_id,
            target_altitude_ft: matched_target_alt,
            is_valid: telemetry.sensor_valid,
        };

        let env = CliSymbolEnv {
            telemetry,
            command: &command,
        };

        // Validate Contract
        let assume_res = agree_contract.verify_assumes(&env);
        let guarantee_res = agree_contract.verify_guarantees(&env);

        print!("      Scenario '{}': ", name);
        if assume_res.is_ok() && guarantee_res.is_ok() {
            println!("PASSED [Op ID: {}, Target Alt: {} ft]", command.operator_id, command.target_altitude_ft);
            total_passed += 1;
        } else {
            println!("FAILED");
            if let Err(e) = guarantee_res {
                println!("        Violation: {}", e);
            }
        }
    }

    // 4. Final Verdict Output
    println!("\n[4/4] Verification Summary");
    println!("      Result: {}/{} scenarios passed verification.", total_passed, scenarios.len());

    if total_passed == scenarios.len() {
        println!("\nSUCCESS: Soar script satisfies all AADL AGREE guarantees!");
    } else {
        println!("\nFAILURE: Contract violations detected.");
        exit(1);
    }
}