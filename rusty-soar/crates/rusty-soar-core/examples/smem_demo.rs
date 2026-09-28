use rusty_soar_core::agent::SoarAgent;

fn main() {
    let mut agent = SoarAgent::new();

    // Intern symbols for Semantic Memory entries
    let attr_name = agent.symbols.intern_str("name");
    let attr_category = agent.symbols.intern_str("category");
    let attr_destination = agent.symbols.intern_str("destination");

    let val_rover = agent.symbols.intern_str("curiosity-rover");
    let val_vehicle = agent.symbols.intern_str("autonomous-vehicle");
    let val_mars = agent.symbols.intern_str("mars-crater");

    // Populate Semantic Memory (SMem) with long-term declarative facts
    let lti1 = agent.smem.create_lti();
    agent.smem.store_fact(lti1, attr_name, val_rover);
    agent.smem.store_fact(lti1, attr_category, val_vehicle);
    agent.smem.store_fact(lti1, attr_destination, val_mars);

    println!("=== SOAR SEMANTIC MEMORY (SMEM) DEMO ===");
    println!("Stored LTI {:?} into Semantic Memory with 3 facts.", lti1);

    // Working Memory target container
    let s1 = agent.symbols.intern_id('S', 1);

    // Query Semantic Memory by cue `name == curiosity-rover` and retrieve into Working Memory
    println!("\nQuerying SMem for cue: name = 'curiosity-rover'...");
    let retrieved_lti = agent.retrieve_smem_to_wm(attr_name, val_rover, s1);

    assert_eq!(retrieved_lti, Some(lti1));
    println!(
        "SMem Query successful! Retrieved LTI: {:?}",
        retrieved_lti.unwrap()
    );

    println!(
        "Working Memory element count post-retrieval: {}",
        agent.wm.len()
    );
    assert_eq!(
        agent.wm.len(),
        3,
        "Retrieved facts must be placed in Working Memory"
    );

    println!("\nSemantic Memory Query & Retrieval: VERIFIED SUCCESS!");
}
