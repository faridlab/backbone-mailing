//! `MailingStatsReadService` — the KPI read side (hand-authored,
//! user-owned; see `metaphor.codegen.yaml`).
//!
//! ONE grouped query per mailing set — the ~30 KPI computes are read-side,
//! non-stored (Odoo's deferred compute does not port to a stateless
//! service). The documented sent/delivered asymmetry is preserved
//! verbatim: `sent` counts traces that were EVER submitted
//! (`sent_datetime IS NOT NULL`); `delivered` counts status CURRENTLY in
//! sent/open/reply. Clicks count `links_click_datetime IS NOT NULL`
//! independent of state (a click that arrives after a bounce is still a
//! click fact).

use uuid::Uuid;

use crate::infrastructure::persistence::trace_repository::{
    MailingTraceCounts, TraceRepository,
};

/// One mailing's aggregate + derived ratios.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MailingStats {
    pub mailing_id: Uuid,
    pub total: i64,
    pub sent: i64,
    pub delivered: i64,
    pub opened: i64,
    pub clicked: i64,
    pub replied: i64,
    pub bounced: i64,
    pub errored: i64,
    pub canceled: i64,
    /// opened/sent percent — None when nothing was sent (no fake 0%).
    pub opened_ratio: Option<i64>,
    /// clicked/sent percent.
    pub clicks_ratio: Option<i64>,
    /// replied/sent percent.
    pub replied_ratio: Option<i64>,
}

impl From<(Uuid, MailingTraceCounts)> for MailingStats {
    fn from((mailing_id, c): (Uuid, MailingTraceCounts)) -> Self {
        Self {
            mailing_id,
            total: c.total,
            sent: c.sent,
            delivered: c.delivered,
            opened: c.opened,
            clicked: c.clicked,
            replied: c.replied,
            bounced: c.bounced,
            errored: c.errored,
            canceled: c.canceled,
            opened_ratio: c.opened_ratio(),
            clicks_ratio: c.clicks_ratio(),
            replied_ratio: c.replied_ratio(),
        }
    }
}

/// Typed error surface (house style).
#[derive(Debug, thiserror::Error)]
pub enum MailingStatsError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

impl MailingStatsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "mailing_db_error",
            Self::NotFound(_) => "not_found",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::Db(_) => 500,
            Self::NotFound(_) => 404,
        }
    }
}

/// The stats read service. Stateless over a pool.
pub struct MailingStatsReadService {
    pool: sqlx::PgPool,
}

impl MailingStatsReadService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Stats for a set of mailings — ONE grouped query.
    pub async fn stats_for_mailings(
        &self,
        mailing_ids: &[Uuid],
    ) -> Result<Vec<MailingStats>, MailingStatsError> {
        let mut tx = self.pool.begin().await?;
        let rows = TraceRepository::counts_for_mailings(&mut tx, mailing_ids).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(MailingStats::from).collect())
    }

    /// Stats for one mailing — NotFound when the id is unknown.
    pub async fn stats_for_mailing(
        &self,
        mailing_id: Uuid,
    ) -> Result<MailingStats, MailingStatsError> {
        let all = self.stats_for_mailings(&[mailing_id]).await?;
        all.into_iter()
            .next()
            .ok_or_else(|| MailingStatsError::NotFound(format!("mailing {mailing_id}")))
    }
}
