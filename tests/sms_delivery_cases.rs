//! SMS-channel asynchronous done-inference (fresh-DB, disposable scratch
//! per test): the delivery-tracker pump advances sms-type traces from real
//! tracker verdicts and closes walked-out sms mailings exactly once.
//!
//! The substrate path is REAL end to end — enqueue through backbone-mail's
//! public sms service (row + tracker minted together), the drainer's claim,
//! then SIGNED delivery-report webhooks — so the pump is proven against the
//! exact durable facts production will hold, not seeded stand-ins. Only the
//! mailing rows and their traces are seeded directly (the sms send walk
//! that would mint them is a separate seam); every delivery verdict after
//! the enqueue is the substrate's own.

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
#[path = "behavior/common/mod.rs"]
mod common;

use backbone_mail::application::service::{SmsStatusWebhookService, WebhookOutcome};
use backbone_mail::infrastructure::persistence::sms_queue_repository::SmsQueueRepository;
use backbone_mailing::application::service::mailing_write_service::MailingWriteService;
use backbone_mailing::application::service::sms_delivery_pump_service::SmsDeliveryPumpService;
use backbone_mailing::infrastructure::persistence::mailing_send_repository::MailingSendRepository;
use chrono::{DateTime, Utc};
use common::*;
use uuid::Uuid;

const SECRET: &str = "pump-seat-webhook-secret";

fn hex_sig(secret: &[u8], body: &[u8]) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).unwrap();
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Seed one sms-type mailing in `sending` (the walk's end state). The
/// domain is the parsed-empty canonical form `[]` — the column's `{`
/// default is the raw-shape sentinel, which the claim-time parse refuses.
async fn seed_sms_mailing(pool: &sqlx::PgPool, marker: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mailing.mailings
               (id, subject, body_html, body_plaintext, email_from, state,
                mailing_type, mailing_domain)
           VALUES ($1, $2, '<p>x</p>', 'plain body', 'c@example.id', 'sending',
                   'sms', '[]'::jsonb)"#,
    )
    .bind(id)
    .bind(marker)
    .execute(pool)
    .await
    .expect("sms mailing seed");
    id
}

/// Stamp the walk-complete marker through the walk's own verb (state-guarded,
/// idempotent — the same shape the send walk will call).
async fn mark_walk(pool: &sqlx::PgPool, mailing_id: Uuid) {
    let mut tx = pool.begin().await.expect("tx");
    let marked = MailingSendRepository::mark_sms_walk_complete(&mut tx, mailing_id)
        .await
        .expect("mark walk");
    tx.commit().await.expect("commit");
    assert!(marked, "marker stamp must land on a fresh sending row");
}

/// Mint one sms-type trace bound to a gateway uuid (the sms send walk's
/// mint, seeded directly).
async fn seed_sms_trace(
    pool: &sqlx::PgPool,
    mailing_id: Uuid,
    email: &str,
    sms_uuid: &str,
) -> Uuid {
    let (trace_id, recipient_id) = (Uuid::new_v4(), Uuid::new_v4());
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
    .bind(email)
    .bind(sms_uuid)
    .execute(pool)
    .await
    .expect("sms trace seed");
    trace_id
}

/// Enqueue a real sms through the substrate's public service: returns the
/// row's external uuid (the tracker is minted in the same transaction).
async fn enqueue_real(pool: &sqlx::PgPool, number: &str, body: &str) -> String {
    let sms =
        backbone_mail::application::service::sms_write_service::SmsWriteService::new(pool.clone());
    let (_id, uuid) = sms
        .enqueue(number, body, None, None, None, None)
        .await
        .expect("substrate enqueue");
    uuid
}

/// One signed delivery report, exactly as the gateway would send it.
async fn report(svc: &SmsStatusWebhookService, status: &str, sms_uuid: &str) -> WebhookOutcome {
    let raw = format!(
        r#"{{"timestamp":"{}","sms_uuid":"{sms_uuid}","status":"{status}"}}"#,
        Utc::now().to_rfc3339()
    );
    let sig = hex_sig(SECRET.as_bytes(), raw.as_bytes());
    svc.handle(raw.as_bytes(), Some(&sig))
        .await
        .expect("webhook handle")
}

