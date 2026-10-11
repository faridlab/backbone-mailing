// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod trace_routes;
pub use trace_routes::{public_composer, public_trace_routes};
