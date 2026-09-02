//! The two-workers correctness probe (MAIL-M67 containment evidence).
//!
//! The send engine's claim lock (`FOR UPDATE SKIP LOCKED`) is held only for
//! claim + state flip; cross-sweep overlap on the same mailing is contained
//! by the `(mailing, recipient)` mint fence plus the disposition probe — not
//! by long locks. These probes drive two independent sweep workers over one
//! scratch database and assert the containment invariants hold under BOTH
//! interleavings the scheduler may pick:
//!
//! 1. **Disjoint claim rounds** — many due mailings, both workers capped by
//!    `claim_batch`: the claims never overlap, everything completes, every
//!    recipient gets exactly one trace and one mail row.
//! 2. **Same-mailing overlap** — a pass-budget-capped mailing left `sending`
//!    is re-claimable by design; two workers overlap on it mid-drive and the
//!    fence absorbs the collision: the union of live traces grows by the
//!    pass budget at most, no `(mailing, recipient)` pair ever duplicates,
//!    and the final sweep converges to exactly the audience size.

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
use super::common::*;
use backbone_mailing::application::service::mailing_write_service::{
    MailingSendConfig, MailingUpsertCommand, MailingWriteService,
};
use backbone_mailing::infrastructure::persistence::trace_repository::TraceRepository;
use uuid::Uuid;

/// A minimal immediate mailing over one audience.
fn audience_cmd(audience_id: Uuid, subject: &str) -> MailingUpsertCommand {
    MailingUpsertCommand {
        subject: subject.into(),
        preview: None,
        body_html: format!("<p>{subject}</p>"),
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
    }
}

fn cfg(claim_batch: i64, recipients_per_pass: i64) -> MailingSendConfig {
    MailingSendConfig {
        claim_batch,
        recipients_per_pass,
        mail_enqueue_batch: 10,
        inter_batch_delay_ms: 0,
        max_enqueues_per_second: None,
        resolver_hard_cap: 100_000,
    }
}

/// Live non-cancel duplicate (mailing, recipient) pairs — must stay empty.
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

/// Distinct live non-cancel (mailing, recipient) pairs — the audience
/// actually reached.
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
async fn concurrent_workers_claim_disjoint_sets_and_complete_everything() {
    let Some(db) = TestDb::new("two").await else {
        return skipped("two");
    };

    // Six audiences of 25, six due mailings; both workers capped at 3 claims.
    const AUDIENCES: usize = 6;
    const MEMBERS: usize = 25;
    let mut mailing_ids = Vec::with_capacity(AUDIENCES);
    for i in 0..AUDIENCES {
        let (audience_id, _) =
            seed_audience(&db.pool, &format!("two{i}"), MEMBERS).await;
        let engine = MailingWriteService::new(db.pool.clone());
        let id = engine
            .create_mailing(&audience_cmd(audience_id, &format!("Probe {i}")))
            .await
            .expect("create");
        engine.launch(id, "immediate", None).await.expect("launch");
        mailing_ids.push(id);
    }

    let worker_a = MailingWriteService::new(db.pool.clone()).with_send_config(cfg(3, 500));
    let worker_b = MailingWriteService::new(db.pool.clone()).with_send_config(cfg(3, 500));
    let (a, b) = tokio::join!(worker_a.send_queue_sweep(), worker_b.send_queue_sweep());
    let (a, b) = (a.expect("worker A"), b.expect("worker B"));

    // SKIP LOCKED disjointness: 6 due mailings, batch 3 per worker — each
    // claimed exactly three, never the same one twice.
    assert_eq!(a.claimed, 3, "worker A claimed {}", a.claimed);
    assert_eq!(b.claimed, 3, "worker B claimed {}", b.claimed);
    assert_eq!(a.completed + b.completed, AUDIENCES, "all six completed");

    // Every mailing reached exactly its own audience.
    let per_mailing: Vec<(Uuid, i64)> = sqlx::query_as(
        r#"SELECT mailing_id, count(*) FROM mailing.mailing_traces
           WHERE trace_status <> 'cancel' AND (metadata->>'deleted_at') IS NULL
           GROUP BY 1"#,
    )
    .fetch_all(&db.pool)
    .await
    .expect("per-mailing grouping");
    assert_eq!(per_mailing.len(), AUDIENCES);
    for (_mid, n) in &per_mailing {
        assert_eq!(*n, MEMBERS as i64, "each mailing reached its 25 members");
    }

    // The containment invariants at the seam grain.
    assert_eq!(duplicate_pairs(&db.pool).await, 0, "no duplicate pairs");
    assert_eq!(
        distinct_reached(&db.pool).await,
        (AUDIENCES * MEMBERS) as i64
    );
    assert_eq!(
        a.minted + b.minted,
        AUDIENCES * MEMBERS,
        "minted counts sum to the exact audience"
    );
    // One mail row (and one message row) per reached recipient.
    assert_eq!(
        count(&db.pool, "SELECT count(*) FROM messaging.mails").await,
        (AUDIENCES * MEMBERS) as i64
    );
    assert_eq!(
        count(&db.pool, "SELECT count(*) FROM messaging.mail_messages").await,
        (AUDIENCES * MEMBERS) as i64
    );
    let states: Vec<(Uuid, String)> = sqlx::query_as(
        r#"SELECT id, state::text FROM mailing.mailings ORDER BY id"#,
    )
    .fetch_all(&db.pool)
    .await
    .expect("states");
    assert!(states.iter().all(|(_, s)| s == "done"));
    db.dispose().await;
}

