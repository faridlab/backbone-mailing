//! Seeded A/B testing: the persisted sampling seed, the fragment partition,
//! winner promotion, and the one-live-test-per-campaign fence (fresh-DB,
//! disposable scratch per test).

#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use backbone_mailing::application::service::ab_test_write_service::AbTestWriteService;
use backbone_mailing::application::service::mailing_write_service::{
    MailingUpsertCommand, MailingWriteService,
};
use uuid::Uuid;

fn variant_cmd(audience_id: Uuid, campaign_id: Uuid, subject: &str) -> MailingUpsertCommand {
    MailingUpsertCommand {
        subject: subject.into(),
        preview: None,
        body_html: format!("<p>{subject}</p>"),
        email_from: "ab@example.id".into(),
        reply_to: None,
        mailing_domain_raw: serde_json::json!([
            ["mailing_audience_id", "=", audience_id.to_string()]
        ]),
        target_model: "mailing_contact".into(),
        schedule_type: "immediate".into(),
        schedule_date: None,
        use_exclusion_list: true,
        campaign_id: Some(campaign_id),
    }
}

#[tokio::test]
async fn the_sampling_seed_is_minted_once_and_persists() {
    let Some(db) = TestDb::new("seed").await else {
        return skipped("seed");
    };
    let svc = AbTestWriteService::new(db.pool.clone());
    let campaign = Uuid::new_v4();
    // Far-future promote_at: an automatic selection requires one, and this
    // test must stay undisturbed by the sweep's promotion step.
    let far = chrono::Utc::now() + chrono::Duration::hours(1);
    let id = svc
        .create_ab_test(campaign, "opened_ratio", Some(far))
        .await
        .expect("create");
    let view = svc.view(id).await.expect("view");
    assert_eq!(view.sampling_seed.len(), 32, "128 bits as 32 hex chars");
    assert!(view.sampling_seed.chars().all(|c| c.is_ascii_hexdigit()));
    // The seed NEVER regenerates: repeated views are byte-stable.
    let again = svc.view(id).await.expect("view again");
    assert_eq!(view.sampling_seed, again.sampling_seed);

    // One live test per campaign — the partial UNIQUE refuses a second.
    let err = svc
        .create_ab_test(campaign, "clicks_ratio", Some(far))
        .await
        .expect_err("second test must refuse");
    assert_eq!(err.http_status(), 409);
    db.dispose().await;
}

#[tokio::test]
async fn variants_partition_the_audience_with_no_overlap() {
    let Some(db) = TestDb::new("split").await else {
        return skipped("split");
    };
    let (audience_id, emails) = seed_audience(&db.pool, "split", 200).await;
    let campaign = Uuid::new_v4();
    let ab = AbTestWriteService::new(db.pool.clone());
    let engine = MailingWriteService::new(db.pool.clone());

    let far = chrono::Utc::now() + chrono::Duration::hours(1);
    let test_id = ab
        .create_ab_test(campaign, "opened_ratio", Some(far))
        .await
        .expect("test");
    let a = engine
        .create_mailing(&variant_cmd(audience_id, campaign, "Variant A"))
        .await
        .expect("A");
    let b = engine
        .create_mailing(&variant_cmd(audience_id, campaign, "Variant B"))
        .await
        .expect("B");
    // Both variants sample with the SAME persisted seed, so membership is
    // the SAME function — the fragments differ only through pc. A@50 takes
    // its ~half; B@100 targets the whole audience but the campaign-scoped
    // seen-list hands it exactly the remainder. Swept SEQUENTIALLY so the
    // drive order (and therefore which variant mints first) is determined
    // by the test, not by a created_at/id coin flip.
    ab.bind_variant(a, test_id, 50).await.expect("bind A");
    ab.bind_variant(b, test_id, 100).await.expect("bind B");
    engine.launch(a, "immediate", None).await.expect("launch A");
    let first = engine.send_queue_sweep().await.expect("sweep A");
    assert_eq!(first.claimed, 1);
    assert_eq!(first.completed, 1);
    engine.launch(b, "immediate", None).await.expect("launch B");
    let second = engine.send_queue_sweep().await.expect("sweep B");
    assert_eq!(second.claimed, 1);
    assert_eq!(second.completed, 1);

    // Every audience member carries EXACTLY one live non-cancel trace: the
    // variants are disjoint AND their union covers the whole audience.
    let per_recipient: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT lower(recipient_email), count(*) FROM mailing.mailing_traces
           WHERE trace_status <> 'cancel' AND (metadata->>'deleted_at') IS NULL
           GROUP BY 1"#,
    )
    .fetch_all(&db.pool)
    .await
    .expect("grouping");
    assert!(
        per_recipient.iter().all(|(_, n)| *n == 1),
        "no recipient carries two variant traces"
    );
    assert_eq!(
        per_recipient.len(),
        emails.len(),
        "the fragments cover the whole audience, disjointly"
    );
    let a_traces: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1",
    )
    .bind(a)
    .fetch_one(&db.pool)
    .await
    .expect("A traces");
    let b_traces: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1",
    )
    .bind(b)
    .fetch_one(&db.pool)
    .await
    .expect("B traces");
    // A's 50% HMAC fragment of 200 members: generously bounded; B takes
    // EXACTLY the remainder.
    assert!(
        (70..=130).contains(&a_traces),
        "variant A fragment was {a_traces}"
    );
    assert_eq!(b_traces, 200 - a_traces, "B gets exactly the remainder");
    db.dispose().await;
}

