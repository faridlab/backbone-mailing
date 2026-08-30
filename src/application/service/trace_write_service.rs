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