#[tokio::test]
async fn overlapping_workers_on_one_mailing_never_double_send() {
    let Some(db) = TestDb::new("ovl").await else {
        return skipped("ovl");
    };
    const AUDIENCE: usize = 60;
    const PASS: usize = 25;
    let (audience_id, emails) = seed_audience(&db.pool, "ovl", AUDIENCE).await;
    let engine = MailingWriteService::new(db.pool.clone());
    let id = engine
        .create_mailing(&audience_cmd(audience_id, "Overlap probe"))
        .await
        .expect("create");
    engine.launch(id, "immediate", None).await.expect("launch");

    // Pass 1 (solo): the pass budget caps at 25 of 60 — the mailing stays
    // `sending` (re-claimable by design; the next sweep resumes).
    let solo = MailingWriteService::new(db.pool.clone()).with_send_config(cfg(16, PASS as i64));
    let first = solo.send_queue_sweep().await.expect("pass 1");
    assert_eq!(first.claimed, 1);
    assert_eq!(first.minted, PASS, "the pass budget bounds the batch");
    assert_eq!(first.still_sending, 1);
    assert_eq!(first.completed, 0);
    assert_eq!(duplicate_pairs(&db.pool).await, 0);
    assert_eq!(distinct_reached(&db.pool).await, PASS as i64);

    // The overlap round: two workers sweep the same `sending` mailing at the
    // same moment. SKIP LOCKED may hand it to just one (the other claims
    // nothing), or the second may re-claim it after the first's claim
    // transaction committed — in which case both walk the SAME remaining
    // recipients and the mint fence absorbs every collision. Whichever
    // interleaving runs, the union grows by at most one pass worth.
    let worker_a = MailingWriteService::new(db.pool.clone()).with_send_config(cfg(16, PASS as i64));
    let worker_b = MailingWriteService::new(db.pool.clone()).with_send_config(cfg(16, PASS as i64));
    let (a, b) = tokio::join!(worker_a.send_queue_sweep(), worker_b.send_queue_sweep());
    let (a, b) = (a.expect("overlap A"), b.expect("overlap B"));
    let reached = distinct_reached(&db.pool).await;
    // The union grows by at least one pass worth (SKIP LOCKED handed the
    // mailing to at least one worker) and never past the audience.
    assert!(
        ((2 * PASS) as i64..=(AUDIENCE as i64)).contains(&reached),
        "union after the overlap round was {reached}"
    );
    assert_eq!(
        duplicate_pairs(&db.pool).await,
        0,
        "the fence held: no (mailing, recipient) pair doubled"
    );
    assert_eq!(
        a.minted + b.minted,
        reached as usize - PASS,
        "minted counts exactly account for the new rows"
    );

    // Sweep to convergence: whatever the overlap left, later passes resume
    // through the seen-list and the mailing completes with EXACTLY the
    // audience — never more.
    let mut rounds = 0;
    loop {
        let out = solo.send_queue_sweep().await.expect("resume sweep");
        rounds += 1;
        if out.completed == 1 || out.claimed == 0 || rounds > 6 {
            break;
        }
    }
    assert_eq!(
        distinct_reached(&db.pool).await,
        AUDIENCE as i64,
        "every member reached exactly once"
    );
    assert_eq!(duplicate_pairs(&db.pool).await, 0);
    let state: String =
        sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("state");
    assert_eq!(state, "done");
    assert_eq!(
        count(&db.pool, "SELECT count(*) FROM messaging.mails").await,
        AUDIENCE as i64,
        "one mail row per member, no duplicates from the overlap"
    );
    // The trace↔mail attach seam stayed total: no live outgoing trace
    // without its mail row, no mail row without a trace.
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
        "every live trace carries its mail row"
    );
    let _ = emails;
    db.dispose().await;
}

