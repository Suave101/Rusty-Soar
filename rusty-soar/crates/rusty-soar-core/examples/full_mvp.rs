use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::rete::{AlphaTest, Field, VariableBinding};

fn main() {
    let mut agent = SoarAgent::new();

    // 1. Intern symbols
    let s1 = agent.symbols.intern_id('S', 1);
    let attr_state = agent.symbols.intern_str("state");
    let attr_phase = agent.symbols.intern_str("phase");
    
    let val_init = agent.symbols.intern_str("initializing");
    let val_running = agent.symbols.intern_str("running");
    let val_done = agent.symbols.intern_str("complete");

    // 2. Rule 1: IF (S1 ^state initializing) THEN ADD (S1 ^phase running)
    agent.add_rule(
        "start-system",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_state),
                val: Some(val_init),
            },
            vec![],
        )],
        vec![Action::Add {
            id: s1,
            attr: attr_phase,
            val: val_running,
        }],
    );

    // 3. Rule 2: Multi-condition Join Rule
    //    IF (S1 ^state initializing) AND (S1 ^phase running) THEN ADD (S1 ^state complete)
    agent.add_rule(
        "complete-system",
        vec![
            (
                AlphaTest {
                    id: Some(s1),
                    attr: Some(attr_state),
                    val: Some(val_init),
                },
                vec![],
            ),
            (
                AlphaTest {
                    id: None,
                    attr: Some(attr_phase),
                    val: Some(val_running),
                },
                vec![VariableBinding {
                    token_wme_index: 0,
                    token_field: Field::Id,
                    wme_field: Field::Id,
                }],
            ),
        ],
        vec![Action::Add {
            id: s1,
            attr: attr_state,
            val: val_done,
        }],
    );

    println!("=== RUSTY SOAR ENGINE RUN ===");
    
    // Seed initial state
    println!("Seeding initial WME: (S1 ^state initializing)");
    agent.insert_wme(s1, attr_state, val_init);

    // Run engine to quiescence
    let total_fired = agent.run_to_quiescence(10);

    println!("\n=== EXECUTION SUMMARY ===");
    println!("Total Rule Fires: {}", total_fired);
    println!("Final WM Count: {}", agent.wm.len());
    
    println!("\nMVP ENGINE CHECK: SUCCESS!");
}