//! Trace verb behavior: the monotonic rank guards, the click pair, the
//! reconcile arm, and the mint fence (fresh-DB, disposable scratch per
//! test).

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use backbone_mailing::application::service::trace_write_service::{
    TraceTransition, TraceWriteService,
};
use backbone_mailing::infrastructure::persistence::trace_repository::TraceRepository;
use uuid::Uuid;

/// Mint a live outgoing trace directly (repository level — tests bypass the
/// engine on purpose).
async fn mint_outgoing(pool: &sqlx::PgPool, email: &str) -> Uuid {
    let (trace_id, mailing_id, recipient_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut tx = pool.begin().await.expect("tx");
    TraceRepository::mint_trace(
        &mut tx,
        trace_id,
        mailing_id,
        None,
        "mailing_contact",
        recipient_id,
        email,
        "outgoing",
        None,
        false,
    )
    .await
    .expect("mint");
    tx.commit().await.expect("commit");
    trace_id
}

#[tokio::test]
async fn set_verbs_move_monotonically_and_replays_skip() {
    let Some(db) = TestDb::new("rank").await else {
        return skipped("rank");
    };
    let svc = TraceWriteService::new(db.pool.clone());
    let trace_id = mint_outgoing(&db.pool, "ladder@example.id").await;

    assert_eq!(
        svc.set_sent(trace_id).await.expect("sent"),
        TraceTransition::Moved
    );
    // A replay of set_sent after the row is already sent: skip, no restamp.
    assert_eq!(
        svc.set_sent(trace_id).await.expect("replay"),
        TraceTransition::Skipped
    );
    assert_eq!(
        svc.set_opened(trace_id).await.expect("opened"),
        TraceTransition::Moved
    );
    // set_sent after open is a downgrade — the rank guard skips it.
    assert_eq!(
        svc.set_sent(trace_id).await.expect("downgrade"),
        TraceTransition::Skipped
    );
    assert_eq!(
        svc.set_replied(trace_id).await.expect("replied"),
        TraceTransition::Moved
    );
    // Reply is terminal for the ladder: everything below skips.
    assert_eq!(
        svc.set_opened(trace_id).await.expect("below reply"),
        TraceTransition::Skipped
    );
    assert_eq!(
        svc.set_bounced(trace_id).await.expect("bounce after reply"),
        TraceTransition::Skipped
    );
    // Unknown id: Missing, not a silent false.
    assert_eq!(
        svc.set_opened(Uuid::new_v4()).await.expect("unknown"),
        TraceTransition::Missing
    );
    db.dispose().await;
}

#[tokio::test]
async fn bounce_and_failure_carry_their_typed_causes() {
    let Some(db) = TestDb::new("fail").await else {
        return skipped("fail");
    };
    let svc = TraceWriteService::new(db.pool.clone());
    let bounced = mint_outgoing(&db.pool, "bounced@example.id").await;
    assert_eq!(
        svc.set_bounced(bounced).await.expect("bounce"),
        TraceTransition::Moved
    );
    let (status, ftype): (String, String) = sqlx::query_as(
        r#"SELECT trace_status::text, failure_type::text
           FROM mailing.mailing_traces WHERE id = $1"#,
    )
    .bind(bounced)
    .fetch_one(&db.pool)
    .await
    .expect("bounce row");
    assert_eq!((status.as_str(), ftype.as_str()), ("bounce", "mail_bounce"));

    let failed = mint_outgoing(&db.pool, "failed@example.id").await;
    assert_eq!(
        svc.set_failed(failed, "mail_smtp", Some("connection refused"))
            .await
            .expect("failed"),
        TraceTransition::Moved
    );
    let (status, ftype, reason): (String, String, Option<String>) = sqlx::query_as(
        r#"SELECT trace_status::text, failure_type::text, failure_reason
           FROM mailing.mailing_traces WHERE id = $1"#,
    )
    .bind(failed)
    .fetch_one(&db.pool)
    .await
    .expect("failed row");
    assert_eq!((status.as_str(), ftype.as_str()), ("error", "mail_smtp"));
    assert_eq!(reason.as_deref(), Some("connection refused"));
    db.dispose().await;
}

#[tokio::test]
async fn click_pair_stamps_the_click_without_a_state_downgrade() {
    let Some(db) = TestDb::new("click").await else {
        return skipped("click");
    };
    let svc = TraceWriteService::new(db.pool.clone());
    let trace_id = mint_outgoing(&db.pool, "clicker@example.id").await;
    // The public click route's pair: opened + clicked.
    assert_eq!(
        svc.set_opened(trace_id).await.expect("open"),
        TraceTransition::Moved
    );
    assert_eq!(
        svc.set_clicked(trace_id).await.expect("click"),
        TraceTransition::Moved
    );
    let (status, clicked): (String, bool) = sqlx::query_as(
        r#"SELECT trace_status::text, links_click_datetime IS NOT NULL
           FROM mailing.mailing_traces WHERE id = $1"#,
    )
    .bind(trace_id)
    .fetch_one(&db.pool)
    .await
    .expect("click row");
    assert_eq!(status, "open");
    assert!(clicked, "the click fact is stamped");
    db.dispose().await;
}

#[tokio::test]
async fn reconcile_applies_settled_transport_verdicts() {
    let Some(db) = TestDb::new("recon").await else {
        return skipped("recon");
    };
    // A done mailing with one outgoing trace attached to a settled mail row.
    let (mailing_id, message_id) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    sqlx::query(
        r#"INSERT INTO mailing.mailings (id, subject, body_html, email_from, state)
           VALUES ($1, 'Reconcile', '<p>x</p>', 'c@example.id', 'done')"#,
    )
    .bind(mailing_id)
    .execute(&db.pool)
    .await
    .expect("mailing");
    let mail_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO messaging.mail_messages (id, body)
           VALUES ($1, '<p>x</p>')"#,
    )
    .bind(message_id)
    .execute(&db.pool)
    .await
    .expect("message");
    sqlx::query(
        r#"INSERT INTO messaging.mails (id, mail_message_id, email_to, state)
           VALUES ($1, $2, 'r@example.id', 'sent')"#,
    )
    .bind(mail_id)
    .bind(message_id)
    .execute(&db.pool)
    .await
    .expect("mail row");
    let mut tx = db.pool.begin().await.expect("tx");
    let trace_id = Uuid::new_v4();
    TraceRepository::mint_trace(
        &mut tx,
        trace_id,
        mailing_id,
        None,
        "mailing_contact",
        Uuid::new_v4(),
        "r@example.id",
        "outgoing",
        None,
        false,
    )
    .await
    .expect("mint");
    TraceRepository::attach_mail_id(&mut tx, trace_id, mail_id)
        .await
        .expect("attach");
    tx.commit().await.expect("commit");

    // The sweep's reconcile arm (step 7) converges the settled verdict.
    let engine =
        backbone_mailing::application::service::mailing_write_service::MailingWriteService::new(
            db.pool.clone(),
        );
    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.reconcile_sent, 1, "the settled mail flips the trace");
    let status: String =
        sqlx::query_scalar("SELECT trace_status::text FROM mailing.mailing_traces WHERE id = $1")
            .bind(trace_id)
            .fetch_one(&db.pool)
            .await
            .expect("status");
    assert_eq!(status, "sent");
    db.dispose().await;
}

