//! `SmsStopService` — the SMS channel's STOP legs (hand-authored, user-owned;
//! see `metaphor.codegen.yaml`). The mass_mailing_sms STOP port: what a STOP
//! IS here, what it may never be, and the one effect it lands.
//!
//! The ported correction this file owns — **MSM-B-10, the STOP-code entropy
//! floor** (schema flag row; ADR-0018 Tier B): upstream stamps every SMS trace
//! with a random THREE-character code (`_get_random_code`, `CODE_SIZE=3`,
//! trigger TR-MSM-4) and the STOP entry surface happily renders its
//! number-entry form for ANY code — a scraper iterating codes inside a known
//! mailing id hits live unsubscribe forms. That whole shape is banned here:
//!
//! - a presented STOP code SHORTER than [`STOP_CODE_MIN_LEN`] is refused with
//!   a TYPED error that carries the raw input — the refusal is auditable,
//!   never a boolean shrug;
//! - the STOP LINK never carries a Tier B code at all (ADR-0018 rule 4: no
//!   Tier B where a machine can carry Tier A) — the link-token surface is the
//!   overlay's route family (the deferred `/r/:code` seam's sibling), and the
//!   human leg's proof is NUMBER-MATCH of the entered number against the
//!   trace's stored canonical E.164 plus entry-surface throttling — the code
//!   alone opens nothing.
//!
//! The two inbound legs, both ending in exactly ONE effect:
//!
//! 1. **`handle_reply`** — the reply leg. A gateway hands (text, sender raw
//!    number, optional country hint). A recognized STOP keyword sends the
//!    sender through backbone-mail's phone formatter (the ONE E.164 home —
//!    typed refusals carrying raw input, never a silent failure) and lands
//!    ONE [`PhoneBlacklistWriteService::add`] with the formatter-minted
//!    [`E164Number`]. Raw variants converge on one canonical row; `remove`
//!    archives; a re-add reactivates — the verb's semantics, untouched. This
//!    service NEVER writes `messaging.phone_blacklists` directly and carries
//!    no reason plumbing (the verbs carry none by design; STOP-context
//!    logging stays mailing-side — the email table's `opt_out_reason_id`
//!    no-writer gap is a registered follow-up, not this surface's to fix).
//!
//! 2. **`stop_by_code`** — the code-bearing leg (upstream's
//!    `/sms/<mailing_id>/<trace_code>` shape). The code is validated FIRST
//!    against the Tier B floor; only a well-formed code proceeds to the same
//!    formatter + add effect. The code is an ENTRY key, not the capability.
//!
//! The Odoo contact-list split (mailings with lists take the per-list
//! subscription opt-out; list-less mailings take the global blacklist) rides
//! the entry ROUTE family — the deferred route seam owns the mailing context;
//! both of its legs end in verbs this module already ships
//! ([`crate::application::service::subscription_write_service::SubscriptionWriteService`]
//! for the per-list flip, this service for the global add).

use std::sync::Arc;

use backbone_mail::application::service::phone_blacklist_write_service::{
    AddOutcome, PhoneBlacklistError, PhoneBlacklistWriteService,
};
use backbone_mail::application::service::phone_validation_service::{
    phone_format, E164Number, PhoneFormatError,
};

// ─── the STOP-code policy (MSM-B-10, ADR-0018 Tier B) ────────────────────────

/// The Tier B length floor for STOP codes at this seam. The upstream
/// `CODE_SIZE=3` random code — and the whole 3–5 char short-code band the
/// C8 ladder flags — sits BELOW this floor; anything shorter is the banned
/// shape and refuses loudly. (The other in-tree Tier B floor is the kiosk
/// PIN's 4-digit minimum; codes here are alphanumeric, so the floor is
/// proportionally higher.)
pub const STOP_CODE_MIN_LEN: usize = 6;

/// The Tier B ceiling — short is a REQUIREMENT for human-typed codes, not a
/// defect; there is no reason a STOP code should ever be long.
pub const STOP_CODE_MAX_LEN: usize = 12;

/// Why a presented STOP code was refused. Every variant carries the raw
/// input — refusals are auditable, never a bare boolean.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StopCodeError {
    #[error("STOP code is empty")]
    Empty { raw: String },
    #[error("STOP code {raw:?} is {len} chars — below the Tier B floor of {min} (the upstream 3-char code shape is banned)")]
    TooShort { raw: String, len: usize, min: usize },
    #[error("STOP code {raw:?} exceeds the Tier B ceiling of {max}")]
    TooLong { raw: String, max: usize },
    #[error("STOP code {raw:?} is not alphanumeric ASCII: {detail}")]
    NotACode { raw: String, detail: String },
}

