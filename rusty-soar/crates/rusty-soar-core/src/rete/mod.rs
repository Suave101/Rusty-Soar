//! Rete pattern matching network implementation.

/// Single-element constant tests and Alpha Memories.
pub mod alpha;
/// Token chains, Join Nodes, and Beta Memories.
pub mod beta;
/// Core execution engine and WME propagation routines.
pub mod engine;
/// Terminal production nodes and rule instantiations.
pub mod production;

pub use alpha::{AlphaMemory, AlphaNode, ConstantTest};
pub use beta::{BetaMemory, JoinNode, Token, TokenKey};
pub use engine::ReteNetwork;
pub use production::{Instantiation, ProductionId, ProductionNode};