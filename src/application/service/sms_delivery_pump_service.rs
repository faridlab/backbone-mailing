//! `SmsDeliveryPumpService` — asynchronous done-inference for sms-type
//! mailings, driven from the delivery tracker (hand-authored, user-owned;
//! see `metaphor.codegen.yaml`).
//!
//! The mail channel completes its mailing synchronously at walk end — the
//! sweep's own last step owns that flip, and that behavior is unchanged and
//! mail-only. The SMS channel structurally cannot: after the send walk
//! hands every recipient to the gateway, DONE is a DELIVERY question, and
//! delivery verdicts arrive asynchronously (the signed delivery-report
//! webhook, or the drainer's own outcome when no report ever comes). This
//! pump is that channel's completion clock.
//!
//! It rides the SINGLE existing send job as an extra sweep step — no
//! second cron. Restart durability follows from the shapes it composes:
//! every trace write is a rank-guarded conditional UPDATE (a replay is a
//! no-op skip), the walk-complete marker is a guarded metadata stamp, and
//! the completion is the state-guarded `sending → done` verb (a replay
//! matches zero rows). A process dying mid-pass loses nothing; the next
//! pass re-reads the same facts and converges.
//!
//! One pass, two arms:
//!
//! 1. ADVANCE — live sms-type traces still in a transient status
//!    (outgoing/process/pending), each joined to its delivery tracker by
//!    the sms_uuid seam. The tracker is the durable fact on BOTH verdict
//!    paths (the webhook mirrors it; the drainer's outcome application
//!    mirrors it) and it survives sms-row GC — a read-only cross-schema
//!    join, the settled-mails reconcile's precedent in the opposite
//!    direction. Each tracker verdict maps through the channel-state
//!    mapping (the port of upstream's SMS_STATE_TO_TRACE_STATUS) onto one
//!    trace verb.
//! 2. INFER DONE — for sms mailings whose walk ended (the durable
//!    `sms_walk_complete` metadata marker, stamped by the walk itself):
//!    lock the mailing row `FOR UPDATE` under the full candidacy
//!    predicate, re-check that no transient trace remains UNDER the lock,
//!    then complete through the state-guarded verb. Lock BEFORE check is
//!    the whole point: two racing verdict-consumers (or a pump racing
//!    itself after a restart) serialize on the row lock, the second one's
//!    re-check sees zero candidates-or-remaining, and it no-ops instead of
//!    double-stamping sent_date or re-raising kpi_mail_required.

use sqlx::PgPool;

use crate::application::service::trace_write_service::SMS_FAILURE_CODES;
use crate::infrastructure::persistence::mailing_send_repository::MailingSendRepository;
use crate::infrastructure::persistence::trace_repository::{SmsTraceVerdict, TraceRepository};

/// Transient traces examined per pass. The job runs on a schedule, so a
/// backlog larger than this drains over consecutive runs — each pass is
/// idempotent, partial progress is safe.
const TRACE_BATCH: i64 = 5000;

/// Walked-out mailings examined per pass (the lock candidates).
const CANDIDATE_BATCH: i64 = 500;

/// The delivery-report codes that mean BOUNCE (permanent, recipient-side)
/// rather than a transient transport error — upstream's bounce-class set,
/// carried verbatim. On the trace these land as `bounce` (with the code)
/// through the channel-pure verb; every other failure code lands as
/// `error` through `set_failed`.
const BOUNCE_CLASS_CODES: &[&str] = &["sms_invalid_destination", "sms_not_allowed", "sms_rejected"];

/// What one pump pass did — the observable tests and the sweep assert
/// against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmsPumpOutcome {
    /// Traces advanced by a tracker verdict this pass.
    pub traces_advanced: usize,
    /// Verdicts that matched zero rows — the trace already at/past the
    /// verb's rank (idempotent replay: restart, sweep re-entry, job
    /// overlap). Never an error condition.
    pub trace_skips: usize,
    /// Walked-out sms mailings examined for done.
    pub candidates: usize,
    /// Mailings flipped to done by the inference this pass.
    pub mailings_completed: usize,
}

