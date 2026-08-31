//! The mass-mode VOLUME probe — campaign-scale sends through the real
//! enqueue seam, asserting the declared operating envelope:
//!
//! - **Batch shape**: the pass budget (`recipients_per_pass`) bounds each
//!   sweep's mints exactly; `mail_enqueue_batch` is the commit grain the
//!   replay window is sized on; a capped pass leaves the mailing `sending`
//!   and later sweeps resume through the seen-list to completion.
//! - **Pacing**: with `max_enqueues_per_second` set, a full pass takes at
//!   least (n-1) × 1000/eps of wall time — the provider rate ceiling is
//!   honored, not advisory.
//! - **Duplicate-mint containment**: two workers overlapping mid-drive at
//!   scale stay inside the fence — the union of live (mailing, recipient)
//!   pairs never exceeds the audience, never doubles a pair, and the final
//!   count is exactly the audience.
//! - **Seam totality**: at scale, every live outgoing trace carries its
//!   `messaging.mails` row and every mail row belongs to a trace.

use super::common::*;
use backbone_mailing::application::service::mailing_write_service::{
    MailingSendConfig, MailingUpsertCommand, MailingWriteService,
};
use std::time::Instant;
use uuid::Uuid;

/// The campaign scale for this probe: comfortably past the two-thousand mark
/// a mass mailing is expected to clear in one launch.
const AUDIENCE: i64 = 2_200;
/// The per-sweep pass budget: forces the multi-pass resume shape a volume
/// send actually runs with (two full passes plus a remainder, so the overlap
/// round still has budget left to contend over).
const PASS: i64 = 900;
/// The commit grain (mail rows per batch transaction).
const BATCH: usize = 100;
/// The provider ceiling used for the pacing assertion.
const EPS: u64 = 300;

/// Bulk-seed one audience of n contacts (set-based inserts — the per-row
/// helper would spend the probe's budget on seeding round trips).
async fn seed_bulk(pool: &sqlx::PgPool, n: i64) -> Uuid {
    // gen_random_uuid() is builtin on modern Postgres and provided by
    // pgcrypto where it is not; the scratch superuser may ensure it.
    let _ = sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
        .execute(pool)
        .await;
    let audience_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mailing.mailing_audiences (id, name) VALUES ($1, 'Volume')"#,
    )
    .bind(audience_id)
    .execute(pool)
    .await
    .expect("seed audience");
    sqlx::query(
        r#"INSERT INTO mailing.mailing_contacts (id, email, name)
           SELECT gen_random_uuid(),
                  'bulk' || g || '@scale.example.id',
                  'Bulk ' || g
           FROM generate_series(1, $1) g"#,
    )
    .bind(n)
    .execute(pool)
    .await
    .expect("bulk contacts");
    sqlx::query(
        r#"INSERT INTO mailing.mailing_subscriptions (contact_id, mailing_audience_id)
           SELECT c.id, $1
           FROM mailing.mailing_contacts c
           WHERE c.email LIKE 'bulk%@scale.example.id'"#,
    )
    .bind(audience_id)
    .execute(pool)
    .await
    .expect("bulk subscriptions");
    audience_id
}

fn volume_cfg() -> MailingSendConfig {
    MailingSendConfig {
        claim_batch: 16,
        recipients_per_pass: PASS,
        mail_enqueue_batch: BATCH,
        inter_batch_delay_ms: 2,
        max_enqueues_per_second: Some(EPS),
        resolver_hard_cap: 100_000,
    }
}

async fn duplicate_pairs(pool: &sqlx::PgPool) -> i64 {
    count(
        pool,
        r#"SELECT count(*) FROM (
               SELECT mailing_id, recipient_id
               FROM mailing.mailing_traces
               WHERE trace_status <> 'cancel'
                 AND (metadata->>'deleted_at') IS NULL
               GROUP BY mailing_id, recipient_id
               HAVING count(*) > 1
           ) d"#,
    )
    .await
}

async fn distinct_reached(pool: &sqlx::PgPool) -> i64 {
    count(
        pool,
        r#"SELECT count(DISTINCT (mailing_id, recipient_id))
           FROM mailing.mailing_traces
           WHERE trace_status <> 'cancel'
             AND (metadata->>'deleted_at') IS NULL"#,
    )
    .await
}