/// Validate a presented STOP code against the Tier B shape: 6–12
/// alphanumeric ASCII characters. Pure — the refusal corpus and the entry
/// surface share it. This is the guard that makes the upstream scraper shape
/// (iterate short codes, harvest live forms) impossible: a 3-character code
/// never reaches the number-match, never renders a form, never effects an
/// opt-out.
pub fn validate_stop_code(raw: &str) -> Result<(), StopCodeError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(StopCodeError::Empty { raw: raw.into() });
    }
    let len = trimmed.len();
    if len < STOP_CODE_MIN_LEN {
        return Err(StopCodeError::TooShort { raw: raw.into(), len, min: STOP_CODE_MIN_LEN });
    }
    if len > STOP_CODE_MAX_LEN {
        return Err(StopCodeError::TooLong { raw: raw.into(), max: STOP_CODE_MAX_LEN });
    }
    if !trimmed.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(StopCodeError::NotACode {
            raw: raw.into(),
            detail: "codes are [A-Za-z0-9] only".into(),
        });
    }
    Ok(())
}

/// The STOP keyword recognizer for the reply leg. Recognizes the plain
/// opt-out intents a recipient actually types — `STOP`, `STOP SMS`,
/// `UNSUBSCRIBE` — case-insensitively, tolerant of stray punctuation and
/// whitespace. Anything else is NOT a STOP (a `YES` or a question must never
/// blacklist anyone) and refuses with [`SmsStopError::NotAStopRequest`].
pub fn is_stop_reply_keyword(text: &str) -> bool {
    let folded: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let squeezed = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(squeezed.as_str(), "STOP" | "STOP SMS" | "UNSUBSCRIBE")
}

// ─── the service ─────────────────────────────────────────────────────────────

/// Typed error surface (house style).
#[derive(Debug, thiserror::Error)]
pub enum SmsStopError {
    /// The entered number would not canonicalize — the formatter's typed
    /// refusal, raw input carried. Never a silent failure, never a guess.
    #[error(transparent)]
    Format(#[from] PhoneFormatError),
    /// The reply text is not a STOP intent.
    #[error("not a STOP request: {text:?}")]
    NotAStopRequest { text: String },
    /// The presented STOP code is the banned short shape (MSM-B-10).
    #[error(transparent)]
    Code(#[from] StopCodeError),
    #[error("db: {0}")]
    Blacklist(#[from] PhoneBlacklistError),
}

impl SmsStopError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Format(_) => "phone_format_refused",
            Self::NotAStopRequest { .. } => "not_a_stop_request",
            Self::Code(_) => "stop_code_refused",
            Self::Blacklist(_) => "mailing_db_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::Format(_) | Self::NotAStopRequest { .. } | Self::Code(_) => 422,
            Self::Blacklist(_) => 500,
        }
    }
}

/// What one STOP landed: the canonical number and which idempotent state the
/// verb's upsert took (newly listed / reactivated / already listed — the
/// surviving row is the same in all three).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReplyOutcome {
    pub number: E164Number,
    pub add: AddOutcome,
}

/// The STOP verbs. Stateless; every effect is ONE call into backbone-mail's
/// public blacklist verb.
pub struct SmsStopService {
    blacklist: Arc<PhoneBlacklistWriteService>,
}

impl SmsStopService {
    pub fn new(blacklist: Arc<PhoneBlacklistWriteService>) -> Self {
        Self { blacklist }
    }

    /// The reply leg: (inbound text, sender raw number, optional country
    /// hint) → canonical E.164 → ONE global-blacklist add.
    ///
    /// The sender number arrives in whatever shape the gateway hands over
    /// (international `+NN…`, national with separators, …); national input
    /// REQUIRES the country hint and refuses loudly without one (the
    /// formatter's typed [`PhoneFormatError`], never a silent skip).
    pub async fn handle_reply(
        &self,
        text: &str,
        raw_number: &str,
        country_hint: Option<&str>,
    ) -> Result<StopReplyOutcome, SmsStopError> {
        if !is_stop_reply_keyword(text) {
            return Err(SmsStopError::NotAStopRequest { text: text.into() });
        }
        self.effect_stop(raw_number, country_hint).await
    }

