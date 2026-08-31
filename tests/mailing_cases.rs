//! Mailing lifecycle + send-engine behavior cases (fresh-DB, disposable
//! scratch per test).

#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use backbone_mailing::application::service::mailing_write_service::{
    DomainInvalid, MailingSendConfig, MailingUpsertCommand, MailingWriteError,
    MailingWriteService, PartyRecipientResolver, SweepOutcome,
};
use backbone_mailing::infrastructure::persistence::mailing_send_repository::{
    CompiledDomain, ResolvedRecipient,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn domain_cmd(audience_id: Uuid, subject: &str) -> MailingUpsertCommand {
    MailingUpsertCommand {
        subject: subject.into(),
        preview: None,
        body_html: "<p>Hello {{ name }}</p>".into(),
        email_from: "campaign@example.id".into(),
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

/// An event sink that records suppressions (assert the observable, not the
/// log line).
#[derive(Default)]
struct RecordingSink {
    suppressed: std::sync::Mutex<Vec<(Uuid, String, String)>>,
    launched: AtomicUsize,
}

impl backbone_mailing::application::service::mailing_write_service::MailingEventSink
    for RecordingSink
{
    fn mailing_launched(
        &self,
        _mailing_id: Uuid,
        _campaign_id: Option<Uuid>,
        _schedule_type: &str,
    ) {
        self.launched.fetch_add(1, Ordering::SeqCst);
    }
    fn recipient_suppressed(&self, _m: Uuid, _t: Uuid, email: &str, cause: &str) {
        self.suppressed
            .lock()
            .unwrap()
            .push((_m, email.to_string(), cause.to_string()));
    }
    fn auto_blacklist_added(&self, _email: &str) {}
    fn ab_test_winner_promoted(&self, _a: Uuid, _w: Uuid) {}
}

#[tokio::test]
async fn create_mailing_stores_the_canonical_domain_form() {
    let Some(db) = TestDb::new("create").await else {
        return skipped("create");
    };
    let (audience_id, _) = seed_audience(&db.pool, "canon", 1).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let cmd = domain_cmd(audience_id, "Canonical form");
    let id = svc.create_mailing(&cmd).await.expect("create");

    let stored: serde_json::Value = sqlx::query_scalar(
        r#"SELECT mailing_domain FROM mailing.mailings WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("stored domain");
    // Canonical object form, not the Odoo triple form it arrived in.
    assert_eq!(
        stored,
        serde_json::json!([
            {"field": "mailing_audience_id", "op": "=", "value": audience_id.to_string()}
        ])
    );
    let state: String = sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .expect("state");
    assert_eq!(state, "draft");
    db.dispose().await;
}

#[tokio::test]
async fn create_mailing_refuses_an_invalid_domain_loudly() {
    let Some(db) = TestDb::new("refuse").await else {
        return skipped("refuse");
    };
    let (audience_id, _) = seed_audience(&db.pool, "refuse", 1).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let mut cmd = domain_cmd(audience_id, "Refused");
    // Non-uuid audience value: refuse-loudly, before anything is stored.
    cmd.mailing_domain_raw = serde_json::json!([["mailing_audience_id", "=", "not-a-uuid"]]);
    let err = svc.create_mailing(&cmd).await.expect_err("must refuse");
    match &err {
        MailingWriteError::Domain(DomainInvalid::BadAudienceUuid { .. }) => {}
        other => panic!("expected BadAudienceUuid, got {other:?}"),
    }
    assert_eq!(err.http_status(), 422);
    assert_eq!(err.code(), "domain_invalid");
    let rows = count(&db.pool, "SELECT count(*) FROM mailing.mailings").await;
    assert_eq!(rows, 0, "a refused create stores nothing");
    db.dispose().await;
}

#[tokio::test]
async fn empty_audience_completes_empty_with_sent_date() {
    let Some(db) = TestDb::new("empty").await else {
        return skipped("empty");
    };
    let (audience_id, _) = seed_audience(&db.pool, "empty", 0).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Empty audience"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 1);
    assert_eq!(out.completed_empty, 1);
    assert_eq!(out.completed, 0);
    let (state, sent_date): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        r#"SELECT state::text, sent_date FROM mailing.mailings WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("mailing row");
    assert_eq!(state, "done");
    assert!(sent_date.is_some(), "complete_empty still stamps sent_date");
    let traces: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1",
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("trace count");
    assert_eq!(traces, 0, "the complete-empty edge mints no traces");
    db.dispose().await;
}

#[tokio::test]
async fn sweep_sends_mints_traces_and_completes() {
    let Some(db) = TestDb::new("sends").await else {
        return skipped("sends");
    };
    let (audience_id, emails) = seed_audience(&db.pool, "sends", 3).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Plain send"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 1);
    assert_eq!(out.recipients_resolved, 3);
    assert_eq!(out.minted, 3);
    assert_eq!(out.enqueued, 3);
    assert_eq!(out.completed, 1);

    let state: String =
        sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("state");
    assert_eq!(state, "done");

    // Exactly one live outgoing-or-beyond trace per recipient…
    for email in &emails {
        let n: i64 = sqlx::query_scalar(
            r#"SELECT count(*) FROM mailing.mailing_traces
               WHERE mailing_id = $1 AND lower(recipient_email) = lower($2)"#,
        )
        .bind(id)
        .bind(email)
        .fetch_one(&db.pool)
        .await
        .expect("trace probe");
        assert_eq!(n, 1, "one trace per recipient ({email})");
    }
    // …and exactly one queue row per recipient through the mail seam.
    let mails: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM messaging.mails m
           JOIN mailing.mailing_traces t ON t.mail_id = m.id
           WHERE t.mailing_id = $1"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("mail probe");
    assert_eq!(mails, 3);
    let _ = SweepOutcome::default();
    db.dispose().await;
}

#[tokio::test]
async fn suppression_is_visible_never_silent() {
    let Some(db) = TestDb::new("suppr").await else {
        return skipped("suppr");
    };
    let (audience_id, emails) = seed_audience(&db.pool, "suppr", 3).await;
    // Blacklist the first member on the mail module's exclusion list.
    sqlx::query(r#"INSERT INTO messaging.mail_blacklists (email) VALUES ($1)"#)
        .bind(&emails[0])
        .execute(&db.pool)
        .await
        .expect("blacklist");
    // Opt the second member out of an UNRELATED list (OPT-OUT-WINS: the
    // member's subscription to the TARGETED audience stays clean, so they
    // resolve — and the cross-list opt-out probe suppresses them visibly.
    // Opting out of the targeted list itself would exclude them at
    // resolution, which is the quieter filter-by-membership path.)
    {
        let other_list = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO mailing.mailing_audiences (id, name) VALUES ($1, 'Elsewhere')"#,
        )
        .bind(other_list)
        .execute(&db.pool)
        .await
        .expect("other audience");
        sqlx::query(
            r#"INSERT INTO mailing.mailing_subscriptions (contact_id, mailing_audience_id)
               VALUES ((SELECT id FROM mailing.mailing_contacts WHERE email = $1), $2)"#,
        )
        .bind(&emails[1])
        .bind(other_list)
        .execute(&db.pool)
        .await
        .expect("other subscription");
        sqlx::query(
            r#"UPDATE mailing.mailing_subscriptions SET opt_out = TRUE, opt_out_datetime = now()
               WHERE contact_id = (SELECT id FROM mailing.mailing_contacts WHERE email = $1)
                 AND mailing_audience_id = $2"#,
        )
        .bind(&emails[1])
        .bind(other_list)
        .execute(&db.pool)
        .await
        .expect("opt out");
    }
    // The contacts table enforces one row per email, so the duplicate-email
    // collapse is exercised on the PARTY seam, where a host resolver may
    // legitimately return the same address twice (two parties sharing an
    // inbox).
    struct DupResolver;
    #[async_trait::async_trait]
    impl PartyRecipientResolver for DupResolver {
        async fn resolve(
            &self,
            _domain: &CompiledDomain,
        ) -> Result<Vec<ResolvedRecipient>, String> {
            Ok(vec![
                ResolvedRecipient {
                    recipient_id: Uuid::new_v4(),
                    email: "shared@party.example.id".into(),
                },
                ResolvedRecipient {
                    recipient_id: Uuid::new_v4(),
                    email: "SHARED@party.example.id".into(),
                },
                ResolvedRecipient {
                    recipient_id: Uuid::new_v4(),
                    email: "solo@party.example.id".into(),
                },
            ])
        }
    }

    let sink = Arc::new(RecordingSink::default());
    let svc = MailingWriteService::new(db.pool.clone()).with_event_sink(sink.clone());
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Suppression"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    let out = svc.send_queue_sweep().await.expect("sweep");

    assert_eq!(out.suppressed_blacklist, 1);
    assert_eq!(out.suppressed_optout, 1);
    assert_eq!(out.minted, 1, "only the clean member sends");

    // The party-seam duplicate collapse: one visible mail_dup cancel, the
    // case-insensitive first copy sends, the solo address sends.
    let party_cmd = MailingUpsertCommand {
        subject: "Party dupes".into(),
        preview: None,
        body_html: "<p>x</p>".into(),
        email_from: "p@example.id".into(),
        reply_to: None,
        mailing_domain_raw: serde_json::json!([]),
        target_model: "party".into(),
        schedule_type: "immediate".into(),
        schedule_date: None,
        use_exclusion_list: true,
        campaign_id: None,
        source_id: None,
        ab_test_id: None,
        ab_testing_enabled: false,
        ab_testing_pc: 0,
    };
    let party_svc = MailingWriteService::new(db.pool.clone())
        .with_event_sink(sink.clone())
        .with_party_resolver(Arc::new(DupResolver));
    let party = party_svc
        .create_mailing(&party_cmd)
        .await
        .expect("party create");
    party_svc
        .launch(party, "immediate", None)
        .await
        .expect("launch");
    let out = party_svc.send_queue_sweep().await.expect("party sweep");
    assert_eq!(out.suppressed_dup, 1, "the case-insensitive dup collapses");
    assert_eq!(
        out.minted, 2,
        "the first copy + the solo address send; the dup does not"
    );

    // Cancel traces carry the matching failure types — visible, paired 1:1
    // with the suppression events.
    let cancels: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT lower(recipient_email) AS email, failure_type::text AS ftype
           FROM mailing.mailing_traces
           WHERE mailing_id = $1 AND trace_status = 'cancel'
           ORDER BY email"#,
    )
    .bind(id)
    .fetch_all(&db.pool)
    .await
    .expect("cancel probes");
    let mut by_email: std::collections::HashMap<String, String> =
        cancels.into_iter().collect();
    assert_eq!(
        by_email.remove(&emails[0].to_lowercase()).as_deref(),
        Some("mail_bl"),
        "blacklisted member carries mail_bl"
    );
    assert_eq!(
        by_email.remove(&emails[1].to_lowercase()).as_deref(),
        Some("mail_optout"),
        "opted-out member carries mail_optout"
    );
    let (dup_email, dup_ftype): (String, String) = sqlx::query_as(
        r#"SELECT lower(recipient_email), failure_type::text
           FROM mailing.mailing_traces
           WHERE mailing_id = $1 AND trace_status = 'cancel'"#,
    )
    .bind(party)
    .fetch_one(&db.pool)
    .await
    .expect("party cancel");
    assert_eq!((dup_email.as_str(), dup_ftype.as_str()), ("shared@party.example.id", "mail_dup"));

    let recorded = sink.suppressed.lock().unwrap().clone();
    assert_eq!(recorded.len(), 3, "one event per suppression");
    let on_contact = recorded.iter().filter(|(m, _, _)| *m == id).count();
    let on_party = recorded.iter().filter(|(m, _, _)| *m == party).count();
    assert_eq!((on_contact, on_party), (2, 1));
    db.dispose().await;
}

#[tokio::test]
async fn schedule_gate_holds_the_mailing_back_until_due() {
    let Some(db) = TestDb::new("sched").await else {
        return skipped("sched");
    };
    let (audience_id, _) = seed_audience(&db.pool, "sched", 2).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Scheduled"))
        .await
        .expect("create");
    let future = chrono::Utc::now() + chrono::Duration::hours(2);
    svc.launch(id, "scheduled", Some(future))
        .await
        .expect("launch");
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 0, "a future schedule_date is not due");

    // Backdate the schedule: the claim's predicate releases the row.
    sqlx::query("UPDATE mailing.mailings SET schedule_date = now() - interval '1 minute' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("backdate");
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 1);
    assert_eq!(out.completed, 1);
    db.dispose().await;
}

#[tokio::test]
async fn pass_budget_stays_sending_and_the_next_sweep_resumes() {
    let Some(db) = TestDb::new("budget").await else {
        return skipped("budget");
    };
    let (audience_id, _) = seed_audience(&db.pool, "budget", 5).await;
    let svc = MailingWriteService::new(db.pool.clone()).with_send_config(MailingSendConfig {
        recipients_per_pass: 2,
        mail_enqueue_batch: 2,
        ..Default::default()
    });
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Capped pass"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");

    let out1 = svc.send_queue_sweep().await.expect("sweep 1");
    assert_eq!(out1.minted, 2, "the pass budget caps new enqueues");
    assert_eq!(out1.still_sending, 1, "the mailing stays sending");
    let state: String =
        sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("state");
    assert_eq!(state, "sending");

    let out2 = svc.send_queue_sweep().await.expect("sweep 2");
    assert_eq!(out2.minted, 2, "the resume pass is ALSO budget-capped");
    assert_eq!(out2.still_sending, 1);
    assert_eq!(out2.skipped_seen, 2, "earlier sends are seen-list skipped");
    let out3 = svc.send_queue_sweep().await.expect("sweep 3");
    assert_eq!(out3.minted, 1, "the final pass drives the last member");
    assert_eq!(out3.completed, 1);
    assert_eq!(out3.skipped_seen, 4);

    // No duplicate mails across passes — the fence + seen probe contain it.
    let distinct_mails: i64 = sqlx::query_scalar(
        r#"SELECT count(DISTINCT t.mail_id) FROM mailing.mailing_traces t WHERE t.mailing_id = $1"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("distinct mails");
    let traces: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1",
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("traces");
    assert_eq!(distinct_mails, 5);
    assert_eq!(traces, 5);
    db.dispose().await;
}

#[tokio::test]
async fn unparseable_domain_at_send_parks_with_zero_sends() {
    let Some(db) = TestDb::new("park").await else {
        return skipped("park");
    };
    let (audience_id, emails) = seed_audience(&db.pool, "park", 3).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Will be corrupted"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    // Corrupt the stored domain AFTER the write-time parse (the send-time
    // re-parse is the refuse-loudly backstop).
    sqlx::query(r#"UPDATE mailing.mailings SET mailing_domain = '["email","BROKEN","x"]'::jsonb WHERE id = $1"#)
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("corrupt");

    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.parked, 1);
    assert_eq!(out.minted, 0, "a parked mailing sends nothing");
    let (state, send_error): (String, Option<String>) = sqlx::query_as(
        r#"SELECT state::text, metadata->>'send_error' FROM mailing.mailings WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("park probe");
    assert_eq!(state, "sending");
    assert!(send_error.is_some(), "the park reason is recorded");
    let mails: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.mails")
            .fetch_one(&db.pool)
            .await
            .expect("mails");
    assert_eq!(mails, 0, "no queue rows were minted");
    let _ = emails;

    // Fix the domain, requeue, and the mailing sends.
    sqlx::query(
        r#"UPDATE mailing.mailings SET mailing_domain = $2::jsonb WHERE id = $1"#,
    )
    .bind(id)
    .bind(serde_json::json!([["mailing_audience_id", "=", audience_id.to_string()]]))
    .execute(&db.pool)
    .await
    .expect("fix");
    svc.requeue_after_domain_fix(id).await.expect("requeue");
    let out = svc.send_queue_sweep().await.expect("sweep after fix");
    assert_eq!(out.minted, 3);
    assert_eq!(out.completed, 1);
    db.dispose().await;
}

#[tokio::test]
async fn auto_blacklist_uses_the_db_clock_and_spread_guard() {
    let Some(db) = TestDb::new("autobl").await else {
        return skipped("autobl");
    };
    // Five bounce traces spread over 30 days — a persistent pattern. The
    // audit trigger stamps created_at UNCONDITIONALLY on INSERT, so the
    // backdate rides an UPDATE (which only touches updated_at).
    for i in 0..5 {
        let trace_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO mailing.mailing_traces
                   (id, mailing_id, recipient_model, recipient_id, recipient_email,
                    trace_status, failure_type)
               VALUES ($1, $2, 'mailing_contact', $3, 'bouncer@example.id',
                       'bounce', 'mail_bounce')"#,
        )
        .bind(trace_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&db.pool)
        .await
        .expect("bounce seed");
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET metadata = jsonb_build_object('created_at',
                   to_jsonb(now() - make_interval(days => $2)))
               WHERE id = $1"#,
        )
        .bind(trace_id)
        .bind(30 - i * 7)
        .execute(&db.pool)
        .await
        .expect("backdate bounce");
    }
    // One incidental bounce for another address — never blacklists.
    sqlx::query(
        r#"INSERT INTO mailing.mailing_traces
               (id, mailing_id, recipient_model, recipient_id, recipient_email,
                trace_status, failure_type)
           VALUES ($1, $2, 'mailing_contact', $3, 'once@example.id', 'bounce', 'mail_bounce')"#,
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .execute(&db.pool)
    .await
    .expect("single bounce");

    let svc = MailingWriteService::new(db.pool.clone());
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.auto_blacklisted, 1, "only the persistent bouncer lands");
    let bl: Option<String> = sqlx::query_scalar(
        "SELECT email FROM messaging.mail_blacklists WHERE lower(email) = 'bouncer@example.id'",
    )
    .fetch_one(&db.pool)
    .await
    .ok();
    assert_eq!(bl.as_deref(), Some("bouncer@example.id"));

    // Idempotent: a second sweep adds nothing new.
    let out = svc.send_queue_sweep().await.expect("sweep 2");
    assert_eq!(out.auto_blacklisted, 0);
    db.dispose().await;
}

#[tokio::test]
async fn retry_failed_soft_deletes_and_requeues() {
    let Some(db) = TestDb::new("retry").await else {
        return skipped("retry");
    };
    let (audience_id, emails) = seed_audience(&db.pool, "retry", 2).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc
        .create_mailing(&domain_cmd(audience_id, "Retry path"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    svc.send_queue_sweep().await.expect("sweep");
    // Flip both traces to error (simulated transport failures).
    sqlx::query(
        r#"UPDATE mailing.mailing_traces
           SET trace_status = 'error', failure_type = 'mail_smtp'
           WHERE mailing_id = $1"#,
    )
    .bind(id)
    .execute(&db.pool)
    .await
    .expect("error flip");

    let removed = svc.retry_failed(id).await.expect("retry");
    assert_eq!(removed, 2, "both failed traces soft-deleted");
    let state: String =
        sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("state");
    assert_eq!(state, "in_queue", "retry_failed exits done into in_queue");

    // The re-send mints fresh live traces for the same recipients.
    let out = svc.send_queue_sweep().await.expect("re-sweep");
    assert_eq!(out.minted, 2);
    assert_eq!(out.completed, 1);
    let live: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM mailing.mailing_traces
           WHERE mailing_id = $1 AND (metadata->>'deleted_at') IS NULL"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("live traces");
    assert_eq!(live, 2);
    let _ = emails;

    // Zero failed traces is a typed refusal, never a silent no-op.
    let err = svc.retry_failed(id).await.expect_err("nothing to retry");
    assert_eq!(err.http_status(), 422);
    db.dispose().await;
}

/// Seed one sms-type draft mailing directly. `body` is passed verbatim so a
/// case can hand the verb a blank body (the DB CHECK only rejects NULL, so
/// the blank case proves the TYPED refusal, not the backstop).
async fn seed_sms_draft(pool: &sqlx::PgPool, marker: &str, body: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mailing.mailings
               (id, subject, body_html, body_plaintext, email_from, state,
                mailing_type, mailing_domain)
           VALUES ($1, $2, '<p>x</p>', $3, 'c@example.id', 'draft',
                   'sms', '[]'::jsonb)"#,
    )
    .bind(id)
    .bind(marker)
    .bind(body)
    .execute(pool)
    .await
    .expect("sms draft seed");
    id
}

#[tokio::test]
async fn launch_refuses_a_blank_bodied_sms_mailing_with_the_typed_error() {
    let Some(db) = TestDb::new("sms-blank-launch").await else {
        return skipped("sms-blank-launch");
    };
    for blank in ["", "   "] {
        let id = seed_sms_draft(&db.pool, "blank-body-sms", blank).await;
        let svc = MailingWriteService::new(db.pool.clone());
        let err = svc.launch(id, "immediate", None).await.expect_err("must refuse");
        match &err {
            MailingWriteError::Invalid(msg) => {
                assert!(msg.contains("body_plaintext"), "message names the missing body: {msg}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        assert_eq!(err.http_status(), 422);
        // Zero row effects: the refusal lands before the queue edge.
        let state: String =
            sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
                .bind(id)
                .fetch_one(&db.pool)
                .await
                .expect("state");
        assert_eq!(state, "draft", "a refused launch leaves the draft untouched");
    }
    db.dispose().await;
}

#[tokio::test]
async fn launch_queues_a_bodied_sms_mailing() {
    let Some(db) = TestDb::new("sms-body-launch").await else {
        return skipped("sms-body-launch");
    };
    let id = seed_sms_draft(&db.pool, "bodied-sms", "flash sale today").await;
    let svc = MailingWriteService::new(db.pool.clone());
    svc.launch(id, "immediate", None).await.expect("launch");
    let state: String =
        sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("state");
    assert_eq!(state, "in_queue");
    db.dispose().await;
}
