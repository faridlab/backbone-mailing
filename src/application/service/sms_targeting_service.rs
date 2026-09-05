//! The SMS channel's targeting port (hand-authored, user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The email channel resolves its external bridge targets through
//! [`crate::application::service::mailing_write_service::TargetRecipientResolver`]
//! — one host-composed adapter per `target_model`, declaratively keyed, no
//! cargo edge onto the target module. This file is the PHONE-channel twin of
//! that seam: the same declarative composition, over canonical E.164 numbers.
//!
//! Why a separate trait and not a flag on the email resolver: the two channels
//! answer different questions. The email arm asks "who has an address to mail"
//! and dedups on the email; the phone arm asks "who has a sanitizable number
//! to text" and — critically — must report the leads it could NOT sanitize.
//! The upstream bridge this ports (website_crm_sms) gated SMS eligibility on
//! STRICT RAW-STRING equality between two independently written phone columns
//! (`lead.phone == visitor.mobile`), so any normalization drift between the
//! two writes silently disqualified every lead. The fix this port lands:
//! eligibility is the SANITIZER's verdict, computed once, canonically, at
//! targeting time — and a lead that fails it is COUNTED in the report, never
//! silently dropped. There is no raw-string comparison anywhere on this path.
//!
//! Composition status (recorded deliberately): the send walk that CONSUMES
//! this port — claim → resolve phones → claim-time suppression →
//! channel-correct trace mint → gateway enqueue through backbone-mail's
//! `SmsWriteService` → walk-complete marker — is tracked as DIT #231
//! ("Compose the SMS send walk in backbone-mailing"); until it lands, an
//! sms-type mailing still parks loudly at the mail walk's channel guard and
//! NOTHING mints from this port on the sweep path. What ships here is the
//! targeting contract itself: the port the walk will call, the report shape
//! its outcome must surface, and the channel-correct mint verbs
//! (`TraceRepository::mint_trace_channel*`) its traces mint through — so the
//! walk composes onto a proven surface instead of growing one.

use crate::application::service::sms_suppression_service::PhoneRecipient;
use crate::infrastructure::persistence::mailing_send_repository::CompiledDomain;

/// One targeting pass's answer: the send set plus the VISIBLE exclusion
/// counts. `excluded_invalid_phone` is the anti-silent-disqualification
/// counter — the send walk logs it, counts it in its sweep outcome, and a
/// future provider leg can surface it per mailing. A targeting pass that
/// returns only the send set would re-create the upstream defect (every
/// drift silently narrowing the audience).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmsTargetingReport {
    /// Recipients with a canonical, sanitizable E.164 number (the send set).
    pub eligible: Vec<PhoneRecipient>,
    /// Live target rows whose phone candidates ALL failed sanitization —
    /// counted, never silently dropped.
    pub excluded_invalid_phone: usize,
}

impl SmsTargetingReport {
    /// Total target rows the pass examined (eligible + excluded).
    pub fn examined(&self) -> usize {
        self.eligible.len() + self.excluded_invalid_phone
    }
}

/// One EXTERNAL target's phone resolver — the phone-channel twin of
/// [`crate::application::service::mailing_write_service::TargetRecipientResolver`].
///
/// The host composes ONE resolver per phone-bearing bridge target (e.g.
/// `crm_lead`), keyed by the `target_model` string it serves. The
/// implementation MUST:
///
/// - resolve under each company's fence (one transaction per company with
///   `bind_company_on`) — the concatenated send set may span companies but
///   every row must have entered through exactly one company's policy;
/// - sanitize through backbone-mail's public `sanitize_candidates` walk and
///   emit the FORMATTER-MINTED `E164Number` (never a raw string) — the
///   canonical-equality guarantees downstream (suppression, STOP matching)
///   hold only if every number on this path is canonical;
/// - count non-sanitizable rows into `excluded_invalid_phone` instead of
///   dropping them;
/// - dedup across companies by CANONICAL number (the send grain), mirroring
///   the email resolvers' cross-company dedup.
#[async_trait::async_trait]
pub trait TargetPhoneResolver: Send + Sync {
    /// The `target_model` value this resolver serves (e.g. `'crm_lead'`).
    fn target_model(&self) -> &'static str;

    /// Resolve the compiled domain into the phone-channel targeting report.
    /// A failure string parks the mailing at send — the same loud-failure
    /// contract the email resolvers carry.
    async fn resolve_phones(&self, domain: &CompiledDomain) -> Result<SmsTargetingReport, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_examined_sums_both_arms() {
        let report = SmsTargetingReport {
            eligible: Vec::new(),
            excluded_invalid_phone: 3,
        };
        assert_eq!(report.examined(), 3);
    }
}
