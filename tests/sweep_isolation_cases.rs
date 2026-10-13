//! The send sweep under failure and under a composer's scope, the retry verb's refusal and
//! reach, the public opt-out across case-variant contacts, and the A/B percentage bound.

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
#[path = "behavior/common/mod.rs"]
mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use backbone_mailing::application::service::mailing_write_service::{
    MailingUpsertCommand, MailingWriteError, MailingWriteService, TargetRecipientResolver,
};
use backbone_mailing::application::service::subscription_write_service::SubscriptionWriteService;
use backbone_mailing::application::service::{MailingRunScope, ScopedWork};
use backbone_mailing::infrastructure::persistence::mailing_send_repository::{
    CompiledDomain, ResolvedRecipient,
};
use common::*;
use uuid::Uuid;

fn cmd(target_model: &str, domain: serde_json::Value, subject: &str) -> MailingUpsertCommand {
    MailingUpsertCommand {
        subject: subject.into(),
        preview: None,
        body_html: "<p>Hello</p>".into(),
        email_from: "campaign@example.id".into(),
        reply_to: None,
        mailing_domain_raw: domain,
        target_model: target_model.into(),
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

fn audience_domain(audience_id: Uuid) -> serde_json::Value {
    serde_json::json!([["mailing_audience_id", "=", audience_id.to_string()]])
}

/// A resolver whose target is down.
struct FailingLeads;

#[async_trait]
impl TargetRecipientResolver for FailingLeads {
    fn target_model(&self) -> &'static str {
        "crm_lead"
    }
    async fn resolve(&self, _domain: &CompiledDomain) -> Result<Vec<ResolvedRecipient>, String> {
        Err("lead store unreachable".into())
    }
}

/// A scope seam that records which mailings it was asked to scope.
#[derive(Default)]
struct RecordingScope {
    seen: Mutex<Vec<Uuid>>,
}

#[async_trait]
impl MailingRunScope for RecordingScope {
    async fn within<'a>(&'a self, mailing_id: Uuid, work: ScopedWork<'a>) -> Result<(), String> {
        self.seen.lock().expect("lock").push(mailing_id);
        work.await;
        Ok(())
    }
}

async fn state_of(pool: &sqlx::PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state::text FROM mailing.mailings WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("state")
}

/// A mailing whose recipients cannot be resolved is parked with the reason and the sweep
/// goes on to the next one; a parked mailing is not claimed again until it is requeued.
#[tokio::test]
async fn one_failing_mailing_does_not_hold_back_the_sweep() {
    let Some(db) = TestDb::new("iso").await else {
        return skipped("iso");
    };
    let (audience_id, _) = seed_audience(&db.pool, "iso", 2).await;
    let svc = MailingWriteService::new(db.pool.clone()).with_target_resolver(Arc::new(FailingLeads));
    let broken = svc.create_mailing(&cmd("crm_lead", serde_json::json!([]), "Broken")).await.expect("create");
    svc.launch(broken, "immediate", None).await.expect("launch");
    let fine = svc.create_mailing(&cmd("mailing_contact", audience_domain(audience_id), "Fine")).await.expect("create");
    svc.launch(fine, "immediate", None).await.expect("launch");

    svc.send_queue_sweep().await.expect("the sweep runs past the broken mailing");
    assert_eq!(state_of(&db.pool, fine).await, "done", "the later mailing was sent");
    let reason: Option<String> =
        sqlx::query_scalar("SELECT metadata->>'send_error' FROM mailing.mailings WHERE id = $1")
            .bind(broken)
            .fetch_one(&db.pool)
            .await
            .expect("reason");
    assert!(reason.unwrap_or_default().contains("lead store unreachable"), "the reason is on the row");

    let again = svc.send_queue_sweep().await.expect("second sweep");
    assert_eq!(again.claimed, 0, "a parked mailing waits for its requeue");
    db.dispose().await;
}

/// Each claimed mailing is driven through the composer's scope seam, by its own id.
#[tokio::test]
async fn each_mailing_is_driven_through_the_scope_seam() {
    let Some(db) = TestDb::new("scope").await else {
        return skipped("scope");
    };
    let (audience_id, _) = seed_audience(&db.pool, "scope", 1).await;
    let scope = Arc::new(RecordingScope::default());
    let svc = MailingWriteService::new(db.pool.clone()).with_run_scope(scope.clone());
    let a = svc.create_mailing(&cmd("mailing_contact", audience_domain(audience_id), "A")).await.expect("create");
    let b = svc.create_mailing(&cmd("mailing_contact", audience_domain(audience_id), "B")).await.expect("create");
    svc.launch(a, "immediate", None).await.expect("launch");
    svc.launch(b, "immediate", None).await.expect("launch");
    svc.send_queue_sweep().await.expect("sweep");
    let seen = scope.seen.lock().expect("lock").clone();
    assert!(seen.contains(&a) && seen.contains(&b), "both mailings ran in their scope: {seen:?}");
    db.dispose().await;
}

