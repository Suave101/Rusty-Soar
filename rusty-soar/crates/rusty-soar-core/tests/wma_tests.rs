//! Working-memory activation, against upstream's `WmaFunctionalTests` and the episodic-memory
//! tests that use it (`EpMemFunctionalTests`).

use rusty_soar_core::agent::{Phase, SoarAgent};
use rusty_soar_core::soar_parser::SoarScript;
use rusty_soar_core::symbol::SymbolId;

/// `%0.2g`, as upstream's `print -i` writes an activation.
fn two_significant(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let exponent = x.abs().log10().floor() as i32;
    let decimals = (1 - exponent).max(0) as usize;
    let text = format!("{:.*}", decimals, x);
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').into() } else { text }
}

/// Upstream's `FunctionalTestHarness::runTest`: runs one more decision than expected, and the
/// agent must have halted after exactly `expected` decision phases. The harness's `succeeded`
/// and `failed` functions halt; here they are `halt`s with the outcome written.
fn run_test(source: &str, expected: usize) -> (SoarAgent, SymbolId) {
    let source = source.replace("(succeeded)", "(halt)").replace("(failed)", "(write |FAILED|) (halt)");
    let script = SoarScript::parse(&source).expect("parses");
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&script, state).expect("installs");
    let mut decision_phases = 0;
    for _ in 0..=expected {
        agent.run_decision_cycle(state);
        // A halt in the propose phase stops the cycle before its decision phase.
        if !(agent.halted && agent.current_phase == Phase::Proposal) {
            decision_phases += 1;
        }
        if agent.halted {
            break;
        }
    }
    assert!(agent.halted, "did not halt:\n{}", agent.output);
    assert!(!agent.output.contains("FAILED") && !agent.output.contains("Failed"), "{}", agent.output);
    assert_eq!(decision_phases, expected, "{}", agent.output);
    (agent, state)
}

/// Upstream `testSimpleActivation`: the agent halts once `^o-from-i2` decays below threshold
/// and is forgotten, after 2679 decisions, and `print s1 -i` shows the activations.
#[test]
fn simple_activation_forgets_after_2679_decisions() {
    let (mut agent, state) = run_test(include_str!("agents/wma-simple-activation.soar"), 2679);
    assert!(agent.output.contains("success"));
    let mut activation = |attr: &str| {
        let attr = agent.symbols.intern_str(attr);
        let key = agent.wm.iter().find(|w| w.id == state && w.attr == attr).map(|w| w.key)?;
        agent.wme_activation(key, true).map(two_significant)
    };
    assert_eq!(activation("o-from-a").as_deref(), Some("-1.5"));
    assert_eq!(activation("o-from-o").as_deref(), Some("-1.9"));
    assert_eq!(activation("i-from-i").as_deref(), Some("1"));
    assert_eq!(activation("o-from-i2"), None, "forgotten");
}

#[test]
fn epmem_query_with_balance_0_uses_activation() {
    run_test(include_str!("agents/epmem-testWMActivation_Balance0.soar"), 5);
}

#[test]
fn epmem_encodes_at_output_with_and_without_activation() {
    run_test(include_str!("agents/epmem-testEpMemEncodeOutput_WMA.soar"), 4);
    run_test(include_str!("agents/epmem-testEpMemEncodeOutput_NoWMA.soar"), 4);
}

#[test]
fn epmem_encodes_at_selection_with_and_without_activation() {
    run_test(include_str!("agents/epmem-testEpMemEncodeSelection_WMA.soar"), 5);
    run_test(include_str!("agents/epmem-testEpMemEncodeSelection_NoWMA.soar"), 5);
}

#[test]
fn wma_settings_are_protected_while_on() {
    let script = SoarScript::parse("wma --set activation on\nwma --set decay-rate 0.8\nwma --get decay-rate\nwma --set activation off\nwma --set decay-rate 0.8\nwma -g decay-rate\n").unwrap();
    let mut agent = SoarAgent::new();
    let (state, _) = agent.create_top_state();
    agent.install_soar_script(&script, state).unwrap();
    assert_eq!(agent.output, "ERROR: this parameter is protected while WMA is on.\n-0.5-0.8");
}
