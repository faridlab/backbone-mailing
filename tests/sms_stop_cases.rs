//! The SMS STOP surface's database cases: the reply round-trip (raw variants
//! converge on ONE canonical blacklist row through the verb; archive
//! semantics preserved), the banned short-code refusal's zero-effect proof,
//! and the claim-time phone-blacklist suppression arm (pre-canceled VISIBLE
//! traces under a real SKIP LOCKED claim — never silent skips).
//!
//! The pure Tier B refusal corpus lives with the policy (unit tests in
//! `sms_stop_service.rs`); these are the fresh-DB legs.

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use backbone_mail::application::service::phone_blacklist_write_service::{
    AddOutcome, PhoneBlacklistWriteService, RemoveOutcome,
};
use backbone_mail::application::service::phone_validation_service::phone_format;
use backbone_mailing::application::service::mailing_write_service::{
    MailingUpsertCommand, MailingWriteService,
};
use backbone_mailing::application::service::sms_stop_service::{SmsStopError, SmsStopService, StopCodeError};
use backbone_mailing::application::service::sms_suppression_service::{
    ClaimedSmsContext, PhoneRecipient, SmsSuppressionService,
};
use backbone_mailing::infrastructure::persistence::mailing_send_repository::MailingSendRepository;
use backbone_mailing::infrastructure::persistence::trace_repository::TraceRepository;
use uuid::Uuid;

/// A canonical Indonesian mobile in several raw skins. All must converge on
/// the same E.164 row.
const RAW_INTERNATIONAL: &str = "+62 812-3456-7890";
const RAW_NATIONAL: &str = "0812 3456 7890";
const CANONICAL: &str = "+6281234567890";

fn stop_service(pool: &sqlx::PgPool) -> SmsStopService {
    SmsStopService::new(std::sync::Arc::new(PhoneBlacklistWriteService::new(pool.clone())))
}

// ── the STOP round-trip ──────────────────────────────────────────────────────

/// The reply leg, end to end: keyword in → formatter → ONE verb add → one
/// canonical row. Raw variants of the same number converge (international
/// with separators, national with hint); remove archives the row; a later
/// STOP reactivates the SAME row — the verb's semantics, untouched.
#[tokio::test]
async fn stop_reply_round_trips_converges_and_preserves_archive_semantics() {
    let Some(db) = TestDb::new("stop").await else {
        return skipped("stop");
    };
    let svc = stop_service(&db.pool);

    // First STOP from the international skin: newly listed.
    let first = svc
        .handle_reply("STOP", RAW_INTERNATIONAL, None)
        .await
        .expect("first STOP lands");
    assert_eq!(first.number.to_string(), CANONICAL);
    assert!(matches!(first.add, AddOutcome::NewlyListed { .. }));

    // Second STOP from the NATIONAL skin (country hint in play): converges
    // onto the SAME canonical row — already listed, same row id.
    let second = svc
        .handle_reply("  stop sms  ", RAW_NATIONAL, Some("id"))
        .await
        .expect("national variant lands");
    assert_eq!(second.number.to_string(), CANONICAL);
    assert_eq!(second.add.row_id(), first.add.row_id());
    assert!(matches!(second.add, AddOutcome::AlreadyListed { .. }));

    // Exactly ONE row — the verb's canonical unique held; no direct inserts
    // minted near-duplicates.
    let (rows, canonical_rows): (i64, i64) = sqlx::query_as(
        r#"SELECT count(*), count(*) FILTER (WHERE number = $1 AND active)
           FROM messaging.phone_blacklists"#,
    )
    .bind(CANONICAL)
    .fetch_one(&db.pool)
    .await
    .expect("row probe");
    assert_eq!(rows, 1, "raw variants converged on one row");
    assert_eq!(canonical_rows, 1, "the row is canonical and active");

    // Remove: the ARCHIVE pattern — row retained, active=false.
    let removed = svc_blacklist(&db.pool)
        .await
        .remove(&first.number)
        .await
        .expect("remove");
    assert!(matches!(removed, RemoveOutcome::Removed { .. }));
    let (rows, active): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE active) FROM messaging.phone_blacklists",
    )
    .fetch_one(&db.pool)
    .await
    .expect("archive probe");
    assert_eq!(rows, 1, "the row survives the remove — archived, not deleted");
    assert_eq!(active, 0);

    // A third STOP REACTIVATES the archived row — same row id, still one row.
    let third = svc
        .handle_reply("UNSUBSCRIBE", RAW_INTERNATIONAL, None)
        .await
        .expect("re-add lands");
    assert_eq!(third.add.row_id(), first.add.row_id());
    assert!(matches!(third.add, AddOutcome::Reactivated { .. }));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.phone_blacklists")
        .fetch_one(&db.pool)
        .await
        .expect("final count");
    assert_eq!(rows, 1, "re-add reactivated — still exactly one canonical row");

    db.dispose().await;
}

