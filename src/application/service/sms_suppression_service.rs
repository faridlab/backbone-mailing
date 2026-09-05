//! `SmsSuppressionService` — the SMS channel's claim-time phone-blacklist
//! suppression arm (hand-authored, user-owned; see `metaphor.codegen.yaml`).
//!
//! The port of the composer's pre-send blacklist class (upstream
//! `_filter_out_and_handle_revoked_sms_values`): a recipient whose canonical
//! E.164 number is on the ACTIVE global phone blacklist is dropped from the
//! send set AND marked with a PRE-CANCELED VISIBLE trace —
//! `trace_status='cancel'`, `failure_type='sms_blacklist'` — never a silent
//! skip. Canceled traces sit OUTSIDE the `(mailing, recipient)` mint fence,
//! so a later re-target after a blacklist `remove` mints a fresh live trace.
//!
//! Contract with the send engine (the SMS drive path):
//!
//! - called AT CLAIM TIME — after the SKIP LOCKED claim flipped the mailing
//!   to `sending` and the recipient resolver produced the phone-bearing
//!   send set, BEFORE any outgoing mint/enqueue (the email path's
//!   `blacklisted_emails` arm is the exact analog, same step of the sweep);
//! - the membership check is ONE batched parameterized query through
//!   backbone-mail's public verb ([`PhoneBlacklistWriteService::listed_among`]
//!   — the send-time suppression check that verb exists for); the unbounded
//!   load-all-the-blacklist read upstream does (`search([])`, SM-B9) is the
//!   defect this shape refuses;
//! - the arm touches NOTHING else: the send set passes through untouched,
//!   no state moves on the mailing, and the caller fires its
//!   `RecipientSuppressedAtSend` events from the returned suppression list.
//!
//! Inputs are formatter-minted [`E164Number`]s by construction (the type has
//! no other constructor), so the canonical-equality check is exact — raw
//! variants of the same number cannot dodge suppression by shape.

use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use backbone_mail::application::service::phone_blacklist_write_service::{
    PhoneBlacklistError, PhoneBlacklistWriteService,
};
use backbone_mail::application::service::phone_validation_service::E164Number;

use crate::infrastructure::persistence::trace_repository::TraceRepository;

/// The suppression trace's failure cause — the trace model's
/// `sms_blacklist` value (schema/models/trace.model.yaml).
pub const SMS_BLACKLIST_FAILURE_TYPE: &str = "sms_blacklist";

/// One phone-bearing send target, as the SMS recipient resolver produces it.
/// `email` is the trace's join key (the recipient_email column is NOT NULL —
/// a contact carries both); `number` is the formatter-minted canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneRecipient {
    pub recipient_id: Uuid,
    pub email: String,
    pub number: E164Number,
}

/// The mailing context the claimed row carries into the arm.
#[derive(Debug, Clone)]
pub struct ClaimedSmsContext {
    pub mailing_id: Uuid,
    pub campaign_id: Option<Uuid>,
    /// Mirrors the mailing's target_model at mint time — the trace's
    /// recipient anchor. Any phone-bearing target the sms channel can
    /// resolve ('mailing_contact', 'party', or an external bridge target
    /// such as 'crm_lead' composed through the targeting port).
    pub recipient_model: String,
}

/// What one suppression pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SuppressionOutcome {
    /// Recipients handed to the arm.
    pub checked: usize,
    /// Recipients suppressed — each carries its pre-canceled trace id.
    pub suppressed: Vec<Suppressed>,
    /// The send set, untouched and in input order.
    pub send_set: Vec<PhoneRecipient>,
}

/// One visible suppression: who, why-not-sent, and the canceled trace that
/// proves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppressed {
    pub recipient_id: Uuid,
    pub email: String,
    pub number: E164Number,
    pub trace_id: Uuid,
}

/// Typed error surface (house style).
#[derive(Debug, thiserror::Error)]
pub enum SmsSuppressionError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("blacklist seam: {0}")]
    Blacklist(#[from] PhoneBlacklistError),
}

impl SmsSuppressionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) | Self::Blacklist(_) => "mailing_db_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        500
    }
}

/// The claim-time phone-blacklist suppression arm.
pub struct SmsSuppressionService {
    pool: PgPool,
    blacklist: Arc<PhoneBlacklistWriteService>,
}

impl SmsSuppressionService {
    pub fn new(pool: PgPool, blacklist: Arc<PhoneBlacklistWriteService>) -> Self {
        Self { pool, blacklist }
    }

    /// Split the claimed mailing's phone recipients into (send set, visible
    /// suppressions). The membership probe is ONE batched query; the cancel
    /// traces mint in ONE transaction, mirroring the engine's suppression
    /// mint block.
    pub async fn suppress_at_claim(
        &self,
        ctx: &ClaimedSmsContext,
        recipients: Vec<PhoneRecipient>,
    ) -> Result<SuppressionOutcome, SmsSuppressionError> {
        let numbers: Vec<E164Number> = recipients.iter().map(|r| r.number.clone()).collect();
        // Empty input short-circuits inside the verb without touching the
        // database — an empty audience is not a suppression event.
        let blacklisted = self.blacklist.listed_among(&numbers).await?;

        let mut outcome = SuppressionOutcome { checked: recipients.len(), ..Default::default() };
        let mut to_cancel: Vec<&PhoneRecipient> = Vec::new();
        for recipient in &recipients {
            if blacklisted.contains(&recipient.number) {
                to_cancel.push(recipient);
            } else {
                outcome.send_set.push(recipient.clone());
            }
        }

        if !to_cancel.is_empty() {
            let mut tx = self.pool.begin().await?;
            for recipient in &to_cancel {
                let trace_id = Uuid::new_v4();
                // Status 'cancel' at mint — the mail engine's suppression
                // block shape: the trace is BORN canceled (there is no
                // outgoing row to retire), outside the mint fence by the
                // partial unique's carve-out. The CHANNEL-PARAMETERIZED mint:
                // a suppression trace stamped trace_type='mail' would be
                // invisible to the delivery pump's sms-scoped verdict join
                // and miscounted by the completion counter — the channel
                // rides the mint, and the canonical number rides with it
                // (the phone-keyed lookup arm of the trace).
                TraceRepository::mint_trace_channel(
                    &mut tx,
                    trace_id,
                    ctx.mailing_id,
                    ctx.campaign_id,
                    &ctx.recipient_model,
                    recipient.recipient_id,
                    &recipient.email,
                    Some(&recipient.number.to_string()),
                    "sms",
                    "cancel",
                    Some(SMS_BLACKLIST_FAILURE_TYPE),
                    false,
                )
                .await?;
                outcome.suppressed.push(Suppressed {
                    recipient_id: recipient.recipient_id,
                    email: recipient.email.clone(),
                    number: recipient.number.clone(),
                    trace_id,
                });
            }
            tx.commit().await?;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    // The claim-time proof (visible sms_blacklist cancel traces under a real
    // SKIP LOCKED claim, send set untouched) lives in tests/sms_stop_cases.rs
    // against a fresh scratch database — this arm's whole surface is the
    // database interaction.
}
