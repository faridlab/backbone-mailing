//! `MailingWriteService` — the mailing lifecycle + the mass-send engine
//! (hand-authored, user-owned; see `metaphor.codegen.yaml`).
//!
//! House rule honored throughout: services orchestrate, repositories hold
//! SQL — no raw sqlx outside `src/infrastructure/persistence/`.
//!
//! The engine mirrors the declared sweep order (schema/hooks/index.hook.yaml,
//! job `mailing-send-queue`, commit policy `commit_per_batch`):
//!
//! 1. claim due mailings `FOR UPDATE SKIP LOCKED` through the pickup verb —
//!    the state-guarded flip rides the claim transaction; a domain that
//!    fails to re-parse PARKS inside the same transaction (refuse loudly:
//!    typed error, zero sends, `metadata.send_error` carries the reason);
//! 2. resolve recipients through the typed domain parser against the target
//!    resolver (contacts via parameterized SQL; party via the host-composed
//!    resolver port — a party mailing without a resolver parks loudly);
//! 3. suppression: exclusion-list blacklist, cross-list opt-out (OPT-OUT-WINS,
//!    the recorded deviation from Odoo's opt-in-wins), seen-list, and
//!    duplicate-email collapses — each visibly pre-cancelled with the
//!    matching failure_type + a `RecipientSuppressedAtSend` event, never a
//!    silent drop;
//! 4. A/B fragment membership via the PERSISTED sampling seed:
//!    HMAC-SHA256(seed, recipient_email), first 8 bytes big-endian mod 100
//!    < ab_testing_pc — stable forever, never re-randomized;
//! 5. mint outgoing traces (the `(mailing, recipient)` partial unique is the
//!    mint fence — a fence hit is logged and skipped, never converged) and
//!    enqueue per-recipient mail rows through backbone-mail's PUBLIC
//!    services (`message_post` with empty recipients + `enqueue`), then
//!    attach the mail row id onto the trace in a `commit_per_batch`
//!    transaction shape;
//! 6. complete the mailing (sent_date + kpi_mail_required on first send) or
//!    leave it `sending` when the pass budget was exhausted (the next sweep
//!    resumes through the seen-list skip);
//! 7. reconcile settled SMTP verdicts onto outgoing traces (set_sent /
//!    set_failed — one failure never fails the batch);
//! 8. auto-blacklist sweep on the DATABASE clock;
//! 9. A/B winner promotion for tests past promote_at with a done sibling.
//!
//! The orphan-repair arm (outgoing traces whose enqueue never completed)
//! runs at the top of every drive, healing crash-between-mint-and-attach.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::infrastructure::persistence::mailing_send_repository::{
    ClaimedMailing, CompiledDomain, DomainField, DomainOp, DomainTerm,
    MailingSendRepository, ResolvedRecipient,
};
use crate::infrastructure::persistence::trace_repository::TraceRepository;

use backbone_mail::application::service::mail_queue_write_service::{
    MailQueueWriteService, MailQueueError,
};
use backbone_mail::application::service::message_write_service::{
    MessagePostCommand, MessageWriteService,
};

/// Typed refusal for a domain that will not parse. NEVER a silent
/// zero-recipient sweep: create/update reject 422 before anything is
/// stored; the send sweep parks the mailing and records the reason.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DomainInvalid {
    #[error("domain must be a JSON array of terms")]
    NotAnArray,
    #[error("term {index}: expected [field, op, value] or {{field, op, value}}")]
    MalformedTerm { index: usize },
    #[error("term {index}: field must be a string")]
    FieldNotString { index: usize },
    #[error("field '{0}' is not in the domain whitelist (email, name, first_name, last_name, company_name, country_code, mailing_audience_id)")]
    UnknownField(String),
    #[error("term {index}: op must be one of =, !=, in, not in, like, not like")]
    UnknownOp { index: usize },
    #[error("term {index}: 'in'/'not in' need an array of strings")]
    ValueNotArray { index: usize },
    #[error("term {index}: '='/'!='/'like'/'not like' need exactly one value")]
    ValueNotScalar { index: usize },
    #[error("term {index}: audience values must be uuids ({raw})")]
    BadAudienceUuid { index: usize, raw: String },
}

/// The service's typed error surface (code + HTTP status, house style).
#[derive(Debug, thiserror::Error)]
pub enum MailingWriteError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("domain invalid: {0}")]
    Domain(#[from] DomainInvalid),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("mail seam: {0}")]
    MailSeam(String),
}

impl MailingWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "mailing_db_error",
            Self::Domain(_) => "domain_invalid",
            Self::NotFound(_) => "not_found",
            Self::Invalid(_) => "invalid_input",
            Self::Conflict(_) => "state_conflict",
            Self::MailSeam(_) => "mail_seam_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::Db(_) => 500,
            Self::Domain(_) => 422,
            Self::NotFound(_) => 404,
            Self::Invalid(_) => 422,
            Self::Conflict(_) => 409,
            Self::MailSeam(_) => 502,
        }
    }
}

impl From<MailQueueError> for MailingWriteError {
    fn from(e: MailQueueError) -> Self {
        Self::MailSeam(e.to_string())
    }
}

// ── configuration (constructor-overridable; defaults mirror config/application.yml) ──

/// The send engine's shape knobs.
#[derive(Debug, Clone)]
pub struct MailingSendConfig {
    /// Mailings claimed per sweep (the claim batch).
    pub claim_batch: i64,
    /// NEW recipients driven per mailing per sweep — the pass budget. When
    /// the audience exceeds it the mailing stays `sending` and the next
    /// sweep resumes through the seen-list skip.
    pub recipients_per_pass: i64,
    /// Recipients minted + enqueued per commit — the `commit_per_batch`
    /// grain (bounds the replay window).
    pub mail_enqueue_batch: usize,
    /// Pause between enqueue batches — the provider-friendly pacing floor.
    pub inter_batch_delay_ms: u64,
    /// Optional hard enqueue rate ceiling (provider contract). When set,
    /// enqueues are spaced at >= 1000/eps milliseconds.
    pub max_enqueues_per_second: Option<u64>,
    /// Resolver safety ceiling. A domain resolving to MORE than this fails
    /// loudly (`Conflict`) — never a silent truncation.
    pub resolver_hard_cap: i64,
}

