use rusty_soar_core::agent::SoarAgent;
use rusty_soar_core::rete::{AlphaTest, Field, VariableBinding};

fn main() {
    let mut agent = SoarAgent::new();

    // 1. Intern symbols
    let s1 = agent.symbols.intern_id('S', 1);
    let attr_type = agent.symbols.intern_str("type");
    let attr_status = agent.symbols.intern_str("status");
    let val_sensor = agent.symbols.intern_str("sensor");
    let val_active = agent.symbols.intern_str("active");

    // 2. Define a 2-Condition Rule with a Variable Join across WMEs:
    //    IF (S1 ^type sensor) AND (S1 ^status active)
    //    Condition 0: (S1 ^type sensor)
    //    Condition 1: (<id> ^status active) WHERE <id> == Condition 0's ID
    let rule_conditions = vec![
        (
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_type),
                val: Some(val_sensor),
            },
            vec![], // No joins on first condition
        ),
        (
            AlphaTest {
                id: None, // Any ID allowed
                attr: Some(attr_status),
                val: Some(val_active),
            },
            vec![VariableBinding {
                token_wme_index: 0, // Match against Condition 0
                token_field: Field::Id,
                wme_field: Field::Id, // Cond 1 ID must equal Cond 0 ID
            }],
        ),
    ];

    agent.rete.add_rule("detect-active-sensor", rule_conditions);

    println!("Initial activations: {}", agent.rete.activations.len());

    // 3. Insert WME 1
    println!("Inserting WME 1: (S1 ^type sensor)");
    agent.insert_wme(s1, attr_type, val_sensor);
    println!("Activations after WME 1: {}", agent.rete.activations.len());

    // 4. Insert WME 2 (Completes the Beta Join)
    println!("Inserting WME 2: (S1 ^status active)");
    agent.insert_wme(s1, attr_status, val_active);

    println!("\n--- RETE Match Complete ---");
    println!("Total Activations: {}", agent.rete.activations.len());
    for inst in &agent.rete.activations {
        println!("Fired Production: '{}'", inst.rule_name);
        println!("Matched WME Count: {}", inst.matched_wmes.len());
    }
}