    /// The code-bearing leg: the presented STOP code is validated FIRST
    /// (the Tier B floor — a three-character code is the banned upstream
    /// shape and refuses with the raw input carried), then the entered
    /// number canonicalizes and the same single add lands. The code gates
    /// entry; the NUMBER is the identity.
    pub async fn stop_by_code(
        &self,
        stop_code: &str,
        entered_raw: &str,
        country_hint: Option<&str>,
    ) -> Result<StopReplyOutcome, SmsStopError> {
        validate_stop_code(stop_code)?;
        self.effect_stop(entered_raw, country_hint).await
    }

    /// The one effect both legs share: formatter → add. No direct table
    /// writes, no reason column, no second call.
    async fn effect_stop(
        &self,
        raw_number: &str,
        country_hint: Option<&str>,
    ) -> Result<StopReplyOutcome, SmsStopError> {
        let number = phone_format(raw_number, country_hint)?;
        let add = self.blacklist.add(&number).await?;
        Ok(StopReplyOutcome { number, add })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── the 3-char refusal corpus (MSM-B-10) ──────────────────────────────

    /// The banned upstream shape, exhaustively at its length: every
    /// three-character alphanumeric code refuses, carrying its raw input.
    #[test]
    fn three_character_codes_refuse_with_raw_carried() {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let mut corpus = Vec::new();
        for &a in ALPHABET {
            for &b in ALPHABET {
                for &c in ALPHABET {
                    corpus.push((a as char).to_string() + &(b as char).to_string() + &(c as char).to_string());
                }
            }
        }
        assert_eq!(corpus.len(), 36 * 36 * 36);
        for code in &corpus {
            match validate_stop_code(code) {
                Err(StopCodeError::TooShort { raw, len, min }) => {
                    assert_eq!(raw, *code, "the refusal carries the raw input");
                    assert_eq!(len, 3);
                    assert_eq!(min, STOP_CODE_MIN_LEN);
                }
                other => panic!("{code} should refuse as TooShort, got {other:?}"),
            }
        }
    }

    /// The full below-the-floor corpus: empty, 1–5 chars, the 3-char
    /// upstream shapes mixed-case, whitespace-padded shorts, and
    /// non-alabetic shapes — every one a TYPED refusal with the raw input.
    #[test]
    #[expect(clippy::expect_used, reason = "unit test asserts the stop-code refusal contract")]
    fn below_floor_corpus_refuses_typed() {
        let corpus = [
            "",
            " ",
            "s",
            "st",
            "sto",
            "STOP",
            "abcde",
            "Ab3",
            "xyz",
            "  abc  ",
            "a b c",
            "!!!",
            "a-bc",
        ];
        for raw in corpus {
            let err = validate_stop_code(raw).expect_err(&format!("{raw:?} must refuse"));
            // Every variant carries the raw input back out.
            let carried = match &err {
                StopCodeError::Empty { raw } => raw,
                StopCodeError::TooShort { raw, .. } => raw,
                StopCodeError::TooLong { raw, .. } => raw,
                StopCodeError::NotACode { raw, .. } => raw,
            };
            assert_eq!(carried, raw, "the refusal carries the raw input verbatim");
        }
    }

    /// Well-formed codes pass: the floor inclusive, the ceiling inclusive,
    /// mixed case and digits.
    #[test]
    fn wellformed_codes_pass() {
        for code in ["abc123", "ABCDEF", "Stop19", "a1b2c3d4", "12characters"] {
            assert!(validate_stop_code(code).is_ok(), "{code} should pass");
        }
        // One past the ceiling refuses.
        assert!(matches!(
            validate_stop_code("13charactersx"),
            Err(StopCodeError::TooLong { .. })
        ));
    }

    // ── the reply keyword recognizer ──────────────────────────────────────

    #[test]
    fn stop_keywords_recognize_tolerantly() {
        for text in [
            "STOP",
            "stop",
            "  Stop  ",
            "STOP  SMS",
            "stop sms",
            "unsubSCRIBE",
            "\"STOP!\"",
            "stop.",
        ] {
            assert!(is_stop_reply_keyword(text), "{text:?} is a STOP intent");
        }
        for text in ["", "yes", "STOPPING PLEASE", "how do I stop this?", "UNSUB"] {
            assert!(!is_stop_reply_keyword(text), "{text:?} is NOT a STOP intent");
        }
    }
}
