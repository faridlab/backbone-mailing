//! `TraceRouteService` — the orchestration behind the public
//! `/r/:code/m/:trace` trace-route family (hand-written, user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! Three legs, one capability: the (code, trace) pair in the path IS the
//! authorization — no session, no grant chain. Every leg starts with the
//! same two steps:
//!
//! 1. **Trace probe** — the trace must exist and be live (soft-delete
//!    filtered). Unknown or deleted: [`TraceRouteError::UnknownTrace`].
//! 2. **(code, trace) consistency** — the code's campaign attribution must
//!    equal the trace's denormalized `campaign_id` (two unattributed halves
//!    are consistent). Mismatch or unknown code:
//!    [`TraceRouteError::Inconsistent`]. The refusal NEVER says which half
//!    was wrong — the HTTP layer maps it onto the same body as a miss, so
//!    unauthenticated enumeration cannot tell a wrong code from a wrong
//!    trace.
//!
//! Then per leg:
//!
//! - **Click** (`GET /r/:code/m/:trace`) — delegation first, stamps second:
//!    the port refuses (unknown / mismatch / not composed) BEFORE any write,
//!    then resolves the target and mints the click at the short-link grain;
//!    only then does this module stamp its own trace with the declared edge
//!    pair `set_opened` + `set_clicked` — always together, through the
//!    state helpers, never raw writes. `Skipped` on replay is the correct
//!    outcome (the rank guard never downgrades); `open_datetime` keeps its
//!    first stamp while `links_click_datetime` carries the LAST click.
//! - **Open pixel** (`GET …/pixel.gif`) — `set_opened` only, via
//!    [`TraceClickPort::attribution`] (read-only — a pixel must never mint a
//!    short-link click).
//! - **Unsubscribe** (`POST …/unsubscribe`) — mailing's own opt-out (the
//!    per-audience flip), NOT the RFC 8058 digest form: the trace's mailing
//!    domain is parsed with the shared refuse-loudly parser and the
//!    recipient is opted out of EVERY audience the mailing targeted, each
//!    through `SubscriptionWriteService::unsubscribe_by_email` (idempotent —
//!    the FIRST opt-out moment is the durable fact; a replay reports the
//!    standing row without restamping).
//!
//! Bare-URL clicks are invisible by design (the ported MSM-B-9 ruling):
//! only links the send rewrites into the `/r/:code/m/:trace` shape are
//! tracked; a plain URL in a body records nothing and that is documented
//! behavior, not a defect.

use uuid::Uuid;

use crate::application::service::mailing_write_service::parse_domain;
use crate::application::service::subscription_write_service::SubscriptionWriteService;
use crate::application::service::trace_click_ports::{
    CodeAttribution, TraceClickError, TraceClickPort, TraceClickSlot,
};
use crate::application::service::trace_write_service::{TraceWriteError, TraceWriteService};
use crate::infrastructure::persistence::mailing_send_repository::MailingSendRepository;
use crate::infrastructure::persistence::trace_repository::TraceRepository;

/// Typed error surface (house style). `UnknownTrace` and `Inconsistent` are
/// the pair the public layer folds into one indistinguishable refusal;
/// `NotComposed` / `Backend` / `Invalid` are internal-class — also refused
/// publicly, but traced loudly so a composition or data bug cannot hide
/// behind the uniform 404.
#[derive(Debug, thiserror::Error)]
pub enum TraceRouteError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    /// No live trace for this id.
    #[error("unknown trace")]
    UnknownTrace,
    /// The (code, trace) pair failed the consistency check — wrong code for
    /// this trace, unknown code, or an unreadable code backend. Deliberately
    /// carries no detail about WHICH half failed.
    #[error("inconsistent (code, trace) pair")]
    Inconsistent,
    /// No TraceClickPort composed — fail closed.
    #[error("trace click port not composed")]
    NotComposed(String),
    /// Storage/backend trouble on either side.
    #[error("backend: {0}")]
    Backend(String),
    /// Stored data failed a parse that should be impossible (a mailing
    /// domain is validated at write time).
    #[error("invalid stored data: {0}")]
    Invalid(String),
}

