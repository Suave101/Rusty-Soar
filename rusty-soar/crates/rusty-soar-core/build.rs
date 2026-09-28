fn main() {
    if let Ok(path) = std::env::var("SOAR_RULES_FILE") {
        println!("cargo:rerun-if-changed={}", path);
        println!("cargo:rustc-env=SOAR_RULES_FILE={}", path);
    } else {
        // Fallback default contained within the crate root
        let default_path = concat!(env!("CARGO_MANIFEST_DIR"), "/rules/default_flight_control.soar");
        println!("cargo:rerun-if-changed={}", default_path);
        println!("cargo:rustc-env=SOAR_RULES_FILE={}", default_path);
    }
}