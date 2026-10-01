# RustySoar

A `#![no_std]`, memory-safe Rust implementation of the Soar cognitive architecture for safety-critical autonomous systems.

## Status
Under active development at ASSIST Lab. The deterministic `no_std` core now covers active instantiation identity, TMS-backed support tracking, preference filtering, selected-operator WMEs, relational/disjunctive/path condition subsets, variable-bound WME RHS actions, and basic `halt`, `interrupt`, `write`, and constant arithmetic functions. Full upstream Soar compatibility and Kani coverage remain in progress.