/// The full delivery walk: drainer claim (outgoing → process), the
/// acceptance report (→ pending), the delivery report (→ sent). Both the
/// sms row and its tracker end terminal.
async fn deliver_fully(pool: &sqlx::PgPool, svc: &SmsStatusWebhookService, sms_uuid: &str) {
    SmsQueueRepository::claim_batch_for_drain(&mut *pool.acquire().await.expect("conn"), 10)
        .await
        .expect("drain claim");
    assert_eq!(
        report(svc, "pending", sms_uuid).await,
        WebhookOutcome::Advanced,
        "acceptance report must land from process"
    );
    assert_eq!(
        report(svc, "sent", sms_uuid).await,
        WebhookOutcome::Advanced,
        "delivery report must land from pending"
    );
}

async fn mailing_state(pool: &sqlx::PgPool, id: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT state::text FROM mailing.mailings WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("mailing state")
}

async fn mailing_sent_date(pool: &sqlx::PgPool, id: Uuid) -> Option<DateTime<Utc>> {
    sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT sent_date FROM mailing.mailings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("mailing sent_date")
}

async fn trace_status(pool: &sqlx::PgPool, id: Uuid) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT trace_status::text FROM mailing.mailing_traces WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("trace status")
}

#[tokio::test]
async fn pump_infers_done_only_after_every_tracker_is_terminal() {
    let Some(db) = TestDb::new("pumpe2e").await else {
        return skipped("pumpe2e");
    };
    let webhook = SmsStatusWebhookService::new(db.pool.clone(), SECRET);
    let pump = SmsDeliveryPumpService::new(db.pool.clone());

    let mailing_id = seed_sms_mailing(&db.pool, "e2e").await;
    mark_walk(&db.pool, mailing_id).await;
    let uuid_a = enqueue_real(&db.pool, "+628110000201", "e2e A").await;
    let uuid_b = enqueue_real(&db.pool, "+628110000202", "e2e B").await;
    let trace_a = seed_sms_trace(&db.pool, mailing_id, "a@example.id", &uuid_a).await;
    let trace_b = seed_sms_trace(&db.pool, mailing_id, "b@example.id", &uuid_b).await;

    // Pass 1 — both trackers hold 'process' (born at enqueue): the traces
    // advance outgoing → process; the mailing is far from done.
    let out = pump.pump_once().await.expect("pass 1");
    assert_eq!(out.traces_advanced, 2, "both traces advance from outgoing");
    assert_eq!(out.candidates, 1);
    assert_eq!(out.mailings_completed, 0, "two transient traces remain");
    assert_eq!(trace_status(&db.pool, trace_a).await, "process");

    // Deliver A only. Pass 2 — A lands terminal, B still transient: the
    // inference must STILL not fire.
    deliver_fully(&db.pool, &webhook, &uuid_a).await;
    let out = pump.pump_once().await.expect("pass 2");
    assert_eq!(out.traces_advanced, 1);
    assert_eq!(out.mailings_completed, 0, "one transient trace remains");
    assert_eq!(trace_status(&db.pool, trace_a).await, "sent");
    assert_eq!(trace_status(&db.pool, trace_b).await, "process");
    assert_eq!(mailing_state(&db.pool, mailing_id).await, "sending");

    // Deliver B. Pass 3 — no transient trace remains under the lock: done.
    deliver_fully(&db.pool, &webhook, &uuid_b).await;
    let out = pump.pump_once().await.expect("pass 3");
    assert_eq!(out.traces_advanced, 1);
    assert_eq!(
        out.mailings_completed, 1,
        "the last terminal tracker closes the mailing"
    );
    assert_eq!(mailing_state(&db.pool, mailing_id).await, "done");
    let sent_date = mailing_sent_date(&db.pool, mailing_id).await;
    assert!(
        sent_date.is_some(),
        "completion stamps sent_date (the KPI fact)"
    );

    // Pass 4 — everything terminal: pure replay, nothing advances, nothing
    // completes, sent_date is untouched to the microsecond.
    let out = pump.pump_once().await.expect("pass 4");
    assert_eq!(out.traces_advanced, 0);
    assert_eq!(
        out.trace_skips, 0,
        "terminal traces leave the transient scan entirely"
    );
    assert_eq!(out.candidates, 0, "a done mailing is not a candidate");
    assert_eq!(out.mailings_completed, 0);
    assert_eq!(
        mailing_sent_date(&db.pool, mailing_id).await,
        sent_date,
        "sent_date stamps once, never re-stamps"
    );
    db.dispose().await;
}

