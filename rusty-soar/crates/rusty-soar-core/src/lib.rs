//! RustySoar Core Engine
#![no_std]
#![deny(missing_docs)]

extern crate alloc;

/// Agent implementation coordinating the Soar decision cycle and memory modules.
pub mod agent;
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
/// Global symbol interning table and identifier system.
pub mod symbol;
/// Working memory arena and element management.
pub mod wm;

pub use agent::{Agent, Phase};