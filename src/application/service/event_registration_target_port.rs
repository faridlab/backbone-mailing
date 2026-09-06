//! The EventRegistrationTargetResolver — the mass_mailing_event bridge
//! target's typed resolver port (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The `event_registration` target model mails an event's registrants: the
//! audience lives in the events module's registration read model (attendee
//! rows carrying an email column), and this module takes NO code
//! dependency on the events module — the read crosses as a DECLARED seam,
//! exactly like the crm/sale bridge targets:
//!
//! - this module owns the trait and its typed error;
//! - the host service composes ONE implementation that resolves
//!   registrations through the events module's read surface, applying the
//!   events module's own declared mail-eligibility law
//!   (`state IN ('open','done') AND active`, plus not soft-deleted) and
//!   mapping the SAME whitelisted typed domain DSL every target sees onto
//!   the registration columns;
//! - the seam stays one-way and read-only: the port answers recipients,
//!   it never writes, never seats, and never widens into a generic events
//!   query surface.
//!
//! Fail-closed posture (two layers, never a silent zero):
//!
//! 1. The REGISTRY layer is the operational default: a mailing targeting
//!    `event_registration` that reaches the send walk with no composed
//!    resolver PARKS loudly (state stays retryable,
//!    `metadata.send_error` names the missing seam) — the same
//!    never-silent contract the crm/sale targets run under. This is
//!    deliberate: a permanently-refusing entry seeded into the resolver
//!    registry would fail the whole sweep (`Conflict`) and starve
//!    UNRELATED mailings of their send turn, while the park isolates the
//!    misconfigured mailing and keeps the queue draining.
//! 2. [`RefusingEventRegistrationTarget`] is the deny-by-default
//!    implementation for DIRECT port consumers (the
//!    SaleInvoicedAmountPort shape): every call refuses with the typed
//!    [`EventRegistrationTargetError::NotComposed`] naming the install
//!    verb.
//!
//! The *_sms twin rides the SAME target on the existing sms channel (the
//! MVX-2 collapse: no twin enum, no twin module); the sms send walk itself
//! stays uncomposed at this seam, so nothing here promises delivery. No
//! track-shaped target exists — the track twins are deferred by owner
//! ruling until a tracks substrate is ratified in the events module.

use std::sync::Arc;

use async_trait::async_trait;

use crate::infrastructure::persistence::mailing_send_repository::{
    CompiledDomain, ResolvedRecipient,
};

/// Why an event-registration target resolution failed.
#[derive(Debug, thiserror::Error)]
pub enum EventRegistrationTargetError {
    /// No [`EventRegistrationTargetResolver`] has been composed — the
    /// deny-by-default refusal. Installing one via
    /// `MailingWriteService::with_event_registration_resolver` (or the
    /// generic `with_target_resolver`) is the only cure.
    #[error("event_registration target port not composed: {detail}")]
    NotComposed { detail: String },
    /// The events backend failed. Deliberately opaque — events-side detail
    /// never leaks through the seam.
    #[error("events backend: {0}")]
    Backend(String),
}

/// The events-registrations bridge target's resolver port: ONE method by
/// design — resolve the SAME whitelisted typed domain DSL every target
/// sees into email recipients. Nothing else about the events module
/// crosses.
///
/// The implementation decides which whitelisted fields carry an honest
/// column on the registration row (email, attendee name, company name do;
/// person-name and country fields do not) and must REFUSE — not silently
/// drop — a domain term it cannot bind.
#[async_trait]
pub trait EventRegistrationTargetResolver: Send + Sync {
    /// Resolve the compiled domain into registrant recipients. Read-only:
    /// the implementation must never write, seat, or mint anything on the
    /// events side.
    async fn resolve(
        &self,
        domain: &CompiledDomain,
    ) -> Result<Vec<ResolvedRecipient>, EventRegistrationTargetError>;
}

/// The deny-by-default implementation for direct port consumers: every
/// call refuses with the typed
/// [`EventRegistrationTargetError::NotComposed`]. The registry layer's
/// park (see the module doc) remains the operational fail-closed default
/// for the send walk.
pub struct RefusingEventRegistrationTarget;

#[async_trait]
impl EventRegistrationTargetResolver for RefusingEventRegistrationTarget {
    async fn resolve(
        &self,
        _domain: &CompiledDomain,
    ) -> Result<Vec<ResolvedRecipient>, EventRegistrationTargetError> {
        Err(EventRegistrationTargetError::NotComposed {
            detail: "no EventRegistrationTargetResolver is installed; refusing to resolve an \
                     event-registration audience — compose one via \
                     MailingWriteService::with_event_registration_resolver"
                .into(),
        })
    }
}

/// The typed port's adapter into the per-target resolver registry: wraps
/// an [`EventRegistrationTargetResolver`] as the generic
/// `TargetRecipientResolver` keyed `event_registration` (the
/// `PartyResolverAdapter` shape). The typed error's `Display` crosses as
/// the refusal string — the send walk parks on it loudly.
pub struct EventRegistrationTargetAdapter {
    inner: Arc<dyn EventRegistrationTargetResolver>,
}

impl EventRegistrationTargetAdapter {
    pub fn new(inner: Arc<dyn EventRegistrationTargetResolver>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl crate::application::service::mailing_write_service::TargetRecipientResolver
    for EventRegistrationTargetAdapter
{
    fn target_model(&self) -> &'static str {
        "event_registration"
    }

    async fn resolve(
        &self,
        domain: &CompiledDomain,
    ) -> Result<Vec<ResolvedRecipient>, String> {
        self.inner
            .resolve(domain)
            .await
            .map_err(|e| e.to_string())
    }
}
