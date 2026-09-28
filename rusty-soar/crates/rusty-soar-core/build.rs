use std::env;
use std::fs;
use std::path::PathBuf;

const DEFAULT_SOAR_RULES: &str = r#"
sp {propose*emergency*descent
   (state <s> ^io.input-link.telemetry <t>)
   (<t> ^engine_status 2)
   -->
   (<s> ^operator <o> +)
   (<o> ^name emergency-descent
        ^operator_id 99
        ^target_altitude_ft 0)
}

sp {propose*maintain*cruise
   (state <s> ^io.input-link.telemetry <t>)
   (<t> ^engine_status 0)
   -->
   (<s> ^operator <o> +)
   (<o> ^name maintain-cruise
        ^operator_id 10
        ^target_altitude_ft 10000)
}
"#;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR env var missing");
    let default_rules_path = PathBuf::from(&out_dir).join("default_rules.soar");

    if let Ok(user_path) = env::var("SOAR_RULES_FILE") {
        if std::path::Path::new(&user_path).exists() {
            println!("cargo:rerun-if-changed={}", user_path);
            println!("cargo:rustc-env=SOAR_RULES_FILE={}", user_path);
            return;
        }
    }

    // Automatically generate fallback rules into OUT_DIR during build/publish
    fs::write(&default_rules_path, DEFAULT_SOAR_RULES)
        .expect("Failed to write default Soar rules to OUT_DIR");

    println!("cargo:rerun-if-changed={}", default_rules_path.display());
    println!("cargo:rustc-env=SOAR_RULES_FILE={}", default_rules_path.display());
}