fn click_err(e: TraceClickError) -> TraceRouteError {
    match e {
        TraceClickError::UnknownCode => TraceRouteError::Inconsistent,
        TraceClickError::AttributionMismatch => TraceRouteError::Inconsistent,
        TraceClickError::NotComposed { detail } => TraceRouteError::NotComposed(detail),
        TraceClickError::Backend(detail) => TraceRouteError::Backend(detail),
    }
}

/// One audience's opt-out outcome for the unsubscribe leg.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudienceOptOut {
    pub audience_id: Uuid,
    /// True when THIS call performed the flip; false when the row was
    /// already opted out (the idempotent replay) or the recipient is not a
    /// member of the audience (nothing to flip).
    pub changed: bool,
    /// True when the standing row is opted out after this call.
    pub opted_out: bool,
}

/// The trace-route orchestration. Stateless over a pool; the click seam is
/// the swappable slot (deny-by-default until the host composes it).
pub struct TraceRouteService {
    pool: sqlx::PgPool,
    clicks: TraceClickSlot,
    trace_write: TraceWriteService,
    subscription_write: SubscriptionWriteService,
}

impl TraceRouteService {
    pub fn new(pool: sqlx::PgPool, clicks: TraceClickSlot) -> Self {
        Self {
            trace_write: TraceWriteService::new(pool.clone()),
            subscription_write: SubscriptionWriteService::new(pool.clone()),
            pool,
            clicks,
        }
    }

    /// The click leg. Returns the redirect target (the port's utm-injected
    /// absolute URL); the caller answers 301.
    pub async fn record_click(
        &self,
        code: &str,
        trace_id: Uuid,
        ip: Option<&str>,
        country_code: Option<&str>,
    ) -> Result<String, TraceRouteError> {
        let row = self.route_trace(trace_id).await?;
        // Delegation first: the port refuses unknown/mismatched codes with
        // NO mint and NO trace stamp; only a consistent pair proceeds.
        let resolved = self
            .clicks
            .resolve_click(code, row.campaign_id, ip, country_code)
            .await
            .map_err(click_err)?;
        // The declared edge pair, through the state helpers (never raw
        // writes). Skipped-on-replay is correct: the rank guard keeps a
        // replied trace replied, and open_datetime keeps its first stamp.
        self.trace_write.set_opened(trace_id).await?;
        self.trace_write.set_clicked(trace_id).await?;
        Ok(resolved.url)
    }

    /// The open-pixel leg: the attribution probe is read-only, the stamp is
    /// `set_opened` alone (a pixel never mints a short-link click and never
    /// stamps a click datetime).
    pub async fn record_open(&self, code: &str, trace_id: Uuid) -> Result<(), TraceRouteError> {
        let row = self.route_trace(trace_id).await?;
        self.check_consistency(code, row.campaign_id).await?;
        self.trace_write.set_opened(trace_id).await?;
        Ok(())
    }

    /// The unsubscribe leg: opt the trace's recipient out of every audience
    /// the trace's mailing targeted. Idempotent per audience — the first
    /// opt-out moment is the durable fact.
    pub async fn unsubscribe(
        &self,
        code: &str,
        trace_id: Uuid,
        reason_id: Option<Uuid>,
    ) -> Result<Vec<AudienceOptOut>, TraceRouteError> {
        let row = self.route_trace(trace_id).await?;
        self.check_consistency(code, row.campaign_id).await?;

        let mut tx = self.pool.begin().await?;
        let domain_json = MailingSendRepository::find_live_mailing_domain(&mut tx, row.mailing_id)
            .await?
            .ok_or_else(|| TraceRouteError::Invalid(format!(
                "trace {} points at mailing {} which no longer resolves a live row",
                row.id, row.mailing_id
            )))?;
        tx.commit().await?;

        let audiences = audience_ids_of(&domain_json)?;
        let mut out = Vec::with_capacity(audiences.len());
        for audience_id in audiences {
            // NotFound means this recipient is not a member of that audience
            // (or is a party-model recipient with no contact row): nothing
            // to flip — reported truthfully, never a silent success.
            let (changed, opted_out) = match self
                .subscription_write
                .unsubscribe_by_email(&row.recipient_email, audience_id, reason_id)
                .await
            {
                Ok((row, changed)) => (changed, row.opt_out),
                Err(
                    crate::application::service::subscription_write_service::SubscriptionWriteError::NotFound(_),
                ) => (false, false),
                Err(e) => return Err(e.into()),
            };
            out.push(AudienceOptOut { audience_id, changed, opted_out });
        }
        Ok(out)
    }