impl Default for MailingSendConfig {
    fn default() -> Self {
        Self {
            claim_batch: 16,
            recipients_per_pass: 500,
            mail_enqueue_batch: 100,
            inter_batch_delay_ms: 0,
            max_enqueues_per_second: None,
            resolver_hard_cap: 100_000,
        }
    }
}

/// The auto-blacklist thresholds (config keys, per the declared rule).
#[derive(Debug, Clone)]
pub struct AutoBlacklistConfig {
    pub max_bounces: i64,
    pub window_weeks: i32,
    pub spread_days: i32,
}

impl Default for AutoBlacklistConfig {
    fn default() -> Self {
        Self {
            max_bounces: 5,
            window_weeks: 13,
            spread_days: 7,
        }
    }
}

// ── ports (host-composed seams) ─────────────────────────────────────────────

/// Resolves a compiled domain into party recipients — the host's composition
/// seam for `target_model = 'party'` (mailing never depends on party
/// directly; one-way, like every cross-schema seam here).
#[async_trait::async_trait]
pub trait PartyRecipientResolver: Send + Sync {
    async fn resolve(&self, domain: &CompiledDomain) -> Result<Vec<ResolvedRecipient>, String>;
}

/// Outbound mailing events. The default sink is a no-op with tracing — the
/// shipped graph carries no outbox dependency; a host composes an
/// outbox-backed sink when it wants the events staged.
pub trait MailingEventSink: Send + Sync {
    fn mailing_launched(&self, mailing_id: Uuid, campaign_id: Option<Uuid>, schedule_type: &str);
    fn recipient_suppressed(&self, mailing_id: Uuid, trace_id: Uuid, email: &str, cause: &str);
    fn auto_blacklist_added(&self, email: &str);
    fn ab_test_winner_promoted(&self, ab_test_id: Uuid, winner_mailing_id: Uuid);
}

/// The default sink: tracing only.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingEventSink;

impl MailingEventSink for TracingEventSink {
    fn mailing_launched(&self, mailing_id: Uuid, campaign_id: Option<Uuid>, schedule_type: &str) {
        tracing::info!(
            mailing_id = %mailing_id,
            campaign_id = ?campaign_id,
            schedule_type,
            "MailingQueuedForSend"
        );
    }
    fn recipient_suppressed(&self, mailing_id: Uuid, trace_id: Uuid, email: &str, cause: &str) {
        tracing::info!(
            mailing_id = %mailing_id,
            trace_id = %trace_id,
            recipient_email = email,
            cause,
            "RecipientSuppressedAtSend"
        );
    }
    fn auto_blacklist_added(&self, email: &str) {
        tracing::info!(email, "auto_blacklist_added");
    }
    fn ab_test_winner_promoted(&self, ab_test_id: Uuid, winner_mailing_id: Uuid) {
        tracing::info!(
            ab_test_id = %ab_test_id,
            winner_mailing_id = %winner_mailing_id,
            "ab_test_winner_promoted"
        );
    }
}

// ── commands ────────────────────────────────────────────────────────────────

/// Create/update payload for a mailing. `mailing_domain_raw` arrives in
/// either accepted shape (Odoo triples or canonical objects) and is stored
/// ONLY in canonical form, after the typed parse.
#[derive(Debug, Clone, Default)]
pub struct MailingUpsertCommand {
    pub subject: String,
    pub preview: Option<String>,
    pub body_html: String,
    pub email_from: String,
    pub reply_to: Option<String>,
    pub mailing_domain_raw: serde_json::Value,
    pub target_model: String,
    pub schedule_type: String,
    pub schedule_date: Option<chrono::DateTime<chrono::Utc>>,
    pub use_exclusion_list: bool,
    pub campaign_id: Option<Uuid>,
}

/// What one sweep did — the observable the cron probe and the volume probe
/// assert against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SweepOutcome {
    pub claimed: usize,
    pub parked: usize,
    pub completed: usize,
    pub completed_empty: usize,
    pub still_sending: usize,
    pub recipients_resolved: usize,
    pub minted: usize,
    pub repaired: usize,
    pub enqueued: usize,
    pub fence_skips: usize,
    pub suppressed_blacklist: usize,
    pub suppressed_optout: usize,
    pub suppressed_dup: usize,
    pub skipped_seen: usize,
    pub skipped_ab_nonmember: usize,
    pub reconcile_sent: usize,
    pub reconcile_failed: usize,
    pub auto_blacklisted: u64,
    pub ab_promotions: usize,
}

// ── the service ─────────────────────────────────────────────────────────────

/// The mailing lifecycle + send engine. Stateless over a pool; every public
/// verb opens its own unit of work.
pub struct MailingWriteService {
    pool: sqlx::PgPool,
    cfg: MailingSendConfig,
    blacklist_cfg: AutoBlacklistConfig,
    party_resolver: Option<Arc<dyn PartyRecipientResolver>>,
    events: Arc<dyn MailingEventSink>,
    messages: MessageWriteService,
    queue: MailQueueWriteService,
}

