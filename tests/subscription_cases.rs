//! Subscription opt-out split + audience archive guard (fresh-DB,
//! disposable scratch per test).

#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use backbone_mailing::application::service::mailing_write_service::{
    MailingSendConfig, MailingUpsertCommand, MailingWriteService,
};
use backbone_mailing::application::service::subscription_write_service::SubscriptionWriteService;
use uuid::Uuid;

#[tokio::test]
async fn subscribe_is_idempotent_and_resubscribe_clears_the_optout() {
    let Some(db) = TestDb::new("sub").await else {
        return skipped("sub");
    };
    let svc = SubscriptionWriteService::new(db.pool.clone());
    let audience = svc.create_audience("Newsletter", false).await.expect("aud");

    let row = svc
        .subscribe("fan@example.id", audience, Some("Fan"))
        .await
        .expect("subscribe");
    assert!(!row.opt_out);
    // A second subscribe converges onto the SAME live row — no duplicate.
    let again = svc
        .subscribe("fan@example.id", audience, None)
        .await
        .expect("re-subscribe");
    assert_eq!(row.id, again.id);
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_subscriptions",
    )
    .fetch_one(&db.pool)
    .await
    .expect("count");
    assert_eq!(rows, 1);

    // Opt out, then re-subscribe: the FLIP goes back, datetime + reason clear.
    let (flipped, was_new) = svc
        .opt_out(again.contact_id, audience, None)
        .await
        .expect("opt out");
    assert!(was_new);
    assert!(flipped.opt_out);
    assert!(flipped.opt_out_datetime.is_some(), "DB-clock stamp");
    let revived = svc
        .subscribe("fan@example.id", audience, None)
        .await
        .expect("revive");
    assert!(!revived.opt_out);
    assert!(revived.opt_out_datetime.is_none());
    // The row SURVIVED the whole dance — the split never deletes.
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_subscriptions",
    )
    .fetch_one(&db.pool)
    .await
    .expect("count");
    assert_eq!(rows, 1);
    db.dispose().await;
}

#[tokio::test]
async fn repeated_unsubscribe_keeps_the_first_optout_moment() {
    let Some(db) = TestDb::new("opt").await else {
        return skipped("opt");
    };
    let svc = SubscriptionWriteService::new(db.pool.clone());
    let audience = svc.create_audience("Deals", false).await.expect("aud");
    svc.subscribe("tired@example.id", audience, None)
        .await
        .expect("sub");
    let (_, first) = svc
        .unsubscribe_by_email("tired@example.id", audience, None)
        .await
        .expect("first unsubscribe");
    assert!(first, "the first unsubscribe flips");
    let stamp: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        r#"SELECT opt_out_datetime FROM mailing.mailing_subscriptions
           WHERE (metadata->>'deleted_at') IS NULL"#,
    )
    .fetch_one(&db.pool)
    .await
    .expect("stamp");

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let (_, second) = svc
        .unsubscribe_by_email("tired@example.id", audience, None)
        .await
        .expect("second unsubscribe");
    assert!(!second, "the second unsubscribe is a no-flip echo");
    let stamp2: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        r#"SELECT opt_out_datetime FROM mailing.mailing_subscriptions
           WHERE (metadata->>'deleted_at') IS NULL"#,
    )
    .fetch_one(&db.pool)
    .await
    .expect("stamp2");
    assert_eq!(stamp, stamp2, "the FIRST opt-out moment is the durable fact");
    db.dispose().await;
}

#[tokio::test]
async fn optout_wins_across_lists_at_send_time() {
    let Some(db) = TestDb::new("wins").await else {
        return skipped("wins");
    };
    let subs = SubscriptionWriteService::new(db.pool.clone());
    let list_a = subs.create_audience("List A", false).await.expect("A");
    let list_b = subs.create_audience("List B", false).await.expect("B");

    // Member of BOTH lists; opts out of B only.
    subs.subscribe("dual@example.id", list_a, None)
        .await
        .expect("sub A");
    let row_b = subs
        .subscribe("dual@example.id", list_b, None)
        .await
        .expect("sub B");
    subs.opt_out(row_b.contact_id, list_b, None)
        .await
        .expect("opt out B");
    // A clean member of A for contrast.
    subs.subscribe("clean@example.id", list_a, None)
        .await
        .expect("sub clean");

    // A mailing on list A (NOT the list the member opted out of).
    let engine = MailingWriteService::new(db.pool.clone());
    let cmd = MailingUpsertCommand {
        subject: "Cross-list".into(),
        preview: None,
        body_html: "<p>x</p>".into(),
        email_from: "c@example.id".into(),
        reply_to: None,
        mailing_domain_raw: serde_json::json!([
            ["mailing_audience_id", "=", list_a.to_string()]
        ]),
        target_model: "mailing_contact".into(),
        schedule_type: "immediate".into(),
        schedule_date: None,
        use_exclusion_list: true,
        campaign_id: None,
    };
    let id = engine.create_mailing(&cmd).await.expect("create");
    engine.launch(id, "immediate", None).await.expect("launch");
    let out = engine.send_queue_sweep().await.expect("sweep");

    // OPT-OUT-WINS: the B opt-out suppresses the A send, visibly.
    assert_eq!(out.suppressed_optout, 1);
    assert_eq!(out.minted, 1, "only the clean member sends");
    let (email, ftype): (String, String) = sqlx::query_as(
        r#"SELECT recipient_email, failure_type::text FROM mailing.mailing_traces
           WHERE mailing_id = $1 AND trace_status = 'cancel'"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("cancel row");
    assert_eq!(email, "dual@example.id");
    assert_eq!(ftype, "mail_optout");
    let _ = MailingSendConfig::default();
    db.dispose().await;
}

#[tokio::test]
async fn archive_guard_refuses_while_a_mailing_cites_the_audience() {
    let Some(db) = TestDb::new("r7").await else {
        return skipped("r7");
    };
    let (audience_id, _) = seed_audience(&db.pool, "guarded", 2).await;
    let engine = MailingWriteService::new(db.pool.clone());
    let cmd = MailingUpsertCommand {
        subject: "In flight".into(),
        preview: None,
        body_html: "<p>x</p>".into(),
        email_from: "c@example.id".into(),
        reply_to: None,
        mailing_domain_raw: serde_json::json!([
            ["mailing_audience_id", "=", audience_id.to_string()]
        ]),
        target_model: "mailing_contact".into(),
        schedule_type: "immediate".into(),
        schedule_date: None,
        use_exclusion_list: true,
        campaign_id: None,
    };
    let id = engine.create_mailing(&cmd).await.expect("create");
    engine.launch(id, "immediate", None).await.expect("launch");

    let subs = SubscriptionWriteService::new(db.pool.clone());
    // R-M7: an in-flight mailing citing the audience blocks the archive.
    let err = subs
        .archive_audience(audience_id)
        .await
        .expect_err("must refuse");
    assert_eq!(err.http_status(), 409);
    assert_eq!(err.code(), "audience_in_use");

    // Complete the mailing, and the archive passes with the member count.
    engine.send_queue_sweep().await.expect("sweep");
    let members = subs.archive_audience(audience_id).await.expect("archive");
    assert_eq!(members, 2);
    db.dispose().await;
}
