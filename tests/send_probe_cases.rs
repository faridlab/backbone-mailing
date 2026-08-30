//! The send-engine probe binary: the two-workers correctness probe and the
//! mass-mode volume probe (fresh-DB, disposable scratch per test — see
//! `behavior/common/mod.rs`). The behavioral case files in `tests/*.rs` are
//! separate integration binaries; this one exists so the probes run as their
//! own suite, separately from the per-verb cases.

mod behavior;