#[tokio::test]
async fn restart_and_concurrent_pumps_complete_the_mailing_exactly_once() {
    let Some(db) = TestDb::new("pumprace").await else {
        return skipped("pumprace");
    };
    let webhook = SmsStatusWebhookService::new(db.pool.clone(), SECRET);

    let mailing_id = seed_sms_mailing(&db.pool, "race").await;
    mark_walk(&db.pool, mailing_id).await;
    let sms_uuid = enqueue_real(&db.pool, "+628110000301", "race").await;
    let _trace = seed_sms_trace(&db.pool, mailing_id, "racer@example.id", &sms_uuid).await;

    // The last delivery verdict lands BEFORE any pump runs — the race under
    // proof is the completion inference itself: many pumps, one completion.
    deliver_fully(&db.pool, &webhook, &sms_uuid).await;

    // A FRESH service instance (the restart shape: nothing in memory) plus
    // a fleet of concurrent passes — exactly as overlapping sweeps would.
    let pool = db.pool.clone();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let pool = pool.clone();
        tasks.spawn(async move {
            SmsDeliveryPumpService::new(pool)
                .pump_once()
                .await
                .expect("concurrent pump")
        });
    }
    let mut completions = 0usize;
    while let Some(res) = tasks.join_next().await {
        completions += res.expect("joined pump").mailings_completed;
    }
    assert_eq!(
        completions, 1,
        "the FOR UPDATE lock serializes the racers; the second locker's re-check no-ops"
    );
    assert_eq!(mailing_state(&db.pool, mailing_id).await, "done");
    let sent_date = mailing_sent_date(&db.pool, mailing_id).await;
    assert!(sent_date.is_some(), "the one completion stamped sent_date");

    // The restart replay: another fresh instance over the same durable
    // facts re-reads completion and re-derives nothing.
    let out = SmsDeliveryPumpService::new(db.pool.clone())
        .pump_once()
        .await
        .expect("restart replay");
    assert_eq!(out.mailings_completed, 0);
    assert_eq!(out.candidates, 0);
    assert_eq!(
        mailing_sent_date(&db.pool, mailing_id).await,
        sent_date,
        "restart-durable: microsecond-identical sent_date"
    );
    db.dispose().await;
}

#[tokio::test]
async fn premature_done_is_refused_without_the_marker_with_remaining_traces_or_parked() {
    let Some(db) = TestDb::new("pumpguard").await else {
        return skipped("pumpguard");
    };
    let pump = SmsDeliveryPumpService::new(db.pool.clone());

    // (a) Walk NOT ended (no marker): every trace terminal — still sending.
    let unmarked = seed_sms_mailing(&db.pool, "unmarked").await;
    let uuid_a = enqueue_real(&db.pool, "+628110000401", "guard a").await;
    let trace_a = seed_sms_trace(&db.pool, unmarked, "ga@example.id", &uuid_a).await;
    {
        let webhook = SmsStatusWebhookService::new(db.pool.clone(), SECRET);
        deliver_fully(&db.pool, &webhook, &uuid_a).await;
    }
    // The trace advanced but the mailing is NOT a candidate: pump pass 1
    // advances the trace, and no completion may fire for it.
    let out = pump.pump_once().await.expect("guard pass");
    assert!(out.traces_advanced >= 1);
    assert_eq!(trace_status(&db.pool, trace_a).await, "sent");
    assert_eq!(out.mailings_completed, 0, "no marker, no inference");
    assert_eq!(mailing_state(&db.pool, unmarked).await, "sending");

    // (b) Walk ended, but one trace is still transient: still sending.
    let partial = seed_sms_mailing(&db.pool, "partial").await;
    mark_walk(&db.pool, partial).await;
    let uuid_b1 = enqueue_real(&db.pool, "+628110000402", "guard b1").await;
    let uuid_b2 = enqueue_real(&db.pool, "+628110000403", "guard b2").await;
    let _t_b1 = seed_sms_trace(&db.pool, partial, "gb1@example.id", &uuid_b1).await;
    let _t_b2 = seed_sms_trace(&db.pool, partial, "gb2@example.id", &uuid_b2).await;
    {
        let webhook = SmsStatusWebhookService::new(db.pool.clone(), SECRET);
        deliver_fully(&db.pool, &webhook, &uuid_b1).await;
        // uuid_b2's tracker stays 'process' — a transient trace remains.
    }
    let out = pump.pump_once().await.expect("partial pass");
    assert_eq!(
        out.mailings_completed, 0,
        "a transient trace holds the mailing open"
    );
    assert_eq!(mailing_state(&db.pool, partial).await, "sending");

    // (c) Walk ended and parked on a send error: excluded — a human owns it.
    let parked = seed_sms_mailing(&db.pool, "parked").await;
    mark_walk(&db.pool, parked).await;
    let uuid_c = enqueue_real(&db.pool, "+628110000404", "guard c").await;
    let _t_c = seed_sms_trace(&db.pool, parked, "gc@example.id", &uuid_c).await;
    {
        let webhook = SmsStatusWebhookService::new(db.pool.clone(), SECRET);
        deliver_fully(&db.pool, &webhook, &uuid_c).await;
        let mut tx = db.pool.begin().await.expect("tx");
        MailingSendRepository::park_mailing(&mut tx, parked, "domain parse failed at send time")
            .await
            .expect("park");
        tx.commit().await.expect("commit");
    }
    let out = pump.pump_once().await.expect("parked pass");
    assert_eq!(
        out.mailings_completed, 0,
        "parked mailings are never auto-closed"
    );
    assert_eq!(mailing_state(&db.pool, parked).await, "sending");
    db.dispose().await;
}

