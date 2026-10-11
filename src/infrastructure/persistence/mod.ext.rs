// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod mailing_send_repository;
pub mod subscription_repository;
pub mod trace_repository;
pub use mailing_send_repository::{
    ClaimedMailing, CompiledDomain, DomainField, DomainOp, DomainTerm, MailingSendRepository,
    ResolvedRecipient,
};
pub use subscription_repository::{AudienceRecipient, SubscriptionRepository, SubscriptionRow};
pub use trace_repository::{MailingTraceCounts, TraceRepository, TraceRow};
