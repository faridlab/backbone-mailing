//! The TraceClickPort — mailing's seam to the short-link module's tracker
//! surface (hand-written; user-owned; see `metaphor.codegen.yaml`).
//!
//! The click route family `/r/:code/m/:trace` is MAILING-owned (the recorded
//! design call), but the `:code` half resolves in the short-link module
//! (link trackers + their click rows live there). This module takes NO code
//! dependency on that module — campaign/utm masters and trackers are reached
//! through one-way logical uuid refs — so the delegation crosses as a PORT:
//! this module owns the trait, the host service registers ONE
//! implementation that composes the short-link module's existing public
//! seams (`find_tracker_by_code` for attribution, `resolve_redirect` for
//! resolution + click mint). Engagement's grain is unchanged — no release
//! on that side is required or wanted.
//!
//! Deny-by-default (the PhoneBookPort shape): until a host registers an
//! implementation via `MailingModule::set_trace_click_port(...)`, every call
//! fails with [`TraceClickError::NotComposed`] — a missing composition is a
//! loud refusal, never a silent "no code" that would read as an unknown
//! link.
//!
//! Contract, in order:
//!
//! 1. [`TraceClickPort::attribution`] is READ-ONLY — it answers the code's
//!    campaign attribution and never mints anything. The open pixel and the
//!    unsubscribe legs use it for the (code, trace) consistency check and
//!    must not record a click.
//! 2. [`TraceClickPort::resolve_click`] is the click leg: it refuses
//!    unknown codes and attribution mismatches BEFORE any click is minted,
//!    then resolves the target and mints the click at the short-link
//!    module's own (link, ip, day) grain. The redirect target it returns is
//!    the already-attributed URL (utm-injected by the owning module).

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

/// Why a code-side call failed.
#[derive(Debug, thiserror::Error)]
pub enum TraceClickError {
    /// No [`TraceClickPort`] has been composed for this module — the
    /// deny-by-default refusal. Installing one via
    /// `MailingModule::set_trace_click_port` is the only cure.
    #[error("trace click port not composed: {detail}")]
    NotComposed { detail: String },
    /// The short code does not resolve to a live tracker.
    #[error("unknown short code")]
    UnknownCode,
    /// The code's campaign attribution does not match the trace's — the
    /// (code, trace) pair is inconsistent and the whole pair is refused.
    #[error("code attribution does not match the trace's campaign")]
    AttributionMismatch,
    /// The short-link backend failed (storage trouble, corrupt stored
    /// target). Not distinguishable from a miss on the public surface.
    #[error("click backend: {0}")]
    Backend(String),
}

/// The code-side attribution a consistency check compares against the
/// trace's denormalized `campaign_id`. A tracker may legitimately carry no
/// campaign (`campaign_id: None`) — two unattributed halves are consistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeAttribution {
    pub campaign_id: Option<Uuid>,
}

/// The resolved redirect target of the click leg (the short-link module's
/// utm-injected absolute URL) plus the code's campaign attribution (echoed
/// for the caller's records; the mismatch check already happened inside the
/// port before any mint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClick {
    pub url: String,
    pub campaign_id: Option<Uuid>,
}

/// The short-link seam. Two methods by design: a read-only attribution
/// probe (pixel + unsubscribe legs must not mint clicks) and the
/// consistency-checked resolve + mint (click leg).
#[async_trait]
pub trait TraceClickPort: Send + Sync {
    /// The code's campaign attribution, read-only. Err
    /// ([`TraceClickError::UnknownCode`]) when the code resolves to no live
    /// tracker; never any side effect.
    async fn attribution(&self, code: &str) -> Result<CodeAttribution, TraceClickError>;

    /// Resolve the code's target AND mint its click — but only after the
    /// consistency check: when the code's campaign attribution differs from
    /// `trace_campaign_id` the call refuses
    /// [`TraceClickError::AttributionMismatch`] WITHOUT minting anything.
    /// `ip` / `country_code` are the visitor context the click grain uses
    /// (first `X-Forwarded-For` hop / geo header, when the host provides
    /// them).
    async fn resolve_click(
        &self,
        code: &str,
        trace_campaign_id: Option<Uuid>,
        ip: Option<&str>,
        country_code: Option<&str>,
    ) -> Result<ResolvedClick, TraceClickError>;
}