#[tokio::test]
async fn the_send_sweep_rides_the_pump_and_the_channel_guard_holds() {
    let Some(db) = TestDb::new("pumpwire").await else {
        return skipped("pumpwire");
    };
    let engine = MailingWriteService::new(db.pool.clone());

    // A walked-out sms mailing whose single tracker is already terminal:
    // the sweep must claim it, drive NOTHING (the channel guard leaves
    // walked-out rows untouched), and close it through its pump step.
    let walked = seed_sms_mailing(&db.pool, "wired").await;
    mark_walk(&db.pool, walked).await;
    let uuid_w = enqueue_real(&db.pool, "+628110000501", "wired").await;
    let _t_w = seed_sms_trace(&db.pool, walked, "w@example.id", &uuid_w).await;
    {
        let webhook = SmsStatusWebhookService::new(db.pool.clone(), SECRET);
        deliver_fully(&db.pool, &webhook, &uuid_w).await;
    }
    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(
        out.claimed, 1,
        "the walked-out mailing is claimable send work"
    );
    assert_eq!(out.enqueued, 0, "the mail walk never touched it");
    assert_eq!(
        out.sms_traces_advanced, 1,
        "the sweep's pump step advanced the trace"
    );
    assert_eq!(
        out.sms_mailings_completed, 1,
        "the sweep's pump step closed the mailing"
    );
    assert_eq!(mailing_state(&db.pool, walked).await, "done");

    // An sms mailing whose walk has NOT ended: the mail walk refuses to
    // route an sms audience through the email queue — it parks loudly.
    let unguarded = seed_sms_mailing(&db.pool, "unguarded").await;
    let out = engine.send_queue_sweep().await.expect("guard sweep");
    assert_eq!(out.parked, 1);
    assert_eq!(
        out.enqueued, 0,
        "no email may ever leave for an sms audience"
    );
    let (state, parked_reason): (String, Option<String>) = sqlx::query_as(
        "SELECT state::text, metadata->>'send_error' FROM mailing.mailings WHERE id = $1",
    )
    .bind(unguarded)
    .fetch_one(&db.pool)
    .await
    .expect("parked row");
    assert_eq!(state, "sending");
    assert!(
        parked_reason.is_some(),
        "the park reason is visible in metadata"
    );
    db.dispose().await;
}