/// Refuse-loudly: an unparseable sender number is a TYPED refusal carrying
/// the raw input — never a silent skip, never a guessed canonical row — and
/// a non-STOP text effects nothing at all.
#[tokio::test]
async fn stop_reply_refuses_unparseable_numbers_and_non_stop_text_loudly() {
    let Some(db) = TestDb::new("stopf").await else {
        return skipped("stopf");
    };
    let svc = stop_service(&db.pool);

    // Not a number at all (hinted, so the refusal is about the input shape,
    // not the missing hint).
    match svc.handle_reply("STOP", "not-a-number", Some("ID")).await {
        Err(SmsStopError::Format(e)) => {
            let raw = match &e {
                backbone_mail::application::service::phone_validation_service::PhoneFormatError::NotANumber { raw, .. } => raw.clone(),
                other => panic!("expected NotANumber carrying raw, got {other:?}"),
            };
            assert_eq!(raw, "not-a-number", "the typed refusal carries the raw input");
        }
        other => panic!("expected a typed Format refusal, got {other:?}"),
    }

    // National input with no country hint: the typed MissingCountryHint.
    assert!(matches!(
        svc.handle_reply("STOP", "0812 3456 7890", None).await,
        Err(SmsStopError::Format(
            backbone_mail::application::service::phone_validation_service::PhoneFormatError::MissingCountryHint { .. }
        ))
    ));

    // A non-STOP reply text never blacklists anyone.
    assert!(matches!(
        svc.handle_reply("YES please", "+6281234567890", None).await,
        Err(SmsStopError::NotAStopRequest { .. })
    ));

    // Every refusal above wrote ZERO rows.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.phone_blacklists")
        .fetch_one(&db.pool)
        .await
        .expect("count");
    assert_eq!(rows, 0);

    db.dispose().await;
}