impl MailingWriteService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            messages: MessageWriteService::new(pool.clone()),
            queue: MailQueueWriteService::new(pool.clone()),
            pool,
            cfg: MailingSendConfig::default(),
            blacklist_cfg: AutoBlacklistConfig::default(),
            party_resolver: None,
            events: Arc::new(TracingEventSink),
        }
    }

    /// Override the engine shape (tests drive small batches + real pacing).
    pub fn with_send_config(mut self, cfg: MailingSendConfig) -> Self {
        self.cfg = cfg;
        self
    }

    /// Override the auto-blacklist thresholds.
    pub fn with_auto_blacklist_config(mut self, cfg: AutoBlacklistConfig) -> Self {
        self.blacklist_cfg = cfg;
        self
    }

    /// Compose the party resolver port (required for `target_model='party'`).
    pub fn with_party_resolver(mut self, r: Arc<dyn PartyRecipientResolver>) -> Self {
        self.party_resolver = Some(r);
        self
    }

    /// Compose the event sink (default: tracing-only).
    pub fn with_event_sink(mut self, sink: Arc<dyn MailingEventSink>) -> Self {
        self.events = sink;
        self
    }

    // ── lifecycle verbs ───────────────────────────────────────────────────────

    /// Create a draft mailing. The domain parses FIRST — a 422 refusal
    /// before anything is stored; the stored form is canonical.
    pub async fn create_mailing(&self, cmd: &MailingUpsertCommand) -> Result<Uuid, MailingWriteError> {
        if cmd.subject.trim().is_empty() {
            return Err(MailingWriteError::Invalid("subject is required".into()));
        }
        if cmd.body_html.trim().is_empty() {
            return Err(MailingWriteError::Invalid("body_html is required".into()));
        }
        if !cmd.email_from.contains('@') {
            return Err(MailingWriteError::Invalid(format!(
                "email_from must be an address: {}",
                cmd.email_from
            )));
        }
        let domain = parse_domain(&cmd.mailing_domain_raw)?;
        let target_model = match cmd.target_model.as_str() {
            "mailing_contact" | "party" => cmd.target_model.clone(),
            other => {
                return Err(MailingWriteError::Invalid(format!(
                    "target_model must be mailing_contact or party, not {other}"
                )))
            }
        };
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        MailingSendRepository::insert_mailing(
            &mut tx,
            id,
            &cmd.subject,
            cmd.preview.as_deref(),
            &cmd.body_html,
            &cmd.email_from,
            cmd.reply_to.as_deref(),
            &canonical_domain_json(&domain),
            &target_model,
            &cmd.schedule_type,
            cmd.schedule_date,
            cmd.use_exclusion_list,
            cmd.campaign_id,
            false,
            0,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Update a DRAFT mailing (guarded — anything past draft refuses 409).
    /// The domain re-parses FIRST with the same refuse-loudly posture.
    pub async fn update_mailing(
        &self,
        id: Uuid,
        cmd: &MailingUpsertCommand,
    ) -> Result<(), MailingWriteError> {
        let domain = parse_domain(&cmd.mailing_domain_raw)?;
        let mut tx = self.pool.begin().await?;
        let updated = MailingSendRepository::update_draft(
            &mut tx,
            id,
            &cmd.subject,
            cmd.preview.as_deref(),
            &cmd.body_html,
            &cmd.email_from,
            cmd.reply_to.as_deref(),
            &canonical_domain_json(&domain),
            &cmd.schedule_type,
            cmd.schedule_date,
            cmd.use_exclusion_list,
        )
        .await?;
        tx.commit().await?;
        if !updated {
            match self.live_state(id).await? {
                None => Err(MailingWriteError::NotFound(format!("mailing {id}"))),
                Some(s) => Err(MailingWriteError::Conflict(format!(
                    "mailing {id} is {s}, only draft is editable"
                ))),
            }
        } else {
            Ok(())
        }
    }

    /// The launch edge: draft → in_queue. `immediate` clears any standing
    /// schedule; `scheduled` requires the date. Stages the arming event
    /// (`MailingQueuedForSend`) — the self-arming trigger for the sweep.
    pub async fn launch(
        &self,
        id: Uuid,
        schedule_type: &str,
        schedule_date: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), MailingWriteError> {
        let (schedule_type, schedule_date) = match schedule_type {
            "immediate" => ("immediate".to_string(), None),
            "scheduled" => {
                let d = schedule_date.ok_or_else(|| {
                    MailingWriteError::Invalid("scheduled launch needs schedule_date".into())
                })?;
                ("scheduled".to_string(), Some(d))
            }
            other => {
                return Err(MailingWriteError::Invalid(format!(
                    "schedule_type must be immediate or scheduled, not {other}"
                )))
            }
        };
        let mut tx = self.pool.begin().await?;
        let queued = MailingSendRepository::queue_mailing(
            &mut tx,
            id,
            &schedule_type,
            schedule_date,
        )
        .await?;
        tx.commit().await?;
        if !queued {
            return Err(match self.live_state(id).await? {
                None => MailingWriteError::NotFound(format!("mailing {id}")),
                Some(s) => MailingWriteError::Conflict(format!(
                    "mailing {id} is {s}, only draft can launch"
                )),
            });
        }
        // The arming event carries the campaign attribution when present.
        let campaign_id = {
            let mut tx = self.pool.begin().await?;
            let cid = MailingSendRepository::mailing_campaign_id(&mut tx, id).await?;
            tx.commit().await?;
            cid
        };
        self.events
            .mailing_launched(id, campaign_id, &schedule_type);
        Ok(())
    }

    /// The cancel edge: [in_queue, sending, done] → draft, schedule cleared,
    /// traces KEPT (the machine has no canceled state — audit survives).
    pub async fn cancel(&self, id: Uuid) -> Result<(), MailingWriteError> {
        let mut tx = self.pool.begin().await?;
        let canceled = MailingSendRepository::cancel_mailing(&mut tx, id).await?;
        tx.commit().await?;
        if !canceled {
            return Err(match self.live_state(id).await? {
                None => MailingWriteError::NotFound(format!("mailing {id}")),
                Some(s) => MailingWriteError::Conflict(format!(
                    "mailing {id} is {s} and cannot be canceled from this state"
                )),
            });
        }
        Ok(())
    }

    /// Re-queue a PARKED mailing after its domain was fixed: clears the
    /// send_error park and moves sending → in_queue (guarded on the park
    /// marker, so a normal in-flight sending mailing is untouched).
    pub async fn requeue_after_domain_fix(&self, id: Uuid) -> Result<(), MailingWriteError> {
        let mut tx = self.pool.begin().await?;
        let unparked = MailingSendRepository::unpark_mailing(&mut tx, id).await?;
        tx.commit().await?;
        if !unparked {
            return match self.live_state(id).await? {
                None => Err(MailingWriteError::NotFound(format!("mailing {id}"))),
                Some(_) => Err(MailingWriteError::Conflict(format!(
                    "mailing {id} is not parked (no send_error to clear)"
                ))),
            };
        }
        Ok(())
    }

    /// `retry_failed`: soft-delete the error/bounce traces (opens the mint
    /// fence for a fresh live trace; the audit trail stays queryable),
    /// then re-queue done → in_queue. Zero failed traces is a typed
    /// refusal, never a silent no-op.
    pub async fn retry_failed(&self, id: Uuid) -> Result<u64, MailingWriteError> {
        let mut tx = self.pool.begin().await?;
        let failed = MailingSendRepository::count_failed_traces(&mut tx, id).await?;
        if failed == 0 {
            tx.rollback().await?;
            return Err(MailingWriteError::Invalid(format!(
                "mailing {id} has no failed (error/bounce) traces to retry"
            )));
        }
        let soft_deleted = MailingSendRepository::soft_delete_failed_traces(&mut tx, id, 1000).await?;
        let requeued = MailingSendRepository::requeue_mailing(&mut tx, id).await?;
        tx.commit().await?;
        if !requeued {
            return Err(MailingWriteError::Conflict(format!(
                "mailing {id} is not done — retry_failed only exits done"
            )));
        }
        Ok(soft_deleted)
    }

    async fn live_state(&self, id: Uuid) -> Result<Option<String>, MailingWriteError> {
        let mut tx = self.pool.begin().await?;
        let row = MailingSendRepository::find_live_state(&mut tx, id).await?;
        tx.commit().await?;
        Ok(row.map(|(_, state, _)| state))
    }

    // ── the send engine ──────────────────────────────────────────────────────

    /// One sweep — the `mailing::send_queue` handler. Every step idempotent;
    /// commit per batch. See the module docs for the step order.
    pub async fn send_queue_sweep(&self) -> Result<SweepOutcome, MailingWriteError> {
        let mut out = SweepOutcome::default();
        let mut parked_ids = HashSet::new();

        // (1) claim + (2's parse half) — parse INSIDE the claim transaction:
        // a refusal parks before any send can happen.
        let claims = {
            let mut tx = self.pool.begin().await?;
            let claimed =
                MailingSendRepository::claim_due_mailings(&mut tx, self.cfg.claim_batch).await?;
            for m in &claimed {
                match parse_domain(&m.mailing_domain) {
                    Ok(_) => {
                        MailingSendRepository::flip_sending(&mut tx, m.id).await?;
                    }
                    Err(e) => {
                        MailingSendRepository::park_mailing(&mut tx, m.id, &e.to_string()).await?;
                        parked_ids.insert(m.id);
                        out.parked += 1;
                        tracing::warn!(
                            mailing_id = %m.id,
                            error = %e,
                            "mailing parked: domain failed to parse at send time"
                        );
                    }
                }
            }
            tx.commit().await?;
            claimed
        };
        out.claimed = claims.len();

        // (2..6) drive each claimed mailing.
        for mailing in &claims {
            if parked_ids.contains(&mailing.id) {
                continue;
            }
            self.drive_mailing(mailing, &mut out).await?;
        }

        // (7) reconcile settled SMTP verdicts (global — done mailings' final
        // traces still converge here).
        {
            let mut tx = self.pool.begin().await?;
            let settled = TraceRepository::settled_mails_for_reconcile(&mut tx).await?;
            for (trace_id, state, failure_type) in settled {
                let moved = match state.as_str() {
                    "sent" => TraceRepository::set_sent(&mut tx, trace_id).await?,
                    _ => {
                        let ftype = failure_type.unwrap_or_else(|| "mail_smtp".into());
                        TraceRepository::set_failed(&mut tx, trace_id, &ftype, None).await?
                    }
                };
                if moved {
                    if state == "sent" {
                        out.reconcile_sent += 1;
                    } else {
                        out.reconcile_failed += 1;
                    }
                }
            }
            tx.commit().await?;
        }

        // (8) auto-blacklist on the DB clock.
        {
            let mut tx = self.pool.begin().await?;
            out.auto_blacklisted = MailingSendRepository::auto_blacklist_sweep(
                &mut tx,
                self.blacklist_cfg.window_weeks,
                self.blacklist_cfg.max_bounces,
                self.blacklist_cfg.spread_days,
            )
            .await?;
            tx.commit().await?;
        }

        // (9) A/B winner promotion.
        self.promote_due_ab_tests(&mut out).await?;

        Ok(out)
    }

    /// Steps (2)–(6) for one claimed mailing.
    async fn drive_mailing(
        &self,
        m: &ClaimedMailing,
        out: &mut SweepOutcome,
    ) -> Result<(), MailingWriteError> {
        // The orphan-repair arm: outgoing traces whose enqueue never
        // completed heal first (crash between mint and attach). The grace
        // interval keeps a CONCURRENT worker's in-flight mint→enqueue
        // window out of the orphan set — repairing it would enqueue twice.
        const REPAIR_GRACE_MINUTES: i32 = 5;
        {
            let mut tx = self.pool.begin().await?;
            let orphans =
                TraceRepository::outgoing_without_mail(&mut tx, m.id, REPAIR_GRACE_MINUTES)
                    .await?;
            tx.commit().await?;
            for (trace_id, _rid, email) in orphans {
                match self.enqueue_one(m, trace_id, &email).await {
                    Ok(()) => out.repaired += 1,
                    Err(e) => {
                        tracing::error!(
                            trace_id = %trace_id,
                            error = %e,
                            "orphan repair enqueue failed"
                        );
                    }
                }
            }
        }

        // (2) resolve.
        let domain = parse_domain(&m.mailing_domain)?; // re-pure; claim already vetted
        let recipients = match m.target_model.as_str() {
            "mailing_contact" => {
                let mut tx = self.pool.begin().await?;
                let r = MailingSendRepository::resolve_contact_recipients(
                    &mut tx,
                    &domain,
                    self.cfg.resolver_hard_cap,
                )
                .await?;
                tx.commit().await?;
                r
            }
            "party" => match &self.party_resolver {
                Some(resolver) => resolver.resolve(&domain).await.map_err(|e| {
                    // A resolver failure parks loudly — not a silent empty sweep.
                    MailingWriteError::Conflict(format!("party resolver failed: {e}"))
                })?,
                None => {
                    let mut tx = self.pool.begin().await?;
                    MailingSendRepository::park_mailing(
                        &mut tx,
                        m.id,
                        "target_model='party' but no party resolver is composed",
                    )
                    .await?;
                    tx.commit().await?;
                    out.parked += 1;
                    return Ok(());
                }
            },
            other => {
                let mut tx = self.pool.begin().await?;
                MailingSendRepository::park_mailing(
                    &mut tx,
                    m.id,
                    &format!("unknown target_model '{other}'"),
                )
                .await?;
                tx.commit().await?;
                out.parked += 1;
                return Ok(());
            }
        };
        if recipients.len() as i64 >= self.cfg.resolver_hard_cap {
            return Err(MailingWriteError::Conflict(format!(
                "mailing {} resolved {} recipients (>= hard cap {}) — refuse to silently truncate",
                m.id,
                recipients.len(),
                self.cfg.resolver_hard_cap
            )));
        }
        out.recipients_resolved += recipients.len();

        if recipients.is_empty() {
            // complete_empty — the claim's shortcut edge: no traces, no
            // mail rows, sent_date still stamped (the mailing DID run; its
            // audience was empty).
            let mut tx = self.pool.begin().await?;
            let flipped = MailingSendRepository::complete_empty(&mut tx, m.id).await?;
            tx.commit().await?;
            if flipped {
                out.completed_empty += 1;
            }
            return Ok(());
        }

        // (3) batched suppression memory. Campaign scope keys on ab_test_id,
        // not ab_testing_enabled: the promoted WINNER COPY (sampling off)
        // also carries the test id, and its audience is exactly the campaign
        // remainder — everyone no variant has sent to yet.
        let mut tx = self.pool.begin().await?;
        let disposition = MailingSendRepository::recipient_disposition(
            &mut tx,
            m.id,
            m.campaign_id.filter(|_| m.ab_test_id.is_some()),
        )
        .await?;
        let emails: Vec<String> = recipients.iter().map(|r| r.email.to_lowercase()).collect();
        let blacklisted = if m.use_exclusion_list {
            MailingSendRepository::blacklisted_emails(&mut tx, &emails).await?
        } else {
            Vec::new()
        };
        let opted_out = MailingSendRepository::opted_out_emails(&mut tx, &emails).await?;
        tx.commit().await?;
        let blacklisted: HashSet<String> = blacklisted.into_iter().collect();
        let opted_out: HashSet<String> = opted_out.into_iter().collect();
        let mut seen_active: HashSet<Uuid> = HashSet::new();
        let mut suppressed_here: HashSet<Uuid> = HashSet::new();
        for (rid, active, here) in disposition {
            if active {
                seen_active.insert(rid);
            }
            if here {
                suppressed_here.insert(rid);
            }
        }

        // (4) A/B membership materialized once per sweep.
        let ab_seed = if m.ab_testing_enabled {
            match m.ab_test_id {
                Some(ab_id) => {
                    let mut tx = self.pool.begin().await?;
                    let seed = MailingSendRepository::sampling_seed(&mut tx, ab_id).await?;
                    tx.commit().await?;
                    match seed {
                        Some(s) => Some(s),
                        None => {
                            let mut tx = self.pool.begin().await?;
                            MailingSendRepository::park_mailing(
                                &mut tx,
                                m.id,
                                "ab_testing_enabled but the bound A/B test has no sampling seed",
                            )
                            .await?;
                            tx.commit().await?;
                            out.parked += 1;
                            return Ok(());
                        }
                    }
                }
                None => {
                    let mut tx = self.pool.begin().await?;
                    MailingSendRepository::park_mailing(
                        &mut tx,
                        m.id,
                        "ab_testing_enabled but no ab_test_id is bound",
                    )
                    .await?;
                    tx.commit().await?;
                    out.parked += 1;
                    return Ok(());
                }
            }
        } else {
            None
        };

        // (3)+(4) walk: dedupe by email, suppress visibly, fragment by seed.
        let mut seen_emails: HashSet<String> = HashSet::new();
        let mut to_send: Vec<&ResolvedRecipient> = Vec::new();
        let mut cancel_work: Vec<(&ResolvedRecipient, &'static str)> = Vec::new();
        for r in &recipients {
            let email_lc = r.email.to_lowercase();
            if seen_active.contains(&r.recipient_id) {
                out.skipped_seen += 1;
                seen_emails.insert(email_lc);
                continue;
            }
            if suppressed_here.contains(&r.recipient_id) {
                // Already visibly suppressed in an earlier pass — the first
                // suppression stays THE suppression.
                out.skipped_seen += 1;
                seen_emails.insert(email_lc);
                continue;
            }
            if !seen_emails.insert(email_lc.clone()) {
                cancel_work.push((r, "mail_dup"));
                out.suppressed_dup += 1;
                continue;
            }
            if blacklisted.contains(&email_lc) {
                cancel_work.push((r, "mail_bl"));
                out.suppressed_blacklist += 1;
                continue;
            }
            if opted_out.contains(&email_lc) {
                cancel_work.push((r, "mail_optout"));
                out.suppressed_optout += 1;
                continue;
            }
            if let Some(seed) = &ab_seed {
                if !ab_member(seed, &email_lc, m.ab_testing_pc) {
                    out.skipped_ab_nonmember += 1;
                    continue;
                }
            }
            to_send.push(r);
        }

        // Visible suppression traces + events (one mint transaction).
        for (r, cause) in &cancel_work {
            let trace_id = Uuid::new_v4();
            let mut tx = self.pool.begin().await?;
            match TraceRepository::mint_trace(
                &mut tx,
                trace_id,
                m.id,
                m.campaign_id,
                &m.target_model,
                r.recipient_id,
                &r.email,
                "cancel",
                Some(cause),
                false,
            )
            .await
            {
                Ok(_) => {
                    tx.commit().await?;
                    self.events
                        .recipient_suppressed(m.id, trace_id, &r.email, cause);
                }
                Err(e) if is_unique_violation(&e) => {
                    tx.rollback().await?;
                    out.fence_skips += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }

        // (5) pass budget + enqueue batches.
        let budget = self.cfg.recipients_per_pass as usize;
        let remaining = to_send.len().saturating_sub(budget);
        let work: Vec<&ResolvedRecipient> = to_send.into_iter().take(budget).collect();

        let mut pacer = Pacer::new(self.cfg.max_enqueues_per_second);
        for batch in work.chunks(self.cfg.mail_enqueue_batch.max(1)) {
            let mut tx = self.pool.begin().await?;
            let mut minted: Vec<(Uuid, &ResolvedRecipient)> = Vec::new();
            for r in batch {
                let r: &ResolvedRecipient = r;
                let trace_id = Uuid::new_v4();
                // The fenced mint: a concurrent worker's live trace for this
                // (mailing, recipient) skips the row inside the batch
                // transaction (a raw unique violation would abort it) —
                // logged + counted, never converged.
                let inserted = TraceRepository::mint_trace_fenced(
                    &mut tx,
                    trace_id,
                    m.id,
                    m.campaign_id,
                    &m.target_model,
                    r.recipient_id,
                    &r.email,
                    "outgoing",
                    None,
                    false,
                )
                .await?;
                if inserted {
                    minted.push((trace_id, r));
                    out.minted += 1;
                } else {
                    tracing::warn!(
                        mailing_id = %m.id,
                        recipient_id = %r.recipient_id,
                        "mint fence: live trace already exists, skipping"
                    );
                    out.fence_skips += 1;
                }
            }
            tx.commit().await?;

            for (trace_id, r) in minted {
                match self.enqueue_one(m, trace_id, &r.email).await {
                    Ok(()) => out.enqueued += 1,
                    Err(e) => {
                        // One recipient's enqueue failure never fails the
                        // batch: the trace flips to error with the seam
                        // reason; retry_failed is the operator's re-queue.
                        let mut tx = self.pool.begin().await?;
                        TraceRepository::set_failed(
                            &mut tx,
                            trace_id,
                            "mail_smtp",
                            Some(&e.to_string()),
                        )
                        .await?;
                        tx.commit().await?;
                        tracing::error!(
                            trace_id = %trace_id,
                            error = %e,
                            "enqueue failed at the mail seam"
                        );
                    }
                }
                pacer.wait().await;
            }
            if self.cfg.inter_batch_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.cfg.inter_batch_delay_ms)).await;
            }
        }

        // (6) complete — or stay `sending` when the budget ran out. The
        // flip is state-guarded: a concurrent worker that completed this
        // mailing first returns false, and only the worker that moved the
        // edge counts the completion.
        if remaining > 0 {
            out.still_sending += 1;
        } else {
            let mut tx = self.pool.begin().await?;
            let flipped = MailingSendRepository::complete_mailing(&mut tx, m.id).await?;
            tx.commit().await?;
            if flipped {
                out.completed += 1;
            }
        }
        Ok(())
    }

    /// The per-recipient enqueue seam: `message_post` (empty recipients —
    /// mints the message row only) + `enqueue` into the outgoing queue, then
    /// attach the mail row id onto the trace. Always through backbone-mail's
    /// PUBLIC services — never a direct write into `messaging`.
    async fn enqueue_one(
        &self,
        m: &ClaimedMailing,
        trace_id: Uuid,
        email: &str,
    ) -> Result<(), MailingWriteError> {
        let posted = self
            .messages
            .message_post(MessagePostCommand {
                body: m.body_html.clone(),
                subject: Some(m.subject.clone()),
                message_type: "email".into(),
                subtype_id: None,
                subtype_name: None,
                is_internal: false,
                author_id: None,
                author_guest_id: None,
                email_from: Some(m.email_from.clone()),
                reply_to: m.reply_to.clone(),
                model: Some("mailing".into()),
                res_id: Some(m.id),
                record_name: Some(m.subject.clone()),
                recipients: Vec::new(),
            })
            .await
            .map_err(|e| MailingWriteError::MailSeam(e.to_string()))?;
        let mail_id = self
            .queue
            .enqueue(
                posted.message_id,
                email,
                None,
                m.reply_to.as_deref(),
                None,
                Some("mailing"),
                Some(m.id),
            )
            .await?;
        let mut tx = self.pool.begin().await?;
        TraceRepository::attach_mail_id(&mut tx, trace_id, mail_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Step (9): promote due A/B winners. `manual` tests never auto-promote
    /// (a human completes them); the completion stamp is guarded, so a
    /// second sweep no-ops.
    async fn promote_due_ab_tests(&self, out: &mut SweepOutcome) -> Result<(), MailingWriteError> {
        let mut tx = self.pool.begin().await?;
        let due = MailingSendRepository::ab_tests_due_for_promotion(&mut tx).await?;
        tx.commit().await?;
        for (ab_test_id, selection) in due {
            if selection == "manual" {
                continue;
            }
            let mut tx = self.pool.begin().await?;
            let ranked =
                MailingSendRepository::rank_variants_by_metric(&mut tx, ab_test_id, &selection)
                    .await?;
            let Some((winner_mailing_id, _ratio)) = ranked.first().copied() else {
                tx.rollback().await?;
                continue; // no done variant yet — wait for the next sweep
            };
            let promoted_id = Uuid::new_v4();
            MailingSendRepository::promote_winner(&mut tx, promoted_id, winner_mailing_id, ab_test_id)
                .await?;
            MailingSendRepository::queue_promoted_winner(&mut tx, promoted_id).await?;
            let stamped =
                MailingSendRepository::complete_ab_test(&mut tx, ab_test_id, winner_mailing_id)
                    .await?;
            tx.commit().await?;
            if stamped {
                out.ab_promotions += 1;
                self.events
                    .ab_test_winner_promoted(ab_test_id, winner_mailing_id);
            }
        }
        Ok(())
    }
}

// ── domain parsing (the refuse-loudly fence) ────────────────────────────────

/// The field whitelist — parse accepts nothing outside this map.
fn field_of(raw: &str) -> Option<DomainField> {
    match raw {
        "email" => Some(DomainField::Email),
        "name" => Some(DomainField::Name),
        "first_name" => Some(DomainField::FirstName),
        "last_name" => Some(DomainField::LastName),
        "company_name" => Some(DomainField::CompanyName),
        "country_code" => Some(DomainField::CountryCode),
        "mailing_audience_id" => Some(DomainField::MailingAudienceId),
        _ => None,
    }
}

/// Parse a mailing domain from EITHER accepted input shape:
/// - Odoo triple terms: `[["email", "like", "acme"], ...]`
/// - canonical object terms: `[{"field": "email", "op": "like", "value": "acme"}]`
///
/// Every deviation is a typed `DomainInvalid` — the caller refuses loudly
/// (422 at write time; a park at send time). An EMPTY array parses to an
/// empty domain (matches every contact) — that is a legitimate "send to all
/// contacts" and stays legal, unlike Odoo's implicit falsy-domain trap.
pub fn parse_domain(raw: &serde_json::Value) -> Result<CompiledDomain, DomainInvalid> {
    let terms_json = raw.as_array().ok_or(DomainInvalid::NotAnArray)?;
    let mut terms = Vec::with_capacity(terms_json.len());
    for (index, tj) in terms_json.iter().enumerate() {
        let (field_raw, op_raw, value): (String, String, serde_json::Value) = if let Some(arr) =
            tj.as_array()
        {
            if arr.len() != 3 {
                return Err(DomainInvalid::MalformedTerm { index });
            }
            let f = arr[0]
                .as_str()
                .ok_or(DomainInvalid::FieldNotString { index })?
                .to_string();
            let o = match arr[1].as_str() {
                Some(s) => s.to_string(),
                None => return Err(DomainInvalid::UnknownOp { index }),
            };
            (f, o, arr[2].clone())
        } else if let Some(obj) = tj.as_object() {
            let f = obj
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or(DomainInvalid::FieldNotString { index })?
                .to_string();
            let o = obj
                .get("op")
                .and_then(|v| v.as_str())
                .ok_or(DomainInvalid::UnknownOp { index })?
                .to_string();
            let v = obj
                .get("value")
                .cloned()
                .ok_or(DomainInvalid::MalformedTerm { index })?;
            (f, o, v)
        } else {
            return Err(DomainInvalid::MalformedTerm { index });
        };

        let field = field_of(&field_raw).ok_or(DomainInvalid::UnknownField(field_raw.clone()))?;
        let op = match op_raw.as_str() {
            "=" => DomainOp::Eq,
            "!=" => DomainOp::Ne,
            "in" => DomainOp::In,
            "not in" => DomainOp::NotIn,
            "like" => DomainOp::Like,
            "not like" => DomainOp::NotLike,
            _ => return Err(DomainInvalid::UnknownOp { index }),
        };

        let values: Vec<String> = match (&op, &value) {
            (DomainOp::In | DomainOp::NotIn, serde_json::Value::Array(a)) => {
                let mut vs = Vec::with_capacity(a.len());
                for v in a {
                    vs.push(
                        v.as_str()
                            .ok_or(DomainInvalid::ValueNotArray { index })?
                            .to_string(),
                    );
                }
                vs
            }
            (DomainOp::In | DomainOp::NotIn, _) => {
                return Err(DomainInvalid::ValueNotArray { index })
            }
            (_, serde_json::Value::String(s)) => vec![s.clone()],
            (_, serde_json::Value::Array(a)) if a.len() == 1 => vec![a[0]
                .as_str()
                .ok_or(DomainInvalid::ValueNotScalar { index })?
                .to_string()],
            _ => return Err(DomainInvalid::ValueNotScalar { index }),
        };

        if field == DomainField::MailingAudienceId {
            for v in &values {
                if Uuid::parse_str(v).is_err() {
                    return Err(DomainInvalid::BadAudienceUuid {
                        index,
                        raw: v.clone(),
                    });
                }
            }
        }

        terms.push(DomainTerm { field, op, values });
    }
    Ok(CompiledDomain { terms })
}

/// Canonical stored form: array of `{"field","op","value"}` objects. The DB
/// is never asked to re-derive Odoo-triple shapes.
pub fn canonical_domain_json(domain: &CompiledDomain) -> serde_json::Value {
    serde_json::Value::Array(
        domain
            .terms
            .iter()
            .map(|t| {
                let value = if t.values.len() == 1
                    && !matches!(t.op, DomainOp::In | DomainOp::NotIn)
                {
                    serde_json::Value::String(t.values[0].clone())
                } else {
                    serde_json::Value::Array(
                        t.values
                            .iter()
                            .map(|v| serde_json::Value::String(v.clone()))
                            .collect(),
                    )
                };
                serde_json::json!({
                    "field": field_name(t.field),
                    "op": op_name(t.op),
                    "value": value,
                })
            })
            .collect(),
    )
}

fn field_name(f: DomainField) -> &'static str {
    match f {
        DomainField::Email => "email",
        DomainField::Name => "name",
        DomainField::FirstName => "first_name",
        DomainField::LastName => "last_name",
        DomainField::CompanyName => "company_name",
        DomainField::CountryCode => "country_code",
        DomainField::MailingAudienceId => "mailing_audience_id",
    }
}

fn op_name(op: DomainOp) -> &'static str {
    match op {
        DomainOp::Eq => "=",
        DomainOp::Ne => "!=",
        DomainOp::In => "in",
        DomainOp::NotIn => "not in",
        DomainOp::Like => "like",
        DomainOp::NotLike => "not like",
    }
}

// ── A/B membership (the persisted-seed sampler) ─────────────────────────────

/// Deterministic fragment membership: HMAC-SHA256(seed, recipient_email),
/// first 8 bytes big-endian as u64, mod 100 < pc. The seed is PERSISTED at
/// test creation and never regenerated — membership is stable across
/// sweeps, restarts, and later re-sends.
pub fn ab_member(seed: &str, recipient_email: &str, pc: i32) -> bool {
    let mut mac = match Hmac::<Sha256>::new_from_slice(seed.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(recipient_email.as_bytes());
    let digest = mac.finalize().into_bytes();
    let bucket = u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3],
        digest[4], digest[5], digest[6], digest[7],
    ]);
    (bucket % 100) < pc as u64
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Postgres unique-violation (23505) — the mint fence's signature.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

