//! Subscription opt-out split + audience archive guard (fresh-DB,
//! disposable scratch per test).

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
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
    let audience = svc.create_audience("Newsletter", true).await.expect("aud");

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
    let audience = svc.create_audience("Deals", true).await.expect("aud");
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
    let list_a = subs.create_audience("List A", true).await.expect("A");
    let list_b = subs.create_audience("List B", true).await.expect("B");

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
        source_id: None,
        ab_test_id: None,
        ab_testing_enabled: false,
        ab_testing_pc: 0,
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
        source_id: None,
        ab_test_id: None,
        ab_testing_enabled: false,
        ab_testing_pc: 0,
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

/// The ONE self-service guard: the subscribe verb admits only PUBLIC
/// audiences, and a private audience, an unknown id, and an archived
/// audience all refuse with the SAME typed error — the caller cannot
/// distinguish which id-shape was wrong (the enumeration-oracle posture
/// the public subscription surface carries).
#[tokio::test]
async fn subscribe_refuses_private_and_unknown_audiences_identically() {
    let Some(db) = TestDb::new("subgate").await else {
        return skipped("subgate");
    };
    let svc = SubscriptionWriteService::new(db.pool.clone());

    let private = svc.create_audience("Members only", false).await.expect("aud");
    let err = svc
        .subscribe("fan@example.id", private, None)
        .await
        .expect_err("private audience must refuse");
    assert_eq!(err.http_status(), 422);
    assert_eq!(err.code(), "audience_not_public");

    let unknown = svc
        .subscribe("fan@example.id", Uuid::new_v4(), None)
        .await
        .expect_err("unknown audience must refuse");
    assert_eq!(unknown.code(), err.code(), "unknown and private must be indistinguishable");
    assert_eq!(std::mem::discriminant(&unknown), std::mem::discriminant(&err));

    // The sanctioned shape still passes: a public audience subscribes.
    let public = svc.create_audience("Open newsletter", true).await.expect("pub");
    let row = svc
        .subscribe("fan@example.id", public, None)
        .await
        .expect("public audience subscribes");
    assert!(!row.opt_out);
    db.dispose().await;
}

/// The anti-oracle unsubscribe: an unknown contact and a known contact
/// with no subscription on the audience answer the SAME typed refusal —
/// the mailing-list membership enumeration the public surface must
/// never expose.
#[tokio::test]
async fn unsubscribe_by_email_conflates_unknown_contact_and_missing_subscription() {
    let Some(db) = TestDb::new("subconflate").await else {
        return skipped("subconflate");
    };
    let svc = SubscriptionWriteService::new(db.pool.clone());
    let audience = svc.create_audience("Newsletter", true).await.expect("aud");
    // A known contact with NO subscription on this audience.
    let other = svc.create_audience("Other list", true).await.expect("other");
    svc.subscribe("known@example.id", other, None).await.expect("seed");

    let stranger = svc
        .unsubscribe_by_email("stranger@example.id", audience, None)
        .await
        .expect_err("unknown contact must refuse");
    let known = svc
        .unsubscribe_by_email("known@example.id", audience, None)
        .await
        .expect_err("known contact without subscription must refuse");

    assert_eq!(stranger.http_status(), 404);
    assert_eq!(known.http_status(), 404);
    assert_eq!(
        format!("{stranger}"),
        format!("{known}"),
        "the two refusal arms must be byte-identical to the caller"
    );
    assert_eq!(stranger.code(), known.code());
    db.dispose().await;
}