/// The code-bearing leg: the banned three-character code refuses BEFORE any
/// effect — even with a perfectly valid number — while a well-formed code
/// proceeds to the same single add.
#[tokio::test]
async fn stop_by_code_refuses_short_codes_before_any_effect() {
    let Some(db) = TestDb::new("stopc").await else {
        return skipped("stopc");
    };
    let svc = stop_service(&db.pool);

    // The banned upstream shape: 3 chars → typed refusal carrying the raw.
    match svc.stop_by_code("aB3", "+6281234567890", None).await {
        Err(SmsStopError::Code(StopCodeError::TooShort { raw, len, min })) => {
            assert_eq!(raw, "aB3");
            assert_eq!(len, 3);
            assert_eq!(min, backbone_mailing::application::service::sms_stop_service::STOP_CODE_MIN_LEN);
        }
        other => panic!("expected TooShort, got {other:?}"),
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.phone_blacklists")
        .fetch_one(&db.pool)
        .await
        .expect("count after refusal");
    assert_eq!(rows, 0, "the banned code shape effects NOTHING");

    // A well-formed code proceeds through the same one-add effect.
    let landed = svc
        .stop_by_code("abc123", RAW_NATIONAL, Some("ID"))
        .await
        .expect("well-formed code lands");
    assert_eq!(landed.number.to_string(), CANONICAL);
    assert!(matches!(landed.add, AddOutcome::NewlyListed { .. }));

    db.dispose().await;
}

// ── the claim-time suppression proof ─────────────────────────────────────────

/// The arm under a REAL claim: the SKIP LOCKED pickup flips the mailing to
/// `sending`, the arm splits the phone-bearing send set against the ACTIVE
/// blacklist, and every hit lands a PRE-CANCELED VISIBLE trace
/// (trace_status='cancel', failure_type='sms_blacklist') — the send set is
/// untouched, the suppression is visible, and the mint fence's carve-out
/// lets a later re-target mint a fresh live trace after a blacklist remove.
#[tokio::test]
async fn claim_time_suppression_mints_visible_cancel_traces_for_blacklisted_numbers() {
    let Some(db) = TestDb::new("smssup").await else {
        return skipped("smssup");
    };

    // An audience of eight contacts; four of their numbers go onto the
    // phone blacklist through the REAL verb before the sweep.
    const TOTAL: usize = 8;
    const BLACKLISTED: usize = 4;
    let (audience_id, _emails) = seed_audience(&db.pool, "smssup", TOTAL).await;

    // Distinct valid numbers, one per contact (ID mobiles +62 81x…).
    let numbers: Vec<String> = (0..TOTAL)
        .map(|i| format!("+62812{:08}", 31000000u64 + i as u64 * 111111u64))
        .collect();

    let engine = MailingWriteService::new(db.pool.clone());
    let mailing_id = engine
        .create_mailing(&MailingUpsertCommand {
            subject: "SMS suppression probe".into(),
            preview: None,
            body_html: "<p>probe</p>".into(),
            email_from: "probe@example.id".into(),
            reply_to: None,
            mailing_domain_raw: serde_json::json!([
                ["mailing_audience_id", "=", audience_id.to_string()]
            ]),
            target_model: "mailing_contact".into(),
            schedule_type: "immediate".into(),
            schedule_date: None,
            use_exclusion_list: true,
            campaign_id: None,
            source_id: None,
            ab_test_id: None,
            ab_testing_enabled: false,
            ab_testing_pc: 0,
        })
        .await
        .expect("create");
    engine.launch(mailing_id, "immediate", None).await.expect("launch");

    // The claim, exactly as the sweep runs it: SKIP LOCKED pickup + the
    // state-guarded flip, inside the claim transaction.
    let ctx = {
        let mut tx = db.pool.begin().await.expect("claim tx");
        let claimed = MailingSendRepository::claim_due_mailings(&mut tx, 16)
            .await
            .expect("claim");
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, mailing_id);
        assert!(MailingSendRepository::flip_sending(&mut tx, mailing_id)
            .await
            .expect("flip"));
        tx.commit().await.expect("claim commit");
        ClaimedSmsContext {
            mailing_id,
            campaign_id: None,
            recipient_model: "mailing_contact".into(),
        }
    };

    // Blacklist the first four numbers via the verb (formatter-minted).
    let blacklist = PhoneBlacklistWriteService::new(db.pool.clone());
    for raw in numbers.iter().take(BLACKLISTED) {
        let number = phone_format(raw, None).expect("fixture number canonicalizes");
        blacklist.add(&number).await.expect("verb add");
    }

    // The resolver's output shape: contact id + email + canonical number,
    // deterministically paired with the fixture numbers (both email-ordered).
    let contacts: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, email FROM mailing.mailing_contacts ORDER BY email",
    )
    .fetch_all(&db.pool)
    .await
    .expect("contact rows");
    assert_eq!(contacts.len(), TOTAL);
    let recipients: Vec<PhoneRecipient> = contacts
        .into_iter()
        .zip(numbers.iter())
        .map(|((recipient_id, email), raw)| PhoneRecipient {
            recipient_id,
            email,
            number: phone_format(raw, None).expect("canonical"),
        })
        .collect();

    // The arm, at claim time.
    let arm = SmsSuppressionService::new(db.pool.clone(), std::sync::Arc::new(blacklist));
    let outcome = arm
        .suppress_at_claim(&ctx, recipients)
        .await
        .expect("suppression pass");

    assert_eq!(outcome.checked, TOTAL);
    assert_eq!(outcome.suppressed.len(), BLACKLISTED);
    assert_eq!(outcome.send_set.len(), TOTAL - BLACKLISTED);

    // Every suppression is VISIBLE: exactly one cancel trace each, with the
    // sms_blacklist cause, carrying the suppressed number's recipient. The
    // trace also carries the CHANNEL it belongs to — a suppression trace
    // stamped trace_type='mail' would be invisible to the delivery pump's
    // sms-scoped verdict join — and the canonical number (the phone-keyed
    // lookup arm).
    for s in &outcome.suppressed {
        let row: (String, Option<String>, String, Option<String>) = sqlx::query_as(
            r#"SELECT trace_status::text, failure_type::text,
                      trace_type::text, recipient_phone
               FROM mailing.mailing_traces WHERE id = $1"#,
        )
        .bind(s.trace_id)
        .fetch_one(&db.pool)
        .await
        .expect("suppression trace");
        assert_eq!(row.0, "cancel", "pre-canceled, visible");
        assert_eq!(row.1.as_deref(), Some("sms_blacklist"));
        assert_eq!(row.2, "sms", "the suppression trace rides the sms channel");
        assert_eq!(
            row.3.as_deref(),
            Some(s.number.to_string().as_str()),
            "the canonical number rides the trace"
        );
    }
    let suppressed_ids: HashSet<Uuid> = outcome.suppressed.iter().map(|s| s.recipient_id).collect();
    assert_eq!(suppressed_ids.len(), BLACKLISTED);

    // The suppressed recipients have ZERO live (non-cancel) traces — they
    // were never going to be sent to — and the send set has zero traces of
    // ANY kind yet (the arm mints nothing for them; the send path owns that).
    let live_for_suppressed: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM mailing.mailing_traces
           WHERE mailing_id = $1 AND trace_status <> 'cancel'
             AND recipient_id = ANY($2)"#,
    )
    .bind(mailing_id)
    .bind(&outcome.suppressed.iter().map(|s| s.recipient_id).collect::<Vec<_>>())
    .fetch_one(&db.pool)
    .await
    .expect("live count");
    assert_eq!(live_for_suppressed, 0, "no outgoing trace for the blacklisted");
    let traces_for_send_set: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM mailing.mailing_traces
           WHERE mailing_id = $1 AND recipient_id = ANY($2)"#,
    )
    .bind(mailing_id)
    .bind(&outcome.send_set.iter().map(|r| r.recipient_id).collect::<Vec<_>>())
    .fetch_one(&db.pool)
    .await
    .expect("send-set count");
    assert_eq!(traces_for_send_set, 0, "the arm minted nothing for the send set");

    // Total trace ledger: exactly the BLACKLISTED cancel traces.
    let all: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1",
    )
    .bind(mailing_id)
    .fetch_one(&db.pool)
    .await
    .expect("total");
    assert_eq!(all as usize, BLACKLISTED);

    // The mint-fence carve-out: a suppressed recipient MAY mint a fresh LIVE
    // trace later (cancel rows sit outside the partial unique) — the
    // re-target-after-remove path.
    let any_suppressed = outcome.suppressed.first().expect("a suppressed row");
    let mut tx = db.pool.begin().await.expect("tx");
    TraceRepository::mint_trace(
        &mut tx,
        Uuid::new_v4(),
        mailing_id,
        None,
        "mailing_contact",
        any_suppressed.recipient_id,
        &any_suppressed.email,
        "outgoing",
        None,
        false,
    )
    .await
    .expect("re-target mint — cancel traces never block it");
    tx.commit().await.expect("commit");

    // And the empty-audience short-circuit: zero in, zero out, no traces.
    let empty = arm.suppress_at_claim(&ctx, Vec::new()).await.expect("empty pass");
    assert_eq!(empty.checked, 0);
    assert!(empty.suppressed.is_empty());
    assert!(empty.send_set.is_empty());

    db.dispose().await;
}

/// A scratch helper: the blacklist verb over the test pool (the remove leg
/// of the round-trip).
async fn svc_blacklist(pool: &sqlx::PgPool) -> PhoneBlacklistWriteService {
    PhoneBlacklistWriteService::new(pool.clone())
}

/// (import trampoline for the HashSet used above)
use std::collections::HashSet;
