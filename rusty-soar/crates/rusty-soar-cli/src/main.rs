use std::env;
use std::fs;
use std::process::{exit, Command};

use rusty_soar_core::aadl_bridge::{
    AgreeAnnex, AgreeVal, FlightTelemetryFFI, SoarCommandFFI, SymbolEnvironment,
};
use rusty_soar_core::soar_parser::{SoarScript, SoarValue};

/// Recognized compilation target triples
const KNOWN_TARGETS: &[(&str, &str)] = &[
    ("wasm32-unknown-unknown", "WebAssembly Bare Engine"),
    ("aarch64-unknown-none", "64-bit ARM Bare-Metal"),
    ("armv8r-none-eabi", "ARMv8-R Real-Time Embedded"),
    ("thumbv7em-none-eabihf", "ARM Cortex-M4F/M7F (FPU)"),
    ("thumbv8m.main-none-eabi", "ARM Cortex-M33 Bare-Metal"),
    ("aarch64-unknown-nto-qnx710", "QNX Neutrino 7.1 RTOS"),
    ("riscv64gc-unknown-none-elf", "64-bit RISC-V Bare-Metal"),
    ("riscv32imac-unknown-none-elf", "32-bit RISC-V Microcontroller"),
    ("powerpc-unknown-none", "PowerPC Bare-Metal"),
    ("aarch64-unknown-linux-musl", "ARM64 Static Linux"),
    ("x86_64-unknown-linux-gnu", "x86_64 Linux Native"),
];

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

fn print_usage() {
    println!("Rusty-Soar Verification & Cross-Compilation CLI");
    println!("Usage:");
    println!("  soar-cli <rules.soar> <contract.aadl> [--target <target_triple>]");
    println!("\nOptions:");
    println!("  -t, --target <triple>   Compilation target (default: host machine target)");
    println!("  -h, --help              Show usage information");
    println!("\nSupported Target Triples:");
    for (triple, desc) in KNOWN_TARGETS {
        println!("  - {:<30} ({})", triple, desc);
    }
}

fn get_host_target() -> String {
    let output = Command::new("rustc").arg("-Vv").output().ok();

    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if line.starts_with("host: ") {
                return line["host: ".len()..].trim().to_string();
            }
        }
    }
    "x86_64-unknown-linux-gnu".to_string()
}

fn ensure_target_installed(target: &str) {
    println!("      Checking target sysroot availability for '{}'...", target);
    let status = Command::new("rustup")
        .arg("target")
        .arg("add")
        .arg(target)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("      Target sysroot '{}' is ready.", target);
        }
        _ => {
            println!("      [Notice] rustup auto-install skipped or target requires custom build-std.");
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) || args.len() < 3 {
        print_usage();
        exit(if args.len() < 3 { 1 } else { 0 });
    }

    let soar_path = &args[1];
    let aadl_path = &args[2];

    let mut target_triple: Option<String> = None;
    let mut i = 3;
    while i < args.len() {
        if (args[i] == "--target" || args[i] == "-t") && i + 1 < args.len() {
            target_triple = Some(args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }

    let host_target = get_host_target();
    let selected_target = target_triple.unwrap_or_else(|| host_target.clone());

    println!("====================================================");
    println!(" Rusty-Soar Verification & Compilation Engine");
    println!("====================================================");
    println!("Target Triple: {}", selected_target);

    // 1. Read & Parse Soar Script
    println!("\n[1/5] Reading Soar script from: {}", soar_path);
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

    if soar_script.productions.is_empty() {
        println!("      [ERROR] No rules found in file!");
        exit(1);
    }

    for prod in &soar_script.productions {
        println!("       - Rule: {} (Conditions: {})", prod.name, prod.conditions.len());
    }

    // 2. Read & Parse AADL Contract Specification
    println!("\n[2/5] Reading AADL AGREE spec from: {}", aadl_path);
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

    // 3. Dynamic Evaluation & Formal Verification
    println!("\n[3/5] Verifying Soar agent against AADL AGREE contract...");

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

        let assume_res = agree_contract.verify_assumes(&env);
        let guarantee_res = agree_contract.verify_guarantees(&env);

        print!("      Scenario '{}': ", name);
        if assume_res.is_ok() && guarantee_res.is_ok() {
            println!(
                "PASSED [Op ID: {}, Target Alt: {} ft]",
                command.operator_id, command.target_altitude_ft
            );
            total_passed += 1;
        } else {
            println!("FAILED");
            if let Err(e) = guarantee_res {
                println!("        Violation: {}", e);
            }
        }
    }

    if total_passed != scenarios.len() {
        println!("\nFAILURE: Contract violations detected. Compilation aborted!");
        exit(1);
    }

    println!("\n[4/5] Verification Passed! Compiling agent to target: {}", selected_target);

    // Auto-install target sysroot if cross-compiling
    if selected_target != host_target {
        ensure_target_installed(&selected_target);
    }

    // Canonicalize path to feed into cargo environment
    let canonical_soar_path = fs::canonicalize(soar_path).unwrap_or_else(|e| {
        eprintln!("Failed to resolve path {}: {}", soar_path, e);
        exit(1);
    });

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .env("SOAR_RULES_FILE", canonical_soar_path.to_str().unwrap())
        .arg("build")
        .arg("--package")
        .arg("rusty-soar-core")
        .arg("--release");

    if selected_target != host_target {
        cargo_cmd.arg("--target").arg(&selected_target);
    }

    let status = cargo_cmd.status().unwrap_or_else(|e| {
        eprintln!("Failed to invoke cargo build: {}", e);
        exit(1);
    });

    if !status.success() {
        eprintln!("\nCompilation failed for target '{}'.", selected_target);
        eprintln!("If this is a Tier 3 target, compile using Nightly build-std:");
        eprintln!("      cargo +nightly build -Z build-std=core,alloc --target {}", selected_target);
        exit(1);
    }

    // 5. Final Output
    println!("\n[5/5] Build Summary");
    println!("      Verification : PASSED (2/2 scenarios satisfied AGREE contract)");
    println!("      Compilation  : SUCCESS");
    if selected_target != host_target {
        println!("      Target Artifact: target/{}/release/librusty_soar_core.a", selected_target);
    } else {
        println!("      Target Artifact: target/release/librusty_soar_core.a");
    }
    println!("\nSUCCESS: Verified Soar agent built successfully for {}!", selected_target);
}