/// The deny-by-default implementation: the slot's initial tenant. Every
/// call refuses with [`TraceClickError::NotComposed`] — a missing
/// composition is a loud failure, never an "unknown code" that would read
/// as a miss.
pub struct RefusingTraceClick;

#[async_trait]
impl TraceClickPort for RefusingTraceClick {
    async fn attribution(&self, code: &str) -> Result<CodeAttribution, TraceClickError> {
        Err(TraceClickError::NotComposed {
            detail: format!(
                "no TraceClickPort is installed; refusing attribution lookup for code \
                 {code} — compose one via MailingModule::set_trace_click_port"
            ),
        })
    }

    async fn resolve_click(
        &self,
        code: &str,
        _trace_campaign_id: Option<Uuid>,
        _ip: Option<&str>,
        _country_code: Option<&str>,
    ) -> Result<ResolvedClick, TraceClickError> {
        Err(TraceClickError::NotComposed {
            detail: format!(
                "no TraceClickPort is installed; refusing click resolution for code \
                 {code} — compose one via MailingModule::set_trace_click_port"
            ),
        })
    }
}

/// The test double: answers every code with the canned attribution and
/// target, no database. Exercises the consistent pair, the mismatch refusal
/// (`campaign_id` set to something the trace does not carry), and the
/// unknown-code refusal (`unknown: true`).
pub struct CannedTraceClick {
    pub campaign_id: Option<Uuid>,
    pub url: String,
    /// Answer [`TraceClickError::UnknownCode`] instead of resolving.
    pub unknown: bool,
}

#[async_trait]
impl TraceClickPort for CannedTraceClick {
    async fn attribution(&self, code: &str) -> Result<CodeAttribution, TraceClickError> {
        if self.unknown {
            return Err(TraceClickError::UnknownCode);
        }
        let _ = code;
        Ok(CodeAttribution { campaign_id: self.campaign_id })
    }

    async fn resolve_click(
        &self,
        code: &str,
        trace_campaign_id: Option<Uuid>,
        _ip: Option<&str>,
        _country_code: Option<&str>,
    ) -> Result<ResolvedClick, TraceClickError> {
        if self.unknown {
            return Err(TraceClickError::UnknownCode);
        }
        if self.campaign_id != trace_campaign_id {
            let _ = code;
            return Err(TraceClickError::AttributionMismatch);
        }
        Ok(ResolvedClick { url: self.url.clone(), campaign_id: self.campaign_id })
    }
}

/// A shared, swappable click-port slot — how a host registers its
/// composition without the generated module builder needing a new field.
/// Services are built over the slot (defaulting to
/// [`RefusingTraceClick`]); `MailingModule::set_trace_click_port` installs
/// the host's implementation and every service sees it on the NEXT call.
/// Reads are cheap (an `RwLock` read); installs happen once at boot.
#[derive(Clone)]
pub struct TraceClickSlot {
    inner: Arc<std::sync::RwLock<Arc<dyn TraceClickPort>>>,
}

impl TraceClickSlot {
    /// Install (replace) the active port.
    pub fn install(&self, port: Arc<dyn TraceClickPort>) {
        // A poisoned lock still holds the old value — recovering it beats
        // panicking every future call over one panicked writer.
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = port;
    }

    /// The currently installed port.
    pub fn current(&self) -> Arc<dyn TraceClickPort> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Default for TraceClickSlot {
    fn default() -> Self {
        Self { inner: Arc::new(std::sync::RwLock::new(Arc::new(RefusingTraceClick))) }
    }
}

#[async_trait]
impl TraceClickPort for TraceClickSlot {
    async fn attribution(&self, code: &str) -> Result<CodeAttribution, TraceClickError> {
        self.current().attribution(code).await
    }

    async fn resolve_click(
        &self,
        code: &str,
        trace_campaign_id: Option<Uuid>,
        ip: Option<&str>,
        country_code: Option<&str>,
    ) -> Result<ResolvedClick, TraceClickError> {
        self.current()
            .resolve_click(code, trace_campaign_id, ip, country_code)
            .await
    }
}