/// A retry on a mailing that is not done refuses with nothing touched, and a retry re-sends
/// the error traces only — a bounced address is not mailed again.
#[tokio::test]
async fn retry_refuses_untouched_and_never_resends_a_bounce() {
    let Some(db) = TestDb::new("retry2").await else {
        return skipped("retry2");
    };
    let (audience_id, _) = seed_audience(&db.pool, "retry2", 2).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc.create_mailing(&cmd("mailing_contact", audience_domain(audience_id), "Retry")).await.expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    svc.send_queue_sweep().await.expect("sweep");
    sqlx::query(
        r#"UPDATE mailing.mailing_traces t SET trace_status = CASE WHEN rn = 1 THEN 'error' ELSE 'bounce' END::trace_status,
                  failure_type = 'mail_smtp'
           FROM (SELECT id, row_number() OVER (ORDER BY id) rn FROM mailing.mailing_traces WHERE mailing_id = $1) o
           WHERE t.id = o.id"#,
    )
    .bind(id)
    .execute(&db.pool)
    .await
    .expect("one error, one bounce");
    let live = |pool: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1 AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("live traces")
    };

    sqlx::query("UPDATE mailing.mailings SET state = 'sending' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("not done");
    match svc.retry_failed(id).await {
        Err(MailingWriteError::Conflict(_)) => {}
        other => panic!("a retry of a running mailing must refuse, got {other:?}"),
    }
    assert_eq!(live(db.pool.clone()).await, 2, "a refused retry touches no trace");

    sqlx::query("UPDATE mailing.mailings SET state = 'done' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("done again");
    let removed = svc.retry_failed(id).await.expect("retry");
    assert_eq!(removed, 1, "only the error trace is retried");
    assert_eq!(live(db.pool.clone()).await, 1, "the bounce stays");
    db.dispose().await;
}

/// The public opt-out reaches every contact carrying the address, whatever its case.
#[tokio::test]
async fn opt_out_reaches_every_case_variant_contact() {
    let Some(db) = TestDb::new("optout").await else {
        return skipped("optout");
    };
    let audience_id = Uuid::new_v4();
    sqlx::query("INSERT INTO mailing.mailing_audiences (id, name) VALUES ($1, 'variants')")
        .bind(audience_id)
        .execute(&db.pool)
        .await
        .expect("audience");
    for email in ["Reader@Example.id", "reader@example.id"] {
        let contact = Uuid::new_v4();
        sqlx::query("INSERT INTO mailing.mailing_contacts (id, email, name) VALUES ($1, $2, 'Reader')")
            .bind(contact)
            .bind(email)
            .execute(&db.pool)
            .await
            .expect("contact");
        sqlx::query("INSERT INTO mailing.mailing_subscriptions (contact_id, mailing_audience_id) VALUES ($1, $2)")
            .bind(contact)
            .bind(audience_id)
            .execute(&db.pool)
            .await
            .expect("subscription");
    }
    let subs = SubscriptionWriteService::new(db.pool.clone());
    let (_, flipped) = subs.unsubscribe_by_email("READER@example.id", audience_id, None).await.expect("opt out");
    assert!(flipped);
    let still: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailing.mailing_subscriptions WHERE mailing_audience_id = $1 AND NOT opt_out",
    )
    .bind(audience_id)
    .fetch_one(&db.pool)
    .await
    .expect("still subscribed");
    assert_eq!(still, 0, "no contact with the address stays subscribed");
    db.dispose().await;
}

/// An A/B percentage outside 0..=100 is a typed refusal, not a database error.
#[tokio::test]
async fn an_out_of_range_ab_percentage_is_refused_typed() {
    let Some(db) = TestDb::new("abpc").await else {
        return skipped("abpc");
    };
    let (audience_id, _) = seed_audience(&db.pool, "abpc", 1).await;
    let svc = MailingWriteService::new(db.pool.clone());
    let mut c = cmd("mailing_contact", audience_domain(audience_id), "AB");
    c.ab_testing_pc = 150;
    let err = svc.create_mailing(&c).await.expect_err("out of range");
    assert_eq!(err.http_status(), 422, "got {err:?}");
    db.dispose().await;
}