    // ── internals ────────────────────────────────────────────────────────────

    async fn route_trace(
        &self,
        trace_id: Uuid,
    ) -> Result<crate::infrastructure::persistence::trace_repository::RouteTraceRow, TraceRouteError> {
        let mut tx = self.pool.begin().await?;
        let row = TraceRepository::find_route_trace(&mut tx, trace_id).await?;
        tx.commit().await?;
        row.ok_or(TraceRouteError::UnknownTrace)
    }

    /// The (code, trace) consistency check, read-only: the code's campaign
    /// attribution must equal the trace's (None == None is consistent — two
    /// unattributed halves belong together).
    async fn check_consistency(
        &self,
        code: &str,
        trace_campaign_id: Option<Uuid>,
    ) -> Result<CodeAttribution, TraceRouteError> {
        let attribution = self.clicks.attribution(code).await.map_err(click_err)?;
        if attribution.campaign_id != trace_campaign_id {
            return Err(TraceRouteError::Inconsistent);
        }
        Ok(attribution)
    }
}

impl From<crate::application::service::subscription_write_service::SubscriptionWriteError>
    for TraceRouteError
{
    fn from(
        e: crate::application::service::subscription_write_service::SubscriptionWriteError,
    ) -> Self {
        use crate::application::service::subscription_write_service::SubscriptionWriteError as E;
        match e {
            E::Db(e) => TraceRouteError::Db(e),
            E::NotFound(d) => TraceRouteError::Backend(format!("subscription: {d}")),
            E::Conflict(d) => TraceRouteError::Backend(format!("subscription: {d}")),
            E::Invalid(d) => TraceRouteError::Invalid(format!("subscription: {d}")),
        }
    }
}

impl From<TraceWriteError> for TraceRouteError {
    fn from(e: TraceWriteError) -> Self {
        match e {
            TraceWriteError::Db(e) => TraceRouteError::Db(e),
            TraceWriteError::Invalid(d) => TraceRouteError::Invalid(format!("trace verb: {d}")),
        }
    }
}

/// The audience ids a mailing's domain targets: every `mailing_audience_id`
/// term with a positive operator (= / in). Negative operators (≠ / not in)
/// are exclusions, not targets — a recipient opted out of an excluded
/// audience was never reached THROUGH it. A domain with no audience terms
/// targets an unbucketed population: there is no per-audience flip to
/// perform, and the caller reports the empty set truthfully.
fn audience_ids_of(domain: &serde_json::Value) -> Result<Vec<Uuid>, TraceRouteError> {
    use crate::infrastructure::persistence::mailing_send_repository::{
        DomainField, DomainOp,
    };

    let compiled = parse_domain(domain)
        .map_err(|e| TraceRouteError::Invalid(format!("stored mailing domain: {e}")))?;
    let mut ids = Vec::new();
    for term in &compiled.terms {
        if term.field == DomainField::MailingAudienceId
            && matches!(term.op, DomainOp::Eq | DomainOp::In)
        {
            for raw in &term.values {
                let id = Uuid::parse_str(raw).map_err(|_| {
                    TraceRouteError::Invalid(format!(
                        "stored mailing domain carries a non-uuid audience value: {raw}"
                    ))
                })?;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    // HTTP-level coverage (click round-trip, pixel, unsubscribe
    // idempotency, consistency refusal, throttle) lives in
    // tests/trace_route_cases.rs — fresh-DB, disposable scratch per test.
}
