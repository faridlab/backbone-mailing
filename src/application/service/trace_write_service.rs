//! `TraceWriteService` — the per-recipient trace verbs (hand-authored,
//! user-owned; see `metaphor.codegen.yaml`).
//!
//! Each verb is ONE conditional UPDATE whose WHERE arm is the declared
//! transition's legal source set (schema/hooks/trace_status.hook.yaml) —
//! the monotonic rank guard. Outcomes are typed, never booleanly silent:
//!
//! - `Moved`   — this call advanced the row;
//! - `Skipped` — the row is already at/past this verb's rank (idempotent
//!   replay: pixel fires twice, click route replays, sweep reconciles a
//!   settled trace again);
//! - `Missing` — no live row (unknown id, or soft-deleted).
//!
//! `set_clicked` stamps the click datetime WITHOUT moving state — the
//! public click route always calls `set_opened` + `set_clicked` together,
//! which is exactly the declared edge pair.

use uuid::Uuid;

use crate::infrastructure::persistence::trace_repository::{TraceRepository, TraceRow};

/// What one trace verb did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceTransition {
    Moved,
    Skipped,
    Missing,
}

/// Typed error surface (house style).
#[derive(Debug, thiserror::Error)]
pub enum TraceWriteError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid: {0}")]
    Invalid(String),
}

impl TraceWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "mailing_db_error",
            Self::Invalid(_) => "invalid_input",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::Db(_) => 500,
            Self::Invalid(_) => 422,
        }
    }
}

/// The trace verbs. Stateless over a pool; one transaction per verb.
pub struct TraceWriteService {
    pool: sqlx::PgPool,
}

impl TraceWriteService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// The trace row (live) — the read side the routes echo.
    pub async fn find(&self, trace_id: Uuid) -> Result<Option<TraceRow>, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        let row = TraceRepository::find_live(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// `set_process` (SMS channel only) — the delivery-tracker pump saw the
    /// channel hold the message (tracker 'process': queued at the gateway
    /// before dispatch). Mail-channel rows never match: their rank never
    /// leaves outgoing before a transport verdict.
    pub async fn set_process(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_process(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_pending` (SMS channel only) — handed to the gateway, awaiting
    /// the delivery report (displays 'Sent'). A tracker already past
    /// 'process' advances the trace in one compressed step from outgoing.
    pub async fn set_pending(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_pending(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_bounced_sms` (SMS channel only) — the delivery report's bounce
    /// verdict with the SMS failure code carried verbatim. Channel-pure by
    /// construction: this verb can never write `mail_bounce` (the email
    /// auto-blacklist's fact source) and never stamps open_datetime. The
    /// caller (the delivery-tracker pump) whitelists the code against the
    /// trace failure vocabulary before calling — an unknown code is refused
    /// here rather than surfacing as a cast failure inside the verb.
    pub async fn set_bounced_sms(
        &self,
        trace_id: Uuid,
        failure_type: &str,
        failure_reason: Option<&str>,
    ) -> Result<TraceTransition, TraceWriteError> {
        if !is_sms_failure_code(failure_type) {
            return Err(TraceWriteError::Invalid(format!(
                "not an SMS failure code: {failure_type}"
            )));
        }
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved =
            TraceRepository::set_bounced_sms(&mut tx, trace_id, failure_type, failure_reason)
                .await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// Stamp the enqueued sms row's external uuid (the SMS twin of the
    /// mail_id seam) — conditional on NULL, exactly like the mail link: a
    /// replayed enqueue repair never overwrites a standing link.
    pub async fn attach_sms_uuid(
        &self,
        trace_id: Uuid,
        sms_uuid: &str,
    ) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        let moved = TraceRepository::attach_sms_uuid(&mut tx, trace_id, sms_uuid).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_sent` — the transport's acceptance stamp.
    pub async fn set_sent(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_sent(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_opened` — the pixel fact.
    pub async fn set_opened(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_opened(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_replied` — the inbound reply fact.
    pub async fn set_replied(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_replied(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_bounced` — stamps failure_type='mail_bounce' (the
    /// auto-blacklist window's fact source).
    pub async fn set_bounced(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_bounced(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_clicked` — the click stamp; state moves only via set_opened
    /// (the click route calls both).
    pub async fn set_clicked(&self, trace_id: Uuid) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        if TraceRepository::find_live(&mut tx, trace_id).await?.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_clicked(&mut tx, trace_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_failed` — the transport's refusal, with the typed cause.
    pub async fn set_failed(
        &self,
        trace_id: Uuid,
        failure_type: &str,
        failure_reason: Option<&str>,
    ) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        let row = TraceRepository::find_live(&mut tx, trace_id).await?;
        if row.is_none() {
            tx.commit().await?;
            return Ok(TraceTransition::Missing);
        }
        let moved = TraceRepository::set_failed(&mut tx, trace_id, failure_type, failure_reason)
            .await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// `set_canceled` — send-time suppression; only from `outgoing`.
    pub async fn set_canceled(
        &self,
        trace_id: Uuid,
        failure_type: &str,
    ) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        let moved = TraceRepository::set_canceled(&mut tx, trace_id, failure_type).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }

    /// Stamp the RFC Message-ID (the inbound reply/bounce match key).
    pub async fn set_message_id(
        &self,
        trace_id: Uuid,
        message_id: &str,
    ) -> Result<TraceTransition, TraceWriteError> {
        let mut tx = self.pool.begin().await?;
        let moved = TraceRepository::set_message_id(&mut tx, trace_id, message_id).await?;
        tx.commit().await?;
        Ok(classify(moved))
    }
}

fn classify(moved: bool) -> TraceTransition {
    if moved {
        TraceTransition::Moved
    } else {
        TraceTransition::Skipped
    }
}

/// The SMS transport-verdict codes that are members of trace_failure_type
/// (the vocabulary schema/models/trace.model.yaml declares and the overlay
/// migration adds). The delivery-tracker pump maps tracker verdicts onto
/// this set before calling `set_bounced_sms` — a code outside it (the mail
/// channel's, or the tracker's unclassified `unknown`) never reaches a verb;
/// the pump substitutes the channel-appropriate default instead, keeping
/// every failure code written to a trace a declared member by construction.
pub const SMS_FAILURE_CODES: &[&str] = &[
    "sms_number_missing",
    "sms_number_format",
    "sms_country_not_supported",
    "sms_registration_needed",
    "sms_credit",
    "sms_server",
    "sms_acc",
    "sms_blacklist",
    "sms_duplicate",
    "sms_optout",
    "sms_expired",
    "sms_invalid_destination",
    "sms_not_allowed",
    "sms_not_delivered",
    "sms_rejected",
];

/// Membership probe for the SMS failure vocabulary above.
pub fn is_sms_failure_code(code: &str) -> bool {
    SMS_FAILURE_CODES.contains(&code)
}
