use rusty_soar_core::agent::{Action, SoarAgent};
use rusty_soar_core::preference::{Preference, PreferenceType};
use rusty_soar_core::rete::AlphaTest;

fn main() {
    let mut agent = SoarAgent::new();

    let s1 = agent.symbols.intern_id('S', 1);
    let attr_location = agent.symbols.intern_str("location");
    let val_sector_a = agent.symbols.intern_str("sector-a");
    let val_sector_b = agent.symbols.intern_str("sector-b");

    let o1_move = agent.symbols.intern_id('O', 1);

    // Rule: Move operator proposal
    agent.add_rule(
        "propose-move",
        vec![(
            AlphaTest {
                id: Some(s1),
                attr: Some(attr_location),
                val: None,
            },
            vec![],
        )],
        vec![Action::Prefer(Preference {
            state: s1,
            operator: o1_move,
            preference_type: PreferenceType::Acceptable,
        })],
    );

    println!("=== SOAR EPISODIC MEMORY (EPMEM) DEMO ===");

    // Decision Cycle 1: Agent is at Sector A
    let wme1 = agent.insert_wme(s1, attr_location, val_sector_a);
    agent.run_decision_cycle(s1);
    println!("Cycle 1 recorded. Agent location: Sector A");

    // Decision Cycle 2: Agent moves to Sector B
    agent.remove_wme(wme1);
    let _wme2 = agent.insert_wme(s1, attr_location, val_sector_b);
    agent.run_decision_cycle(s1);
    println!("Cycle 2 recorded. Agent location: Sector B");

    assert_eq!(agent.epmem.len(), 2);

    // Query Episodic Memory for cue: "When was the agent at Sector A?"
    println!("\nQuerying EpMem for cue: location = sector-a...");
    let query_cue = vec![(s1, attr_location, val_sector_a)];
    let recalled_ep_id = agent.epmem.query(&query_cue);

    assert!(recalled_ep_id.is_some());
    let episode = agent.epmem.retrieve(recalled_ep_id.unwrap()).unwrap();

    println!(
        "EpMem Recall Successful! Recalled Episode ID: {:?}, Time Step: {}",
        episode.id, episode.time_step
    );
    assert_eq!(episode.time_step, 1);

    println!("\nEpisodic Memory Snapshot Recording & Recall: VERIFIED SUCCESS!");
}