/// Enqueue rate pacing: when a ceiling is set, consecutive enqueues are
/// spaced at >= 1000/eps ms. Sleep-based, per worker — honest pacing for a
/// provider contract, not a token bucket with debt.
struct Pacer {
    min_interval: Option<Duration>,
    last: Option<Instant>,
}

impl Pacer {
    fn new(max_per_second: Option<u64>) -> Self {
        Self {
            min_interval: max_per_second
                .filter(|eps| *eps > 0)
                .map(|eps| Duration::from_millis(1000 / eps)),
            last: None,
        }
    }

    async fn wait(&mut self) {
        if let (Some(min), Some(last)) = (self.min_interval, self.last) {
            let elapsed = last.elapsed();
            if elapsed < min {
                tokio::time::sleep(min - elapsed).await;
            }
        }
        self.last = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_odoo_triples() {
        let raw = serde_json::json!([["email", "like", "acme"], ["country_code", "=", "ID"]]);
        let d = parse_domain(&raw).unwrap();
        assert_eq!(d.terms.len(), 2);
        assert_eq!(canonical_domain_json(&d)[0]["op"], "like");
    }

    #[test]
    fn parse_accepts_canonical_objects() {
        let raw = serde_json::json!([
            {"field": "email", "op": "in", "value": ["a@x.id", "b@x.id"]}
        ]);
        let d = parse_domain(&raw).unwrap();
        assert_eq!(d.terms[0].op, DomainOp::In);
    }

    #[test]
    fn parse_refuses_unknown_fields_loudly() {
        let raw = serde_json::json!([["1; DROP TABLE", "=", "x"]]);
        assert!(matches!(
            parse_domain(&raw),
            Err(DomainInvalid::UnknownField(_))
        ));
    }

    #[test]
    fn parse_refuses_unknown_ops_loudly() {
        let raw = serde_json::json!([["email", "ilike", "x"]]);
        assert!(matches!(parse_domain(&raw), Err(DomainInvalid::UnknownOp { .. })));
    }

    #[test]
    fn parse_refuses_non_array_loudly() {
        let raw = serde_json::json!({"field": "email"});
        assert!(matches!(parse_domain(&raw), Err(DomainInvalid::NotAnArray)));
    }

    #[test]
    fn parse_refuses_bad_audience_uuid_loudly() {
        let raw = serde_json::json!([["mailing_audience_id", "=", "not-a-uuid"]]);
        assert!(matches!(
            parse_domain(&raw),
            Err(DomainInvalid::BadAudienceUuid { .. })
        ));
    }

    #[test]
    fn empty_domain_is_legal_matches_all() {
        let d = parse_domain(&serde_json::json!([])).unwrap();
        assert!(d.terms.is_empty());
    }

    #[test]
    fn ab_membership_is_deterministic_and_roughly_proportional() {
        let seed = "cafebabedeadbeef";
        let mut members = 0u32;
        let n = 2000;
        for i in 0..n {
            let email = format!("user{i}@example.id");
            assert_eq!(ab_member(seed, &email, 20), ab_member(seed, &email, 20));
            if ab_member(seed, &email, 20) {
                members += 1;
            }
        }
        // ~20% ± 5 absolute on 2000 samples — HMAC buckets are uniform.
        let pct = members as f64 / n as f64 * 100.0;
        assert!((12.0..=28.0).contains(&pct), "membership pct was {pct}");
        // A different seed flips membership for SOME emails (not identity).
        let flipped = (0..100)
            .filter(|i| {
                ab_member(seed, &format!("u{i}@x.id"), 50)
                    != ab_member("other-seed", &format!("u{i}@x.id"), 50)
            })
            .count();
        assert!(flipped > 0);
    }
}
