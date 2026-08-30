//! `AbTestWriteService` — seeded A/B test management (hand-authored,
//! user-owned; see `metaphor.codegen.yaml`).
//!
//! The invariant this service guards: a test's `sampling_seed` is minted
//! EXACTLY ONCE, at creation (128 random bits, hex), and never regenerated.
//! Fragment membership — HMAC-SHA256(seed, recipient_email) mod 100 < pc —
//! is therefore stable across sweeps, restarts, and later re-sends. One
//! live test per campaign (the partial UNIQUE); variants bind as DRAFTS.
//!
//! Auto-promotion runs inside the send sweep (ordered, idempotent). This
//! service owns the MANUAL half: `select_winner` completes a 'manual'
//! test — copies the chosen variant at ab_testing_pc=100 (sampling
//! disabled) and queues the winner copy.

use rand::RngCore;
use uuid::Uuid;

use crate::infrastructure::persistence::mailing_send_repository::MailingSendRepository;

/// Typed error surface (house style).
#[derive(Debug, thiserror::Error)]
pub enum AbTestWriteError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid: {0}")]
    Invalid(String),
}

impl AbTestWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "mailing_db_error",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "ab_test_conflict",
            Self::Invalid(_) => "invalid_input",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::Db(_) => 500,
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::Invalid(_) => 422,
        }
    }
}

/// One A/B test's public shape (the read side).
#[derive(Debug, Clone)]
pub struct AbTestView {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub winner_selection: String,
    pub promote_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed: bool,
    pub winner_mailing_id: Option<Uuid>,
    pub sampling_seed: String,
    pub variants: Vec<(Uuid, String)>,
}

/// The A/B test verbs. Stateless over a pool; one transaction per verb.
pub struct AbTestWriteService {
    pool: sqlx::PgPool,
}

impl AbTestWriteService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Create a test for a campaign, MINTING the persisted sampling seed
    /// (once — 128 random bits as 32 hex chars). One live test per
    /// campaign: the partial UNIQUE refuses a second loudly.
    pub async fn create_ab_test(
        &self,
        campaign_id: Uuid,
        winner_selection: &str,
        promote_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Uuid, AbTestWriteError> {
        let selection = match winner_selection {
            "manual" | "opened_ratio" | "clicks_ratio" | "replied_ratio" => winner_selection,
            other => {
                return Err(AbTestWriteError::Invalid(format!(
                    "winner_selection must be manual | opened_ratio | clicks_ratio | replied_ratio, not {other}"
                )))
            }
        };
        if selection != "manual" && promote_at.is_none() {
            return Err(AbTestWriteError::Invalid(
                "an automatic selection needs promote_at (manual tests need none)".into(),
            ));
        }
        let mut seed_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut seed_bytes);
        let sampling_seed: String = seed_bytes.iter().map(|b| format!("{b:02x}")).collect();

        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        match MailingSendRepository::insert_ab_test(
            &mut tx,
            id,
            campaign_id,
            selection,
            promote_at,
            &sampling_seed,
        )
        .await
        {
            Ok(_) => {}
            // The partial UNIQUE(campaign_id) — one live test per campaign.
            Err(e) if is_unique_violation(&e) => {
                tx.rollback().await?;
                return Err(AbTestWriteError::Conflict(format!(
                    "campaign {campaign_id} already carries a live A/B test"
                )));
            }
            Err(e) => return Err(e.into()),
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Bind a DRAFT mailing as a variant at `pc` percent. The binding also
    /// stamps the test's campaign onto the mailing when unattributed —
    /// variants share the campaign, which is what the cross-variant
    /// seen-list dedupe keys on.
    pub async fn bind_variant(
        &self,
        mailing_id: Uuid,
        ab_test_id: Uuid,
        pc: i32,
    ) -> Result<(), AbTestWriteError> {
        if !(0..=100).contains(&pc) {
            return Err(AbTestWriteError::Invalid(format!(
                "ab_testing_pc must be 0..=100, not {pc}"
            )));
        }
        let mut tx = self.pool.begin().await?;
        let bound = MailingSendRepository::bind_variant(&mut tx, mailing_id, ab_test_id, pc).await?;
        tx.commit().await?;
        match bound {
            Some(_) => Ok(()),
            None => {
                let state = {
                    let mut tx = self.pool.begin().await?;
                    let s = MailingSendRepository::find_live_state(&mut tx, mailing_id).await?;
                    tx.commit().await?;
                    s.map(|(_, state, _)| state)
                };
                match state {
                    None => Err(AbTestWriteError::NotFound(format!("mailing {mailing_id}"))),
                    Some(s) => Err(AbTestWriteError::Conflict(format!(
                        "mailing {mailing_id} is {s} — only draft mailings bind as variants"
                    ))),
                }
            }
        }
    }

    /// One test's view (variants included).
    pub async fn view(&self, ab_test_id: Uuid) -> Result<AbTestView, AbTestWriteError> {
        let mut tx = self.pool.begin().await?;
        let row = MailingSendRepository::find_ab_test(&mut tx, ab_test_id).await?;
        let variants = MailingSendRepository::variant_mailing_ids(&mut tx, ab_test_id).await?;
        tx.commit().await?;
        let Some((
            id,
            campaign_id,
            winner_selection,
            promote_at,
            completed,
            winner_mailing_id,
            sampling_seed,
        )) = row
        else {
            return Err(AbTestWriteError::NotFound(format!("ab test {ab_test_id}")));
        };
        Ok(AbTestView {
            id,
            campaign_id,
            winner_selection,
            promote_at,
            completed,
            winner_mailing_id,
            sampling_seed,
            variants,
        })
    }

    /// MANUAL winner selection: complete the test with the chosen variant
    /// and queue the winner copy (full-audience resend at pc=100, sampling
    /// disabled). Idempotent through the guarded completion stamp.
    pub async fn select_winner(
        &self,
        ab_test_id: Uuid,
        winner_mailing_id: Uuid,
    ) -> Result<Option<Uuid>, AbTestWriteError> {
        let mut tx = self.pool.begin().await?;
        let test = MailingSendRepository::find_ab_test(&mut tx, ab_test_id).await?;
        let Some((_, _, selection, _, completed, _, _)) = test else {
            tx.rollback().await?;
            return Err(AbTestWriteError::NotFound(format!("ab test {ab_test_id}")));
        };
        if completed {
            tx.rollback().await?;
            return Err(AbTestWriteError::Conflict(format!(
                "ab test {ab_test_id} is already completed"
            )));
        }
        let variants = MailingSendRepository::variant_mailing_ids(&mut tx, ab_test_id).await?;
        if !variants.iter().any(|(id, _)| *id == winner_mailing_id) {
            tx.rollback().await?;
            return Err(AbTestWriteError::Invalid(format!(
                "mailing {winner_mailing_id} is not a variant of test {ab_test_id}"
            )));
        }
        let promoted_id = Uuid::new_v4();
        MailingSendRepository::promote_winner(&mut tx, promoted_id, winner_mailing_id, ab_test_id)
            .await?;
        MailingSendRepository::queue_promoted_winner(&mut tx, promoted_id).await?;
        let stamped =
            MailingSendRepository::complete_ab_test(&mut tx, ab_test_id, winner_mailing_id).await?;
        tx.commit().await?;
        if stamped {
            tracing::info!(
                ab_test_id = %ab_test_id,
                winner_mailing_id = %winner_mailing_id,
                promoted = %promoted_id,
                "manual A/B winner selected ({selection})"
            );
        }
        Ok(if stamped { Some(promoted_id) } else { None })
    }
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