#[tokio::test]
async fn campaign_scale_send_keeps_batch_shape_pacing_and_the_fence() {
    let Some(db) = TestDb::new("vol").await else {
        return skipped("vol");
    };
    let audience_id = seed_bulk(&db.pool, AUDIENCE).await;
    let engine = MailingWriteService::new(db.pool.clone());
    let id = engine
        .create_mailing(&MailingUpsertCommand {
            subject: "Volume probe".into(),
            preview: None,
            body_html: "<p>volume</p>".into(),
            email_from: "volume@example.id".into(),
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
    engine.launch(id, "immediate", None).await.expect("launch");

    // ── Pass 1: timed, budget-capped, rate-limited ──────────────────────────
    let worker = MailingWriteService::new(db.pool.clone()).with_send_config(volume_cfg());
    let started = Instant::now();
    let first = worker.send_queue_sweep().await.expect("pass 1");
    let pass1_elapsed = started.elapsed();

    assert_eq!(first.claimed, 1);
    assert_eq!(
        first.recipients_resolved, AUDIENCE as usize,
        "the whole campaign resolved"
    );
    assert_eq!(
        first.minted, PASS as usize,
        "the pass budget bounds the sweep's mints exactly"
    );
    assert_eq!(first.still_sending, 1, "a capped pass stays sending");
    assert_eq!(first.completed, 0);
    assert_eq!(duplicate_pairs(&db.pool).await, 0);
    assert_eq!(distinct_reached(&db.pool).await, PASS);

    // Pacing: n enqueues at eps/second need at least (n-1)*1000/eps ms of
    // wall time (the floor, not the target — DB work rides on top).
    let floor_ms = (PASS as u64 - 1) * (1000 / EPS);
    assert!(
        pass1_elapsed.as_millis() as u64 >= floor_ms,
        "pass 1 took {:?}, pacing floor was {floor_ms}ms",
        pass1_elapsed
    );

    // ── The overlap round at scale: two workers, one `sending` mailing ──────
    let worker_a = MailingWriteService::new(db.pool.clone()).with_send_config(volume_cfg());
    let worker_b = MailingWriteService::new(db.pool.clone()).with_send_config(volume_cfg());
    let (a, b) = tokio::join!(worker_a.send_queue_sweep(), worker_b.send_queue_sweep());
    let (a, b) = (a.expect("overlap A"), b.expect("overlap B"));
    let reached = distinct_reached(&db.pool).await;
    assert!(
        ((2 * PASS)..=AUDIENCE).contains(&reached),
        "post-overlap union {reached} outside [2*PASS, AUDIENCE]"
    );
    assert_eq!(
        duplicate_pairs(&db.pool).await,
        0,
        "the fence held at campaign scale"
    );
    assert_eq!(
        a.minted + b.minted,
        (reached - PASS) as usize,
        "minted counts exactly account for the new rows (fence skips absorbed)"
    );
    assert!(
        a.fence_skips + b.fence_skips <= (reached - PASS) as usize,
        "fence skips never exceed the collided rows"
    );

    // ── Sweep to convergence ────────────────────────────────────────────────
    let finisher = MailingWriteService::new(db.pool.clone()).with_send_config(volume_cfg());
    let mut rounds = 0;
    loop {
        let out = finisher.send_queue_sweep().await.expect("convergence sweep");
        rounds += 1;
        if out.completed == 1 || out.claimed == 0 || rounds > 6 {
            break;
        }
    }

    // ── Final invariants ────────────────────────────────────────────────────
    let state: String =
        sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("state");
    assert_eq!(state, "done", "the campaign completed across resumed passes");
    assert_eq!(distinct_reached(&db.pool).await, AUDIENCE);
    assert_eq!(duplicate_pairs(&db.pool).await, 0);
    assert_eq!(
        count(&db.pool, "SELECT count(*) FROM messaging.mails").await,
        AUDIENCE,
        "one mail row per audience member, exactly"
    );
    assert_eq!(
        count(&db.pool, "SELECT count(*) FROM messaging.mail_messages").await,
        AUDIENCE,
        "one message row per audience member, exactly"
    );
    assert_eq!(
        count(
            &db.pool,
            r#"SELECT count(*) FROM mailing.mailing_traces t
               WHERE t.trace_status <> 'cancel'
                 AND (t.metadata->>'deleted_at') IS NULL
                 AND t.mail_id IS NULL"#
        )
        .await,
        0,
        "no live trace without its mail row"
    );
    assert_eq!(
        count(
            &db.pool,
            r#"SELECT count(*) FROM messaging.mails m
               WHERE NOT EXISTS (
                   SELECT 1 FROM mailing.mailing_traces t
                   WHERE t.mail_id = m.id
                     AND (t.metadata->>'deleted_at') IS NULL)"#
        )
        .await,
        0,
        "no mail row without its trace"
    );
    // The send grain stayed healthy at scale: nothing errored at the seam.
    assert_eq!(
        count(
            &db.pool,
            r#"SELECT count(*) FROM mailing.mailing_traces
               WHERE trace_status = 'error'"#
        )
        .await,
        0
    );
    db.dispose().await;
}