#[tokio::test]
async fn tracker_verdicts_map_to_the_channel_pure_failure_vocabulary() {
    let Some(db) = TestDb::new("pumpmap").await else {
        return skipped("pumpmap");
    };
    let pump = SmsDeliveryPumpService::new(db.pool.clone());

    // Trackers are seeded directly here: this case proves the MAPPING (the
    // port of the channel-state table), not the substrate's verdict path —
    // the e2e case above already drives the real webhook seam.
    async fn seed_tracker(
        pool: &sqlx::PgPool,
        sms_uuid: &str,
        state: &str,
        failure_type: Option<&str>,
        failure_reason: Option<&str>,
    ) {
        sqlx::query(
            r#"INSERT INTO messaging.sms_trackers
                   (sms_uuid, state, failure_type, failure_reason)
               VALUES ($1, $2::mail_notification_status,
                       $3::notification_failure_type, $4)"#,
        )
        .bind(sms_uuid)
        .bind(state)
        .bind(failure_type)
        .bind(failure_reason)
        .execute(pool)
        .await
        .expect("tracker seed");
    }

    let mailing_id = seed_sms_mailing(&db.pool, "mapping").await;
    mark_walk(&db.pool, mailing_id).await;

    // exception + bounce-class code → trace BOUNCE, code verbatim.
    let uuid_inv = format!("map-inv-{}", Uuid::new_v4().simple());
    seed_tracker(
        &db.pool,
        &uuid_inv,
        "exception",
        Some("sms_invalid_destination"),
        Some("dead number"),
    )
    .await;
    let t_inv = seed_sms_trace(&db.pool, mailing_id, "inv@example.id", &uuid_inv).await;

    // exception + transport code → trace ERROR, code verbatim.
    let uuid_credit = format!("map-credit-{}", Uuid::new_v4().simple());
    seed_tracker(
        &db.pool,
        &uuid_credit,
        "exception",
        Some("sms_credit"),
        None,
    )
    .await;
    let t_credit = seed_sms_trace(&db.pool, mailing_id, "credit@example.id", &uuid_credit).await;

    // exception + a code from the MAIL vocabulary → trace ERROR with the
    // channel default, never a cast failure and never a mail code.
    let uuid_cross = format!("map-cross-{}", Uuid::new_v4().simple());
    seed_tracker(&db.pool, &uuid_cross, "exception", Some("mail_smtp"), None).await;
    let t_cross = seed_sms_trace(&db.pool, mailing_id, "cross@example.id", &uuid_cross).await;

    // exception + no code at all → the channel default.
    let uuid_bare = format!("map-bare-{}", Uuid::new_v4().simple());
    seed_tracker(&db.pool, &uuid_bare, "exception", None, None).await;
    let t_bare = seed_sms_trace(&db.pool, mailing_id, "bare@example.id", &uuid_bare).await;

    // bounce state → trace BOUNCE (the state itself carries the class).
    let uuid_rej = format!("map-rej-{}", Uuid::new_v4().simple());
    seed_tracker(&db.pool, &uuid_rej, "bounce", Some("sms_rejected"), None).await;
    let t_rej = seed_sms_trace(&db.pool, mailing_id, "rej@example.id", &uuid_rej).await;

    // canceled → trace CANCEL from outgoing, channel default when no code.
    let uuid_cx = format!("map-cx-{}", Uuid::new_v4().simple());
    seed_tracker(&db.pool, &uuid_cx, "canceled", None, None).await;
    let t_cx = seed_sms_trace(&db.pool, mailing_id, "cx@example.id", &uuid_cx).await;

    // A 'ready' tracker is pre-dispatch: the trace must stay outgoing.
    let uuid_ready = format!("map-ready-{}", Uuid::new_v4().simple());
    seed_tracker(&db.pool, &uuid_ready, "ready", None, None).await;
    let t_ready = seed_sms_trace(&db.pool, mailing_id, "ready@example.id", &uuid_ready).await;

    let out = pump.pump_once().await.expect("mapping pass");
    assert_eq!(
        out.traces_advanced, 6,
        "every verdict except ready advances"
    );
    // The ready trace is pre-dispatch and stays outgoing — a TRANSIENT
    // trace by definition, so the remaining-check must hold the mailing
    // open even though every other verdict landed terminal. (The same
    // guard as the partial-delivery case, seen from the mapping side.)
    assert_eq!(
        out.mailings_completed, 0,
        "the ready trace holds the mailing open"
    );
    assert_eq!(mailing_state(&db.pool, mailing_id).await, "sending");

    let row = |trace: Uuid| {
        let pool = db.pool.clone();
        async move {
            let (status, ftype): (String, Option<String>) =
                sqlx::query_as("SELECT trace_status::text, failure_type::text FROM mailing.mailing_traces WHERE id = $1")
                    .bind(trace)
                    .fetch_one(&pool)
                    .await
                    .expect("mapped row");
            (status, ftype)
        }
    };
    assert_eq!(
        row(t_inv).await,
        ("bounce".into(), Some("sms_invalid_destination".into()))
    );
    assert_eq!(
        row(t_credit).await,
        ("error".into(), Some("sms_credit".into()))
    );
    assert_eq!(
        row(t_cross).await,
        ("error".into(), Some("sms_server".into()))
    );
    assert_eq!(
        row(t_bare).await,
        ("error".into(), Some("sms_server".into()))
    );
    assert_eq!(
        row(t_rej).await,
        ("bounce".into(), Some("sms_rejected".into()))
    );
    assert_eq!(
        row(t_cx).await,
        ("cancel".into(), Some("sms_blacklist".into()))
    );
    assert_eq!(trace_status(&db.pool, t_ready).await, "outgoing");
    db.dispose().await;
}
