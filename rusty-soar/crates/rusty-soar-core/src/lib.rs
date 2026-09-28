//! RustySoar Core Engine
#![no_std]
#![deny(missing_docs)]

extern crate alloc;

/// Agent implementation coordinating the Soar decision cycle and memory modules.
pub mod agent;
/// Episodic memory system for temporal snapshot recording and retrieval.
pub mod epmem;
/// Architectural impasse types and substate handling.
pub mod impasse;
/// Explanation-Based Learning (Chunking) for rule synthesis.
pub mod learning;
/// Preference resolution semantics for candidate operator selection.
pub mod preference;
/// Production rule structures and action definitions.
pub mod production;
/// Index-based RETE pattern matching engine.
pub mod rete;
/// Reinforcement Learning (RL) mechanism for reward-based numeric preference updates.
pub mod rl;
/// Semantic memory system for long-term factual knowledge storage and retrieval.
pub mod smem;
/// Global symbol interning table and identifier system.
pub mod symbol;
/// Truth Maintenance System (TMS) for tracking dependencies and I-support retractions.
pub mod tms;
/// Working memory arena and element management.
pub mod wm;
/// Formal verification bridge proving hardware equivalence between AADL/AGREE contracts and the Rust implementation.
pub mod aadl_bridge;
/// Parser for soar scripts
pub mod soar_parser;

/// Alias `preference` as `decision` for module path compatibility.
pub use preference as decision;
pub use agent::{Agent, SoarAgent};