#[tokio::test]
async fn mint_fence_rejects_duplicate_live_traces_and_cancel_reopens() {
    let Some(db) = TestDb::new("fence").await else {
        return skipped("fence");
    };
    let (mailing_id, recipient_id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut tx = db.pool.begin().await.expect("tx");
    TraceRepository::mint_trace(
        &mut tx,
        Uuid::new_v4(),
        mailing_id,
        None,
        "mailing_contact",
        recipient_id,
        "fence@example.id",
        "outgoing",
        None,
        false,
    )
    .await
    .expect("first mint");
    tx.commit().await.expect("commit");

    // The partial unique is the fence: a second LIVE mint for the same
    // (mailing, recipient) refuses loudly.
    let mut tx = db.pool.begin().await.expect("tx");
    let dup = TraceRepository::mint_trace(
        &mut tx,
        Uuid::new_v4(),
        mailing_id,
        None,
        "mailing_contact",
        recipient_id,
        "fence@example.id",
        "outgoing",
        None,
        false,
    )
    .await;
    assert!(
        dup.is_err(),
        "the (mailing, recipient) unique must fire on a duplicate live mint"
    );
    let err_str = format!("{:?}", dup.err().unwrap());
    assert!(err_str.contains("23505"), "unique violation, got {err_str}");
    tx.rollback().await.expect("rollback");

    // Cancel rows sit OUTSIDE the fence: cancel + re-mint is legal
    // (suppression visibility + re-targeting after re-subscribe).
    let mut tx = db.pool.begin().await.expect("tx");
    let cancel_id = TraceRepository::mint_trace(
        &mut tx,
        Uuid::new_v4(),
        mailing_id,
        None,
        "mailing_contact",
        recipient_id,
        "fence@example.id",
        "cancel",
        Some("mail_bl"),
        false,
    )
    .await
    .expect("cancel mint");
    tx.commit().await.expect("commit");
    let _ = cancel_id;
    db.dispose().await;
}

// ── SMS-overlay verbs (same shapes, channel-specific edges) ──────────────────

/// Mint an SMS-channel trace directly (the repository mint is mail-typed;
/// sms traces are minted by the sms send walk — tests bypass it on purpose
/// and may stamp the gateway seam by hand or leave it for the attach verb).
async fn mint_sms_trace(pool: &sqlx::PgPool, phone_email: &str, sms_uuid: Option<&str>) -> Uuid {
    let (trace_id, mailing_id, recipient_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        r#"INSERT INTO mailing.mailing_traces
               (id, trace_type, mailing_id, recipient_model, recipient_id,
                recipient_email, sms_uuid, trace_status, metadata)
           VALUES ($1, 'sms', $2, 'mailing_contact', $3, $4, $5, 'outgoing',
                   jsonb_build_object('created_at', to_jsonb(now())))"#,
    )
    .bind(trace_id)
    .bind(mailing_id)
    .bind(recipient_id)
    .bind(phone_email)
    .bind(sms_uuid)
    .execute(pool)
    .await
    .expect("sms trace mint");
    trace_id
}

#[tokio::test]
async fn sms_verbs_advance_the_partial_order_and_replays_skip() {
    let Some(db) = TestDb::new("smsrank").await else {
        return skipped("smsrank");
    };
    let svc = TraceWriteService::new(db.pool.clone());

    // The compressed step: a tracker already past 'process' advances an
    // outgoing trace to pending in ONE move (the partial order the
    // outgoing→pending edge admits).
    let compressed = mint_sms_trace(&db.pool, "compressed@example.id", Some("u-compressed")).await;
    assert_eq!(
        svc.set_pending(compressed).await.expect("compressed"),
        TraceTransition::Moved
    );
    // A LATE process verdict must not downgrade a pending row.
    assert_eq!(
        svc.set_process(compressed).await.expect("late process"),
        TraceTransition::Skipped
    );
    // Pending replay skips.
    assert_eq!(
        svc.set_pending(compressed).await.expect("replay"),
        TraceTransition::Skipped
    );

    // The ladder: outgoing → process → pending → sent, each replay a skip.
    let ladder = mint_sms_trace(&db.pool, "ladder@example.id", None).await;
    assert_eq!(
        svc.set_process(ladder).await.expect("process"),
        TraceTransition::Moved
    );
    assert_eq!(
        svc.set_pending(ladder).await.expect("pending"),
        TraceTransition::Moved
    );
    assert_eq!(
        svc.set_sent(ladder).await.expect("sent"),
        TraceTransition::Moved
    );
    let sent_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT sent_datetime FROM mailing.mailing_traces WHERE id = $1")
            .bind(ladder)
            .fetch_one(&db.pool)
            .await
            .expect("sent stamp");
    assert!(sent_at.is_some(), "set_sent stamps sent_datetime on the sms channel too");
    // Everything below sent skips — the rank guard holds for the sms edges.
    assert_eq!(
        svc.set_bounced_sms(ladder, "sms_invalid_destination", None)
            .await
            .expect("bounce below sent"),
        TraceTransition::Skipped
    );

    // A non-SMS failure code is refused at the verb door — never cast.
    let strict = mint_sms_trace(&db.pool, "strict@example.id", Some("u-strict")).await;
    let refused = svc
        .set_bounced_sms(strict, "mail_bounce", None)
        .await
        .expect_err("mail_bounce must be refused by the sms verb");
    assert_eq!(refused.code(), "invalid_input");

    // The gateway seam stamps once: a second attach never overwrites.
    assert_eq!(
        svc.attach_sms_uuid(ladder, "u-ladder").await.expect("attach"),
        TraceTransition::Moved
    );
    assert_eq!(
        svc.attach_sms_uuid(ladder, "u-other").await.expect("second"),
        TraceTransition::Skipped
    );
    let seam: String =
        sqlx::query_scalar("SELECT sms_uuid FROM mailing.mailing_traces WHERE id = $1")
            .bind(ladder)
            .fetch_one(&db.pool)
            .await
            .expect("seam");
    assert_eq!(seam, "u-ladder");

    // Unknown ids are Missing on the sms verbs too.
    assert_eq!(
        svc.set_process(Uuid::new_v4()).await.expect("unknown"),
        TraceTransition::Missing
    );
    db.dispose().await;
}

#[tokio::test]
async fn sms_bounce_is_channel_pure_and_never_feeds_the_mail_auto_blacklist() {
    let Some(db) = TestDb::new("smspure").await else {
        return skipped("smspure");
    };
    let svc = TraceWriteService::new(db.pool.clone());
    let bounced = mint_sms_trace(&db.pool, "phone-holder@example.id", Some("u-pure")).await;

    assert_eq!(
        svc.set_bounced_sms(bounced, "sms_invalid_destination", Some("dead number"))
            .await
            .expect("sms bounce"),
        TraceTransition::Moved
    );
    let (status, ftype, reason, opened): (String, String, Option<String>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            r#"SELECT trace_status::text, failure_type::text, failure_reason, open_datetime
               FROM mailing.mailing_traces WHERE id = $1"#,
        )
        .bind(bounced)
        .fetch_one(&db.pool)
        .await
        .expect("bounce row");
    assert_eq!(status, "bounce");
    assert_eq!(ftype, "sms_invalid_destination", "the code rides verbatim");
    assert_eq!(reason.as_deref(), Some("dead number"));
    assert!(
        opened.is_none(),
        "an sms bounce is not a mail touch — open_datetime stays unstamped"
    );

    // The isolation proof, with a POSITIVE CONTROL so the negative is
    // meaningful: the auto-blacklist sweep enrolls only a PATTERN of mail
    // bounces (>= max_bounces spread > spread_days — one incidental bounce
    // never blacklists), so the control is two mail_bounce traces for one
    // address, created a week apart, against thresholds (2, 7 days). The
    // SMS bounce — same trace_status, different failure vocabulary — must
    // NOT enroll under the same thresholds.
    let control_email = "mail-bounce-control@example.id";
    let control_old = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mailing.mailing_traces
               (id, trace_type, mailing_id, recipient_model, recipient_id,
                recipient_email, trace_status, failure_type)
           VALUES ($1, 'mail', $2, 'mailing_contact', $3, $4, 'bounce', 'mail_bounce')"#,
    )
    .bind(control_old)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(control_email)
    .execute(&db.pool)
    .await
    .expect("old control bounce");
    // The audit trigger stamps created_at UNCONDITIONALLY on INSERT, so the
    // backdate rides an UPDATE (which only refreshes updated_at).
    sqlx::query(
        r#"UPDATE mailing.mailing_traces
           SET metadata = jsonb_build_object('created_at',
                                             to_jsonb(now() - interval '8 days'))
           WHERE id = $1"#,
    )
    .bind(control_old)
    .execute(&db.pool)
    .await
    .expect("backdate control bounce");
    let mail_control = mint_outgoing(&db.pool, control_email).await;
    assert_eq!(
        svc.set_bounced(mail_control).await.expect("control"),
        TraceTransition::Moved
    );
    sqlx::query(
        r#"INSERT INTO mailing.mailings (id, subject, body_html, email_from, state)
           VALUES ($1, 'Purity', '<p>x</p>', 'c@example.id', 'done')"#,
    )
    .bind(Uuid::new_v4())
    .execute(&db.pool)
    .await
    .expect("mailing");
    let engine =
        backbone_mailing::application::service::mailing_write_service::MailingWriteService::new(
            db.pool.clone(),
        )
        .with_auto_blacklist_config(
            backbone_mailing::application::service::mailing_write_service::AutoBlacklistConfig {
                max_bounces: 2,
                window_weeks: 13,
                spread_days: 7,
            },
        );
    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(
        out.auto_blacklisted, 1,
        "the mail-bounce control pattern (2 bounces, 8 days apart) enrolls"
    );
    let sms_enrolled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messaging.mail_blacklists WHERE email = 'phone-holder@example.id'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("sms blacklist probe");
    assert_eq!(
        sms_enrolled, 0,
        "an sms bounce never enrolls an email blacklist row — channel purity holds at the DB"
    );
    db.dispose().await;
}