/// The channel-state mapping: tracker state (+ its failure code) → the
/// trace verb to apply. The port of upstream's SMS_STATE_TO_TRACE_STATUS
/// plus its bounce-class refinement (`_update_sms_traces`): exception-state
/// verdicts whose code is in the bounce class land as trace `bounce`, all
/// others as trace `error`.
enum VerdictAction {
    /// Nothing to do (tracker still pre-dispatch).
    Skip,
    Process,
    Pending,
    Sent,
    /// Trace `bounce` with the SMS failure code carried verbatim.
    Bounce(&'static str),
    /// Trace `error` with a member failure code.
    Failed(&'static str),
    /// Trace `cancel` (from outgoing only — the rank guard handles rows
    /// the pump already advanced past).
    Canceled(&'static str),
}

fn map_verdict(tracker_state: &str, failure_type: Option<&str>) -> VerdictAction {
    // The code a verb will carry: the tracker's own when it is a member of
    // the trace failure vocabulary, else the channel-appropriate default.
    // Non-members (the mail channel's codes, the tracker's unclassified
    // `unknown`) never reach a verb — the cast inside would abort the
    // caller's transaction. The member is returned as the vocabulary's own
    // static slot, never as a borrow of the input.
    let code_or = |fallback: &'static str| -> &'static str {
        match failure_type {
            Some(code) => match SMS_FAILURE_CODES.iter().copied().find(|m| *m == code) {
                Some(member) => member,
                None => fallback,
            },
            None => fallback,
        }
    };
    match tracker_state {
        "ready" => VerdictAction::Skip,
        "process" => VerdictAction::Process,
        "pending" => VerdictAction::Pending,
        "sent" => VerdictAction::Sent,
        "bounce" => VerdictAction::Bounce(code_or("sms_not_delivered")),
        "exception" => match failure_type {
            Some(code) if BOUNCE_CLASS_CODES.contains(&code) => {
                VerdictAction::Bounce(code_or("sms_not_delivered"))
            }
            _ => VerdictAction::Failed(code_or("sms_server")),
        },
        "canceled" => VerdictAction::Canceled(code_or("sms_blacklist")),
        other => {
            tracing::warn!(
                tracker_state = other,
                "unknown delivery-tracker state; leaving the trace transient"
            );
            VerdictAction::Skip
        }
    }
}

/// The SMS delivery-tracker pump. Stateless over a pool.
pub struct SmsDeliveryPumpService {
    pool: PgPool,
}

impl SmsDeliveryPumpService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// One pump pass (the send job's last sweep step). Both arms are
    /// independently idempotent; a pass that dies midway leaves every
    /// landed write standing and every unlanded one still derivable.
    pub async fn pump_once(&self) -> Result<SmsPumpOutcome, sqlx::Error> {
        let mut out = SmsPumpOutcome::default();

        // Arm 1 — advance transient traces from tracker verdicts. One small
        // transaction per trace: a verdict that cannot land (infra fault)
        // fails the pass loudly, and every verdict that already landed
        // stays landed.
        let verdicts = {
            let mut conn = self.pool.acquire().await?;
            TraceRepository::sms_traces_with_tracker_verdicts(&mut conn).await?
        };
        for verdict in &verdicts {
            if self.apply_verdict(verdict).await? {
                out.traces_advanced += 1;
            } else {
                out.trace_skips += 1;
            }
        }

        // Arm 2 — infer done for walked-out sms mailings. Candidate scan is
        // lock-free; the per-candidate transaction re-establishes candidacy
        // under FOR UPDATE before any check or write.
        let candidates = {
            let mut conn = self.pool.acquire().await?;
            MailingSendRepository::sms_done_inference_candidates(&mut conn, CANDIDATE_BATCH).await?
        };
        out.candidates = candidates.len();
        for mailing_id in candidates {
            let mut tx = self.pool.begin().await?;
            let Some(locked) =
                MailingSendRepository::lock_mailing_for_done(&mut tx, mailing_id).await?
            else {
                // Candidacy ended between scan and lock (completed, parked,
                // or deleted by a concurrent pass) — the lock's zero-row
                // result IS the re-check.
                continue;
            };
            let remaining = TraceRepository::count_transient_sms_traces(&mut tx, locked).await?;
            let mut completed = false;
            if remaining == 0 {
                completed = MailingSendRepository::complete_mailing(&mut tx, locked).await?;
            }
            tx.commit().await?;
            if completed {
                tracing::info!(
                    mailing_id = %locked,
                    "sms mailing completed by the delivery-tracker pump"
                );
                out.mailings_completed += 1;
            }
        }

        Ok(out)
    }

    /// Apply one tracker verdict as its mapped trace verb, in the verb's
    /// own small transaction. True when the row advanced; false when the
    /// rank guard matched zero rows (the idempotent skip).
    async fn apply_verdict(&self, verdict: &SmsTraceVerdict) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let moved = match map_verdict(&verdict.tracker_state, verdict.failure_type.as_deref()) {
            VerdictAction::Skip => {
                tx.commit().await?;
                return Ok(false);
            }
            VerdictAction::Process => {
                TraceRepository::set_process(&mut tx, verdict.trace_id).await?
            }
            VerdictAction::Pending => {
                TraceRepository::set_pending(&mut tx, verdict.trace_id).await?
            }
            VerdictAction::Sent => TraceRepository::set_sent(&mut tx, verdict.trace_id).await?,
            VerdictAction::Bounce(code) => {
                TraceRepository::set_bounced_sms(
                    &mut tx,
                    verdict.trace_id,
                    code,
                    verdict.failure_reason.as_deref(),
                )
                .await?
            }
            VerdictAction::Failed(code) => {
                TraceRepository::set_failed(
                    &mut tx,
                    verdict.trace_id,
                    code,
                    verdict.failure_reason.as_deref(),
                )
                .await?
            }
            VerdictAction::Canceled(code) => {
                TraceRepository::set_canceled(&mut tx, verdict.trace_id, code).await?
            }
        };
        tx.commit().await?;
        Ok(moved)
    }
}
