//! `SubscriptionWriteService` — the opt-out split verbs (hand-authored,
//! user-owned; see `metaphor.codegen.yaml`).
//!
//! The split, restated: `opt_out` is a FLIP, never a DELETE. Opting out
//! stamps `opt_out_datetime` on the DATABASE clock (optionally a reason);
//! re-subscribing flips back and clears both. Rows live forever — the
//! audience keeps its history, and the send engine's cross-list
//! OPT-OUT-WINS probe (one opt-out anywhere suppresses the contact
//! everywhere) reads the same rows.
//!
//! The archive verb carries the R-M7 guard: an audience cited by any
//! in-flight mailing's canonical domain refuses 409.

use uuid::Uuid;

use crate::infrastructure::persistence::subscription_repository::{
    SubscriptionRepository, SubscriptionRow,
};

/// Typed error surface (house style).
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionWriteError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid: {0}")]
    Invalid(String),
}

impl SubscriptionWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "mailing_db_error",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "audience_in_use",
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

/// The subscription/audience verbs. Stateless over a pool; one transaction
/// per verb.
pub struct SubscriptionWriteService {
    pool: sqlx::PgPool,
}

impl SubscriptionWriteService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Create an audience.
    pub async fn create_audience(&self, name: &str, is_public: bool) -> Result<Uuid, SubscriptionWriteError> {
        if name.trim().is_empty() {
            return Err(SubscriptionWriteError::Invalid("audience name is required".into()));
        }
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        SubscriptionRepository::insert_audience(&mut tx, id, name, is_public).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Subscribe a contact (upserting the contact row by email) to an
    /// audience. Idempotent — converges onto the existing live row AND
    /// clears any standing opt-out (a re-subscribe is the only sanctioned
    /// way out of an opt-out).
    pub async fn subscribe(
        &self,
        email: &str,
        audience_id: Uuid,
        name: Option<&str>,
    ) -> Result<SubscriptionRow, SubscriptionWriteError> {
        let email = email.trim();
        if !email.contains('@') {
            return Err(SubscriptionWriteError::Invalid(format!(
                "email must be an address: {email}"
            )));
        }
        let mut tx = self.pool.begin().await?;
        SubscriptionRepository::ensure_default_reasons(&mut tx).await?;
        let contact_id = SubscriptionRepository::upsert_contact(&mut tx, email, name).await?;
        let row = SubscriptionRepository::subscribe(&mut tx, contact_id, audience_id).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// The opt-out flip: stamps `opt_out_datetime` on the DB clock,
    /// optionally with a reason. Returns the flipped row; a repeated
    /// unsubscribe returns the standing row WITHOUT restamping (the FIRST
    /// opt-out moment is the durable fact) — `already_out` tells them apart.
    pub async fn opt_out(
        &self,
        contact_id: Uuid,
        audience_id: Uuid,
        reason_id: Option<Uuid>,
    ) -> Result<(SubscriptionRow, bool), SubscriptionWriteError> {
        let mut tx = self.pool.begin().await?;
        let flipped = SubscriptionRepository::opt_out(&mut tx, contact_id, audience_id, reason_id)
            .await?;
        tx.commit().await?;
        match flipped {
            Some(row) => Ok((row, true)),
            None => {
                let standing = self.find(contact_id, audience_id).await?;
                match standing {
                    Some(row) => Ok((row, false)),
                    None => Err(SubscriptionWriteError::NotFound(format!(
                        "subscription for contact {contact_id} on audience {audience_id}"
                    ))),
                }
            }
        }
    }

    /// The unsubscribe ROUTE seam: arrives with an email (+ the audience
    /// context from the trace). Falls back to the default reason when none
    /// is picked.
    pub async fn unsubscribe_by_email(
        &self,
        email: &str,
        audience_id: Uuid,
        reason_id: Option<Uuid>,
    ) -> Result<(SubscriptionRow, bool), SubscriptionWriteError> {
        let mut tx = self.pool.begin().await?;
        SubscriptionRepository::ensure_default_reasons(&mut tx).await?;
        let contact_id = SubscriptionRepository::contact_id_by_email(&mut tx, email)
            .await?
            .ok_or_else(|| {
                SubscriptionWriteError::NotFound(format!("contact {email}"))
            })?;
        let reason = match reason_id {
            Some(r) => Some(r),
            None => SubscriptionRepository::default_reason_id(&mut tx).await?,
        };
        let flipped = SubscriptionRepository::opt_out(&mut tx, contact_id, audience_id, reason)
            .await?;
        tx.commit().await?;
        match flipped {
            Some(row) => Ok((row, true)),
            None => {
                let standing = self.find(contact_id, audience_id).await?;
                match standing {
                    Some(row) => Ok((row, false)),
                    None => Err(SubscriptionWriteError::NotFound(format!(
                        "subscription for {email} on audience {audience_id}"
                    ))),
                }
            }
        }
    }

    /// One live subscription row (the read side).
    pub async fn find(
        &self,
        contact_id: Uuid,
        audience_id: Uuid,
    ) -> Result<Option<SubscriptionRow>, SubscriptionWriteError> {
        let mut tx = self.pool.begin().await?;
        let row = SubscriptionRepository::find_live(&mut tx, contact_id, audience_id).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// Admin removal — soft-deletes the membership (distinct from the
    /// opt-out flip: removes the row from the audience entirely; audit
    /// trail stays).
    pub async fn remove(
        &self,
        contact_id: Uuid,
        audience_id: Uuid,
    ) -> Result<(), SubscriptionWriteError> {
        let mut tx = self.pool.begin().await?;
        let removed = SubscriptionRepository::remove(&mut tx, contact_id, audience_id).await?;
        tx.commit().await?;
        if !removed {
            return Err(SubscriptionWriteError::NotFound(format!(
                "subscription for contact {contact_id} on audience {audience_id}"
            )));
        }
        Ok(())
    }

    /// Archive an audience — R-M7 guarded: refuses 409 while any in-flight
    /// mailing's canonical domain cites the audience. Returns the live
    /// member count it archived with.
    pub async fn archive_audience(
        &self,
        audience_id: Uuid,
    ) -> Result<i64, SubscriptionWriteError> {
        let mut tx = self.pool.begin().await?;
        let in_use = SubscriptionRepository::audience_in_use_by_mailings(&mut tx, audience_id)
            .await?;
        if !in_use.is_empty() {
            tx.rollback().await?;
            return Err(SubscriptionWriteError::Conflict(format!(
                "audience {audience_id} is cited by {} in-flight mailing(s): {}",
                in_use.len(),
                in_use
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let members = SubscriptionRepository::active_member_count(&mut tx, audience_id).await?;
        let archived = SubscriptionRepository::soft_delete_audience(&mut tx, audience_id).await?;
        tx.commit().await?;
        if !archived {
            return Err(SubscriptionWriteError::NotFound(format!(
                "audience {audience_id}"
            )));
        }
        Ok(members)
    }
}

#[cfg(test)]
mod tests {
    // Behavior coverage (fresh-DB subscribe/opt-out flips, OPT-OUT-WINS at
    // send, R-M7 refusal) lives in tests/subscription_cases.rs and
    // tests/behavior/ — the pure parse/unit surface here is thin by design.
}