/// The channel-parameterized mints: an sms trace is born with
/// `trace_type='sms'` and its canonical number, and the fenced variant
/// absorbs a duplicate as `Ok(false)` inside the same transaction. A
/// phone-channel trace minted through the mail-shaped verb would be
/// invisible to the delivery pump's sms-scoped verdict join — the
/// channel rides the mint.
#[tokio::test]
async fn channel_mint_stamps_sms_type_and_phone_and_the_fence_absorbs_duplicates() {
    let Some(db) = TestDb::new("chmint").await else {
        return skipped("chmint");
    };
    let (trace_id, mailing_id, recipient_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut tx = db.pool.begin().await.expect("tx");
    TraceRepository::mint_trace_channel(
        &mut tx,
        trace_id,
        mailing_id,
        None,
        "crm_lead",
        recipient_id,
        "", // a phone-only recipient carries no email anchor
        Some("+6281234567890"),
        "sms",
        "outgoing",
        None,
        false,
    )
    .await
    .expect("channel mint");
    // The fenced twin over the same (mailing, recipient): absorbed, not
    // an error — the surrounding transaction stays usable.
    let absorbed = TraceRepository::mint_trace_channel_fenced(
        &mut tx,
        Uuid::new_v4(),
        mailing_id,
        None,
        "crm_lead",
        recipient_id,
        "",
        Some("+6281234567890"),
        "sms",
        "outgoing",
        None,
        false,
    )
    .await
    .expect("fenced channel mint");
    assert!(!absorbed, "the mint fence absorbs the duplicate");
    tx.commit().await.expect("commit");

    let row: (String, String, Option<String>, String) = sqlx::query_as(
        r#"SELECT trace_type::text, trace_status::text, recipient_phone, recipient_email
           FROM mailing.mailing_traces WHERE id = $1"#,
    )
    .bind(trace_id)
    .fetch_one(&db.pool)
    .await
    .expect("channel trace");
    assert_eq!(row.0, "sms", "the channel rides the mint");
    assert_eq!(row.1, "outgoing");
    assert_eq!(row.2.as_deref(), Some("+6281234567890"));
    assert_eq!(row.3, "", "a phone-only recipient carries the empty anchor");
    db.dispose().await;
}