#[tokio::test]
async fn due_test_promotes_the_ranked_winner_idempotently() {
    let Some(db) = TestDb::new("promo").await else {
        return skipped("promo");
    };
    let (audience_id, emails) = seed_audience(&db.pool, "promo", 40).await;
    let campaign = Uuid::new_v4();
    let ab = AbTestWriteService::new(db.pool.clone());
    let engine = MailingWriteService::new(db.pool.clone());

    // Create with a FUTURE promote_at: sweep 1 (which drives both variants)
    // must not see a due test — it would promote on a zero-signal tie
    // before any opens have landed. The backdate below is what makes the
    // test due, after the opens land.
    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    let test_id = ab
        .create_ab_test(campaign, "opened_ratio", Some(future))
        .await
        .expect("test");
    let a = engine
        .create_mailing(&variant_cmd(audience_id, campaign, "Winner bait"))
        .await
        .expect("A");
    let b = engine
        .create_mailing(&variant_cmd(audience_id, campaign, "Loser bait"))
        .await
        .expect("B");
    ab.bind_variant(a, test_id, 100).await.expect("bind A");
    ab.bind_variant(b, test_id, 0).await.expect("bind B");
    engine.launch(a, "immediate", None).await.expect("launch A");
    engine.launch(b, "immediate", None).await.expect("launch B");
    // B at pc=0 covers nobody; complete it empty via its own sweep pass.
    engine.send_queue_sweep().await.expect("sweep 1");

    // Give variant A opens so the ranking has a signal. sent_datetime is
    // stamped too: the rank's denominator is SENT traces, so an open
    // without a sent stamp ranks as noise.
    sqlx::query(
        r#"UPDATE mailing.mailing_traces SET trace_status = 'open',
                  open_datetime = now(), sent_datetime = now()
           WHERE mailing_id = $1"#,
    )
    .bind(a)
    .execute(&db.pool)
    .await
    .expect("opens");

    // NOW the test is due.
    sqlx::query("UPDATE mailing.mailing_ab_tests SET promote_at = now() - interval '1 minute' WHERE id = $1")
        .bind(test_id)
        .execute(&db.pool)
        .await
        .expect("backdate promote_at");

    let out = engine.send_queue_sweep().await.expect("sweep 2");
    assert_eq!(out.ab_promotions, 1, "the due test promotes its winner");
    let view = ab.view(test_id).await.expect("view");
    assert!(view.completed);
    assert_eq!(view.winner_mailing_id, Some(a), "opened_ratio ranks A first");

    // The winner copy: queued at pc=100 with sampling disabled (variant A is
    // ALSO at pc=100 — the disabled sampler is what distinguishes the copy).
    let promoted: Vec<(Uuid, String, bool, i32)> = sqlx::query_as(
        r#"SELECT id, state::text, ab_testing_enabled, ab_testing_pc
           FROM mailing.mailings
           WHERE ab_test_id = $1 AND NOT ab_testing_enabled"#,
    )
    .bind(test_id)
    .fetch_all(&db.pool)
    .await
    .expect("promoted row");
    assert_eq!(promoted.len(), 1);
    assert_eq!(promoted[0].1, "in_queue");
    assert!(!promoted[0].2, "the copy does not sample again");

    // Sweep 3 drives the copy: A already reached the whole audience (pc=100
    // with the same seed), so the copy's campaign-remainder audience is
    // EMPTY — it completes sending nobody. Promotion is idempotent.
    let third = engine.send_queue_sweep().await.expect("sweep 3");
    assert_eq!(third.ab_promotions, 0);
    assert_eq!(third.claimed, 1, "the winner copy is driven");
    assert_eq!(
        third.minted, 0,
        "the winner copy re-sends nobody the campaign already covered"
    );
    assert_eq!(third.completed, 1);
    let _ = emails;
    db.dispose().await;
}

#[tokio::test]
async fn manual_tests_wait_for_a_human_winner() {
    let Some(db) = TestDb::new("manual").await else {
        return skipped("manual");
    };
    let (audience_id, _) = seed_audience(&db.pool, "manual", 10).await;
    let campaign = Uuid::new_v4();
    let ab = AbTestWriteService::new(db.pool.clone());
    let engine = MailingWriteService::new(db.pool.clone());
    let test_id = ab
        .create_ab_test(campaign, "manual", None)
        .await
        .expect("manual test");
    let a = engine
        .create_mailing(&variant_cmd(audience_id, campaign, "Manual A"))
        .await
        .expect("A");
    ab.bind_variant(a, test_id, 100).await.expect("bind");
    engine.launch(a, "immediate", None).await.expect("launch");
    engine.send_queue_sweep().await.expect("sweep");

    // Even with promote_at long past, 'manual' never auto-promotes.
    sqlx::query("UPDATE mailing.mailing_ab_tests SET promote_at = now() - interval '1 hour' WHERE id = $1")
        .bind(test_id)
        .execute(&db.pool)
        .await
        .expect("backdate");
    let out = engine.send_queue_sweep().await.expect("sweep 2");
    assert_eq!(out.ab_promotions, 0, "manual selection is human-only");

    // The human picks: the winner copy is minted and queued, once.
    let promoted = ab.select_winner(test_id, a).await.expect("select");
    assert!(promoted.is_some());
    let err = ab
        .select_winner(test_id, a)
        .await
        .expect_err("second selection refuses");
    assert_eq!(err.http_status(), 409);
    db.dispose().await;
}
