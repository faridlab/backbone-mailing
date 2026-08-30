//! Trace verb behavior: the monotonic rank guards, the click pair, the
//! reconcile arm, and the mint fence (fresh-DB, disposable scratch per
//! test).

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
