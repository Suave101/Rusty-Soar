//! Runs the river-crossing example (examples/river-crossing) on its recorded weather.

use std::path::Path;
use std::process::Command;

#[test]
fn river_crossing_learns_the_shortest_trip() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/river-crossing");
    if !example.exists() {
        // The example isn't in the published crate.
        return;
    }
    let output = Command::new(env!("CARGO_BIN_EXE_rusty-soar-cli"))
        .arg("trace")
        .arg(example.join("load.soar"))
        .args(["--seed", "1", "--input"])
        .arg(example.join("weather.rec"))
        .output()
        .expect("runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let trace = String::from_utf8(output.stdout).expect("utf-8");
    assert!(!trace.contains("==>S: S2"), "no impasses");
    assert!(!trace.contains("Warning"), "no warnings");

    let trips: Vec<u32> = trace
        .lines()
        .filter_map(|line| line.strip_prefix("trip "))
        .map(|line| line.split(": ").nth(1).and_then(|rest| rest.split(' ').next()).unwrap().parse().unwrap())
        .collect();
    assert!(trips.len() > 100, "{} trips", trips.len());
    assert!(trips[0] > 11, "the first trip explores");
    // The recording turns learning and exploration off at decision 2200; every trip after that is
    // the shortest, 11 crossings.
    assert!(trips[trips.len() - 20..].iter().all(|&n| n == 11), "{:?}", &trips[trips.len() - 20..]);
}
