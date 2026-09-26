#![no_std]
#![deny(missing_docs)]
#![deny(unsafe_code)]

//! # RustySoar Core
//! A `#![no_std]`, zero-GC, certifiable Rust implementation of the Soar cognitive engine.

extern crate alloc;

/// Top-level Soar agent orchestration and decision cycle driver.
pub mod agent;
/// Decision cycle and operator preference resolution.
pub mod decision;
/// Rete pattern matching execution network.
pub mod rete;
/// Symbol interning and O(1) identifier mapping.
pub mod symbol;
/// Working Memory Elements (WMEs) and generational arena allocation.
pub mod wm;

pub use agent::{Agent, Phase};
pub use symbol::SymbolId;
pub use wm::{Wme, WmeArena, WmeKey};