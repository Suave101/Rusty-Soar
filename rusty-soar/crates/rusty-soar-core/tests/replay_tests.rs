//! Recorded input: each decision's values go on the input link before it runs, as JSoar's
//! `InputWme.update` puts them.

use rusty_soar_core::agent::{InputChange, InputRecording, InputReplay, RecordedValue, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;

#[test]
fn values_are_read_as_in_a_rule() {
    let recording = InputRecording::parse(
        "# a comment\ndecision 1\nf.a 3\nf.b 3.0\nf.c -1.5E2\nf.d |ok|\nf.e ok\nf.g |a \\| b|\nf.h |3|\nf.i -Infinity\ncommand rl --set learning on\ndecision\n",
    )
    .unwrap();
    assert_eq!(recording.decisions.len(), 2);
    let values: Vec<RecordedValue> = recording.decisions[0]
        .iter()
        .filter_map(|c| match c {
            InputChange::Set { value, .. } => Some(value.clone()),
            InputChange::Command(_) => None,
        })
        .collect();
    use RecordedValue::*;
    assert_eq!(values, vec![Int(3), Float(3.0), Float(-150.0), Str("ok".into()), Str("ok".into()), Str("a | b".into()), Str("3".into()), Float(f64::NEG_INFINITY)]);
    assert_eq!(recording.decisions[0].last(), Some(&InputChange::Command("rl --set learning on".into())));
    assert!(recording.decisions[1].is_empty());
}

#[test]
fn bad_lines_are_errors_with_their_line() {
    assert!(InputRecording::parse("f.a 3\n").unwrap_err().contains("line 1"));
    assert!(InputRecording::parse("decision\nf.a\n").unwrap_err().contains("line 2"));
    assert!(InputRecording::parse("decision\nf..a 1\n").is_err());
    assert!(InputRecording::parse("decision\nf.a |open\n").is_err());
}

/// A rule writes the airspeed whenever its WME is new: a repeated value is not a new WME, but
/// `0` after `0.0` is, since an integer is a different value from a float.
#[test]
fn a_value_changes_only_when_it_differs() {
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    let rule = "sp {speed (state <s> ^io.input-link.flightdata.airspeed <a>) --> (write |speed | <a> (crlf))}";
    agent.install_soar_script(&SoarScript::parse(rule).unwrap(), state).unwrap();
    let recording = InputRecording::parse(
        "decision\nflightdata.airspeed 0.0\ndecision\nflightdata.airspeed 0.0\ndecision\nflightdata.airspeed 0\ndecision\ncommand rl --set learning on\n",
    )
    .unwrap();
    let mut replay = InputReplay::new(recording);
    let mut written = Vec::new();
    while replay.apply_next(&mut agent).unwrap() {
        agent.run_decision_cycle(state);
        written.push(core::mem::take(&mut agent.output));
    }
    assert_eq!(written, vec!["speed 0.000000\n", "", "speed 0\n", ""]);
    assert!(agent.rl.params.learning, "the command ran");
    assert!(!replay.apply_next(&mut agent).unwrap(), "the recording is used up");
}

#[test]
fn a_path_is_either_a_value_or_an_identifier() {
    let mut agent = SoarAgent::new();
    agent.create_top_state();
    let mut replay = InputReplay::new(InputRecording::parse("decision\nf.a 1\ndecision\nf.a.b 2\n").unwrap());
    replay.apply_next(&mut agent).unwrap();
    assert!(replay.apply_next(&mut agent).unwrap_err().contains("f.a"));
}