#[tokio::test]
async fn orphan_repair_heals_aged_gaps_and_leaves_live_windows_alone() {
    let Some(db) = TestDb::new("fix").await else {
        return skipped("fix");
    };
    let engine = MailingWriteService::new(db.pool.clone());

    // Two mailings over an empty audience (nothing new to mint — the sweep
    // pass exists here only to run its repair arm), each carrying one
    // outgoing trace whose enqueue never happened: one AGED (a crash left
    // it behind ten minutes ago), one FRESH (a concurrent worker is
    // between mint and enqueue right now).
    let mut mailings = Vec::new();
    for subject in ["Aged gap", "Live window"] {
        let (audience_id, _) = seed_audience(&db.pool, subject, 0).await;
        let id = engine
            .create_mailing(&audience_cmd(audience_id, subject))
            .await
            .expect("create");
        let mut tx = db.pool.begin().await.expect("tx");
        let trace_id = Uuid::new_v4();
        TraceRepository::mint_trace(
            &mut tx,
            trace_id,
            id,
            None,
            "mailing_contact",
            Uuid::new_v4(),
            &format!("{subject}@example.id"),
            "outgoing",
            None,
            false,
        )
        .await
        .expect("orphan mint");
        tx.commit().await.expect("commit");
        engine.launch(id, "immediate", None).await.expect("launch");
        mailings.push((id, trace_id, subject));
    }
    // Age the FIRST mailing's trace past the repair grace.
    sqlx::query(
        r#"UPDATE mailing.mailing_traces
           SET metadata = jsonb_build_object('created_at',
                                             to_jsonb(now() - interval '10 minutes'))
           WHERE id = $1"#,
    )
    .bind(mailings[0].1)
    .execute(&db.pool)
    .await
    .expect("backdate");

    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.repaired, 1, "only the aged gap is repaired");
    assert_eq!(out.completed_empty, 2);

    let aged_mail: Option<Uuid> =
        sqlx::query_scalar("SELECT mail_id FROM mailing.mailing_traces WHERE id = $1")
            .bind(mailings[0].1)
            .fetch_one(&db.pool)
            .await
            .expect("aged row");
    assert!(aged_mail.is_some(), "the crash gap healed: enqueue + attach ran");
    let fresh_mail: Option<Uuid> =
        sqlx::query_scalar("SELECT mail_id FROM mailing.mailing_traces WHERE id = $1")
            .bind(mailings[1].1)
            .fetch_one(&db.pool)
            .await
            .expect("fresh row");
    assert!(
        fresh_mail.is_none(),
        "the live mint→enqueue window was NOT double-enqueued"
    );
    assert_eq!(
        count(&db.pool, "SELECT count(*) FROM messaging.mails").await,
        1,
        "exactly one mail row — no duplicate send at the seam"
    );
    db.dispose().await;
}
