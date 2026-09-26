use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let attr_light = agent.symbols.intern_str("light");
    let val_green = agent.symbols.intern_str("green");

    let attr_status = agent.symbols.intern_str("status");
    let val_can_go = agent.symbols.intern_str("can-go");

    let attr_action = agent.symbols.intern_str("action");
    let val_accelerate = agent.symbols.intern_str("accelerate");

    // Rule 1 (Elaboration): If light == green => Assert status = can-go
    agent.add_rule(
        "elaborate-status-go",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_light),
                val: Some(val_green),
            },
            vec![],
        )],
        vec![Action::Add {
            id: s1,
            attr: attr_status,
            val: val_can_go,
        }],
    );

    // Rule 2 (Chained Elaboration): If status == can-go => Assert action = accelerate
    agent.add_rule(
        "elaborate-action-accelerate",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_status),
                val: Some(val_can_go),
            },
            vec![],
        )],
        vec![Action::Add {
            id: s1,
            attr: attr_action,
            val: val_accelerate,
        }],
    );

    println!("=== SOAR TRUTH MAINTENANCE SYSTEM (TMS) DEMO ===");

    // Step 1: Insert root WME (light = green)
    let root_wme_key = agent.insert_wme(s1, attr_light, val_green);

    // Step 2: Run proposal phase to fire chained elaboration rules
    agent.run_elaboration_phase();

    println!("Post-elaboration WME count: {}", agent.wm.len());
    assert_eq!(agent.wm.len(), 3, "Expected root WME + 2 derived elaboration WMEs");
    assert_eq!(agent.tms.active_justifications_count(), 2);

    // Step 3: Retract root WME (light = green) -> Cascading TMS retraction should remove all derived WMEs
    println!("\nRetracting root WME (light = green)...");
    agent.remove_wme(root_wme_key);

    println!("Post-retraction WME count: {}", agent.wm.len());
    assert_eq!(
        agent.wm.len(),
        0,
        "Cascading TMS retraction must remove all dependent I-supported WMEs!"
    );

    println!("\nTruth Maintenance System Cascading Retractions: VERIFIED SUCCESS!");
}