//! Bridge-target behavior cases (fresh-DB, disposable scratch per test):
//! the crm/sale target overlays, the per-target resolver seam, the
//! declarative default domains, the source-keyed winner-metric reads, the
//! billing-side invoiced-amount seam, and the parallel sms winner axis.

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup or assertion failure precisely")]
#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use backbone_mailing::application::service::ab_test_write_service::{
    AbTestWriteError, AbTestWriteService,
};
use backbone_mailing::application::service::mailing_stats_read_service::{
    MailingStatsReadService, SHARED_SOURCE_CAVEAT,
};
use backbone_mailing::application::service::mailing_write_service::{
    DomainInvalid, MailingUpsertCommand, MailingWriteError, MailingWriteService,
    PartyRecipientResolver, TargetRecipientResolver,
};
use backbone_mailing::application::service::sale_invoiced_amount_port::{
    SaleInvoicedAmountError, SaleInvoicedAmountPort, SourceInvoicedAmount,
};
use backbone_mailing::application::service::{
    EventRegistrationTargetError, EventRegistrationTargetResolver,
    RefusingEventRegistrationTarget,
};
use backbone_mailing::infrastructure::persistence::mailing_send_repository::{
    CompiledDomain, DomainField, DomainOp, ResolvedRecipient,
};
use std::sync::Arc;
use uuid::Uuid;

const BRIDGE_TARGETS: [&str; 3] = ["crm_lead", "crm_deal", "selling_customer"];

// ── canned seams ────────────────────────────────────────────────────────────

/// A per-target resolver double: serves exactly the target it declares,
/// answering with two distinct recipients (distinct ids AND emails — the
/// in-batch duplicate suppression would otherwise eat them).
struct CannedTarget {
    model: &'static str,
    nonce: u32,
}

impl CannedTarget {
    fn recipients(&self) -> Vec<ResolvedRecipient> {
        (0..2)
            .map(|i| ResolvedRecipient {
                recipient_id: Uuid::new_v4(),
                email: format!("{}-{}-{}@bridge.example.id", self.model, self.nonce, i),
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl TargetRecipientResolver for CannedTarget {
    fn target_model(&self) -> &'static str {
        self.model
    }

    async fn resolve(
        &self,
        _domain: &CompiledDomain,
    ) -> Result<Vec<ResolvedRecipient>, String> {
        Ok(self.recipients())
    }
}

/// The legacy party seam's double (drives the adapter regression).
struct CannedParty {
    nonce: u32,
}

#[async_trait::async_trait]
impl PartyRecipientResolver for CannedParty {
    async fn resolve(
        &self,
        _domain: &CompiledDomain,
    ) -> Result<Vec<ResolvedRecipient>, String> {
        Ok((0..2)
            .map(|i| ResolvedRecipient {
                recipient_id: Uuid::new_v4(),
                email: format!("party-{}-{}@bridge.example.id", self.nonce, i),
            })
            .collect())
    }
}

/// A per-source billing double: answers from a fixed map and records every
/// consultation (the ranking must ask once per DISTINCT source).
struct MapBackedInvoiced {
    amounts: std::collections::HashMap<Uuid, rust_decimal::Decimal>,
    consulted: std::sync::Mutex<Vec<Uuid>>,
}

#[async_trait::async_trait]
impl SaleInvoicedAmountPort for MapBackedInvoiced {
    async fn invoiced_amount_for_source(
        &self,
        source_id: Uuid,
    ) -> Result<SourceInvoicedAmount, SaleInvoicedAmountError> {
        self.consulted.lock().unwrap().push(source_id);
        match self.amounts.get(&source_id) {
            Some(amount) => Ok(SourceInvoicedAmount {
                source_id,
                amount_untaxed_total: *amount,
            }),
            None => Err(SaleInvoicedAmountError::Backend(format!(
                "no canned total for source {source_id}"
            ))),
        }
    }
}

// ── command helpers ─────────────────────────────────────────────────────────

fn bridge_cmd(target: &str, subject: &str) -> MailingUpsertCommand {
    MailingUpsertCommand {
        subject: subject.into(),
        preview: None,
        body_html: format!("<p>{subject}</p>"),
        email_from: "bridge@example.id".into(),
        reply_to: None,
        // EMPTY domain: the default-domain probe observes exactly what the
        // create path stores for this shape.
        mailing_domain_raw: serde_json::json!([]),
        target_model: target.into(),
        schedule_type: "immediate".into(),
        schedule_date: None,
        use_exclusion_list: true,
        campaign_id: Some(Uuid::new_v4()),
        source_id: None,
        ab_test_id: None,
        ab_testing_enabled: false,
        ab_testing_pc: 0,
    }
}

fn audience_variant_cmd(
    audience_id: Uuid,
    campaign_id: Uuid,
    source_id: Option<Uuid>,
    subject: &str,
) -> MailingUpsertCommand {
    MailingUpsertCommand {
        subject: subject.into(),
        preview: None,
        body_html: format!("<p>{subject}</p>"),
        email_from: "ab-bridge@example.id".into(),
        reply_to: None,
        mailing_domain_raw: serde_json::json!([
            ["mailing_audience_id", "=", audience_id.to_string()]
        ]),
        target_model: "mailing_contact".into(),
        schedule_type: "immediate".into(),
        schedule_date: None,
        use_exclusion_list: true,
        campaign_id: Some(campaign_id),
        // The winner-metric cite carriers ride the COMMAND — the same client
        // path a composed host uses (no direct-SQL stamping here).
        source_id,
        ab_test_id: None,
        ab_testing_enabled: true,
        ab_testing_pc: 100,
    }
}

async fn stored_domain(pool: &sqlx::PgPool, id: Uuid) -> serde_json::Value {
    sqlx::query_scalar("SELECT mailing_domain FROM mailing.mailings WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("stored domain")
}

/// Seed a done mailing citing a source. The cite carriers (source, campaign)
/// ride the upsert command — the same client path a composed host uses; only
/// the `done` state is stamped (reaching it through the lifecycle verbs needs
/// a full send walk, which is not what this read-model probe exercises).
async fn seed_sourced_mailing(
    engine: &MailingWriteService,
    pool: &sqlx::PgPool,
    source_id: Option<Uuid>,
    campaign_id: Option<Uuid>,
) -> Uuid {
    let id = engine
        .create_mailing(&MailingUpsertCommand {
            subject: "sourced".into(),
            preview: None,
            body_html: "<p>x</p>".into(),
            email_from: "s@example.id".into(),
            reply_to: None,
            mailing_domain_raw: serde_json::json!([]),
            target_model: "mailing_contact".into(),
            schedule_type: "immediate".into(),
            schedule_date: None,
            use_exclusion_list: true,
            campaign_id,
            source_id,
            ab_test_id: None,
            ab_testing_enabled: false,
            ab_testing_pc: 0,
        })
        .await
        .expect("sourced mailing create");
    sqlx::query("UPDATE mailing.mailings SET state = 'done' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("done-state stamp");
    id
}

/// Seed one trace on a mailing with the given settled facts.
async fn seed_trace(
    pool: &sqlx::PgPool,
    mailing_id: Uuid,
    campaign_id: Option<Uuid>,
    n: u32,
    sent: bool,
    opened: bool,
    clicked: bool,
    replied: bool,
) {
    let status = if replied {
        "reply"
    } else if opened {
        "open"
    } else if sent {
        "sent"
    } else {
        "outgoing"
    };
    sqlx::query(
        r#"INSERT INTO mailing.mailing_traces
               (mailing_id, campaign_id, recipient_model, recipient_id,
                recipient_email, trace_status, sent_datetime, open_datetime,
                links_click_datetime, reply_datetime)
           VALUES ($1, $2, 'mailing_contact', $3, $4, $5::trace_status,
                   CASE WHEN $6 THEN now() END,
                   CASE WHEN $7 THEN now() END,
                   CASE WHEN $8 THEN now() END,
                   CASE WHEN $9 THEN now() END)"#,
    )
    .bind(mailing_id)
    .bind(campaign_id)
    .bind(Uuid::new_v4())
    .bind(format!("trace-{n}@bridge.example.id"))
    .bind(status)
    .bind(sent)
    .bind(opened)
    .bind(clicked)
    .bind(replied)
    .execute(pool)
    .await
    .expect("trace seed");
}

// ── (1) bridge targets without a composed resolver park loudly ──────────────

#[tokio::test]
async fn bridge_targets_without_a_composed_resolver_park_loudly() {
    let Some(db) = TestDb::new("bridge-park").await else {
        return skipped("bridge-park");
    };
    // NO resolvers composed — the deny-by-default posture.
    let svc = MailingWriteService::new(db.pool.clone());
    let mut ids: Vec<(Uuid, String)> = Vec::new();
    for target in BRIDGE_TARGETS {
        let id = svc
            .create_mailing(&bridge_cmd(target, &format!("No resolver {target}")))
            .await
            .expect("create");
        svc.launch(id, "immediate", None).await.expect("launch");
        ids.push((id, target.to_string()));
    }
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 3);
    assert_eq!(out.parked, 3, "every bridge target parks, none sends");
    assert_eq!(out.recipients_resolved, 0);
    assert_eq!(out.completed, 0);
    for (id, target) in &ids {
        let (state, err): (String, Option<String>) = sqlx::query_as(
            r#"SELECT state::text, metadata->>'send_error'
               FROM mailing.mailings WHERE id = $1"#,
        )
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .expect("mailing row");
        assert_eq!(state, "sending", "a parked mailing stays retryable");
        let err = err.expect("send_error present");
        assert!(
            err.contains(&format!("no {target} resolver is composed")),
            "the park reason names the missing target seam: {err}"
        );
    }
    db.dispose().await;
}

// ── (2) bridge targets resolve through their composed per-target resolvers ──

#[tokio::test]
async fn bridge_targets_resolve_through_their_composed_resolvers() {
    let Some(db) = TestDb::new("bridge-resolve").await else {
        return skipped("bridge-resolve");
    };
    let mut engine = MailingWriteService::new(db.pool.clone());
    for (nonce, target) in BRIDGE_TARGETS.iter().enumerate() {
        engine = engine.with_target_resolver(Arc::new(CannedTarget {
            model: *target,
            nonce: nonce as u32 + 1,
        }));
    }
    let mut ids: Vec<Uuid> = Vec::new();
    for target in BRIDGE_TARGETS {
        let id = engine
            .create_mailing(&bridge_cmd(target, &format!("Resolved {target}")))
            .await
            .expect("create");
        engine.launch(id, "immediate", None).await.expect("launch");
        ids.push(id);
    }
    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 3);
    assert_eq!(out.recipients_resolved, 6, "two recipients per target");
    assert_eq!(out.minted, 6);
    assert_eq!(out.enqueued, 6);
    assert_eq!(out.completed, 3, "every bridge mailing walks to done");
    assert_eq!(out.parked, 0);
    // Per-target traces: every trace's recipient email carries its own
    // target's prefix — each resolver's answer went to its own mailing only.
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"SELECT mailing_id, recipient_email
           FROM mailing.mailing_traces
           ORDER BY recipient_email"#,
    )
    .fetch_all(&db.pool)
    .await
    .expect("traces");
    assert_eq!(rows.len(), 6);
    for (mailing_id, email) in &rows {
        let target: String =
            sqlx::query_scalar("SELECT target_model::text FROM mailing.mailings WHERE id = $1")
                .bind(mailing_id)
                .fetch_one(&db.pool)
                .await
                .expect("target");
        assert!(
            email.starts_with(&target),
            "trace {email} belongs to a {target} mailing"
        );
    }
    db.dispose().await;
}

// ── (3) the legacy party seam keeps working through its adapter ─────────────

#[tokio::test]
async fn party_resolver_keeps_working_through_the_adapter() {
    let Some(db) = TestDb::new("bridge-party-adapter").await else {
        return skipped("bridge-party-adapter");
    };
    let svc = MailingWriteService::new(db.pool.clone())
        .with_party_resolver(Arc::new(CannedParty { nonce: 7 }));
    let id = svc
        .create_mailing(&bridge_cmd("party", "Party through the adapter"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 1);
    assert_eq!(out.recipients_resolved, 2);
    assert_eq!(out.minted, 2);
    assert_eq!(out.enqueued, 2);
    assert_eq!(out.completed, 1);
    let emails: Vec<String> = sqlx::query_scalar(
        "SELECT recipient_email FROM mailing.mailing_traces WHERE mailing_id = $1",
    )
    .bind(id)
    .fetch_all(&db.pool)
    .await
    .expect("party traces");
    assert!(emails.iter().all(|e| e.starts_with("party-7-")));
    db.dispose().await;
}

// ── (4) declarative default domains apply per target at create ──────────────

#[tokio::test]
async fn default_domains_apply_per_target_at_create() {
    let Some(db) = TestDb::new("bridge-defaults").await else {
        return skipped("bridge-defaults");
    };
    let svc = MailingWriteService::new(db.pool.clone());

    // selling_customer + EMPTY domain → the typed mail-ability default.
    let sale = svc
        .create_mailing(&bridge_cmd("selling_customer", "Sale default"))
        .await
        .expect("create");
    assert_eq!(
        stored_domain(&db.pool, sale).await,
        serde_json::json!([{"field": "email", "op": "!=", "value": ""}]),
        "the sale bridge adopts its exclusion policy as declarative JSON"
    );

    // crm_lead + EMPTY domain → upstream parity: NO default is injected.
    let lead = svc
        .create_mailing(&bridge_cmd("crm_lead", "Lead no-default"))
        .await
        .expect("create");
    assert_eq!(
        stored_domain(&db.pool, lead).await,
        serde_json::json!([]),
        "the crm bridge ships no default domain (upstream parity)"
    );

    // An AUTHORED domain always wins over the target's default.
    let mut authored = bridge_cmd("selling_customer", "Authored wins");
    authored.mailing_domain_raw = serde_json::json!([["email", "like", "acme"]]);
    let won = svc.create_mailing(&authored).await.expect("create");
    assert_eq!(
        stored_domain(&db.pool, won).await,
        serde_json::json!([{"field": "email", "op": "like", "value": "acme"}]),
        "an explicitly authored domain is stored verbatim (canonical form)"
    );

    // The closed enum still refuses unknown targets loudly.
    let err = svc
        .create_mailing(&bridge_cmd("partner", "Unknown target"))
        .await
        .expect_err("must refuse");
    match &err {
        MailingWriteError::Invalid(msg) => {
            assert!(msg.contains("partner"), "names the refused target: {msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(err.http_status(), 422);
    db.dispose().await;
}

// ── (5) source-grouped stats attribute by source and flag shared sources ────

#[tokio::test]
async fn source_grouped_stats_attribute_by_source_and_flag_shared_sources() {
    let Some(db) = TestDb::new("bridge-source-stats").await else {
        return skipped("bridge-source-stats");
    };
    let shared = Uuid::new_v4(); // serves TWO campaigns
    let solo = Uuid::new_v4();
    let c1 = Uuid::new_v4();
    let c2 = Uuid::new_v4();
    let c3 = Uuid::new_v4();
    let engine = MailingWriteService::new(db.pool.clone());

    // shared → two mailings under two different campaigns.
    let m1 = seed_sourced_mailing(&engine, &db.pool, Some(shared), Some(c1)).await;
    seed_trace(&db.pool, m1, Some(c1), 0, true, true, false, false).await;
    seed_trace(&db.pool, m1, Some(c1), 1, false, false, false, false).await;
    let m2 = seed_sourced_mailing(&engine, &db.pool, Some(shared), Some(c2)).await;
    seed_trace(&db.pool, m2, Some(c2), 2, true, false, true, true).await;
    // solo → one mailing, one campaign.
    let m3 = seed_sourced_mailing(&engine, &db.pool, Some(solo), Some(c3)).await;
    seed_trace(&db.pool, m3, Some(c3), 3, true, false, false, false).await;
    // no source → attributes nothing, stays out of the read.
    let m4 = seed_sourced_mailing(&engine, &db.pool, None, Some(c3)).await;
    seed_trace(&db.pool, m4, Some(c3), 4, true, true, false, false).await;

    let stats = MailingStatsReadService::new(db.pool.clone());
    let rows = stats
        .source_grouped_stats(&[m1, m2, m3, m4])
        .await
        .expect("source-grouped stats");
    assert_eq!(rows.len(), 2, "the source-less mailing stays out");

    let shared_row = rows.iter().find(|r| r.source_id == shared).expect("shared");
    assert_eq!(shared_row.campaigns, 2, "the source serves two campaigns");
    assert_eq!(shared_row.mailings, 2);
    assert_eq!(shared_row.total, 3);
    assert_eq!(shared_row.sent, 2);
    // The opened arm counts open AND reply (a reply implies an open) —
    // the reply trace on m2 lands in both arms.
    assert_eq!(shared_row.opened, 2);
    assert_eq!(shared_row.clicked, 1);
    assert_eq!(shared_row.replied, 1);
    assert!(shared_row.shared_source);
    assert_eq!(
        shared_row.shared_source_caveat,
        Some(SHARED_SOURCE_CAVEAT),
        "the read model STATES the caveat, it never lets the reader infer it"
    );

    let solo_row = rows.iter().find(|r| r.source_id == solo).expect("solo");
    assert_eq!(solo_row.campaigns, 1);
    assert_eq!(solo_row.mailings, 1);
    assert_eq!(solo_row.sent, 1);
    assert!(!solo_row.shared_source);
    assert_eq!(solo_row.shared_source_caveat, None);
    db.dispose().await;
}

// ── shared invoiced-amount probe scaffolding ────────────────────────────────

/// Drive a `sale_invoiced_amount` test to the promotion edge: two variants
/// done (both at pc=100 — the campaign seen-list hands the second one the
/// complete-empty path), each citing its own engagement source through the
/// upsert command (the real client path), promote_at backdated.
async fn invoiced_edge_setup(db: &TestDb) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let (audience_id, _) = seed_audience(&db.pool, "invoiced", 2).await;
    let ab = AbTestWriteService::new(db.pool.clone());
    let engine = MailingWriteService::new(db.pool.clone());
    let campaign = Uuid::new_v4();
    // The sources each variant will cite — minted up front so the cites ride
    // the CREATE command instead of a post-hoc SQL stamp.
    let s1 = Uuid::new_v4();
    let s2 = Uuid::new_v4();
    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    let test_id = ab
        .create_ab_test(campaign, "sale_invoiced_amount", Some(future))
        .await
        .expect("test");
    let a = engine
        .create_mailing(&audience_variant_cmd(
            audience_id,
            campaign,
            Some(s1),
            "Rich source variant",
        ))
        .await
        .expect("A");
    let b = engine
        .create_mailing(&audience_variant_cmd(
            audience_id,
            campaign,
            Some(s2),
            "Lean source variant",
        ))
        .await
        .expect("B");
    ab.bind_variant(a, test_id, 100).await.expect("bind A");
    ab.bind_variant(b, test_id, 100).await.expect("bind B");
    engine.launch(a, "immediate", None).await.expect("launch A");
    engine.launch(b, "immediate", None).await.expect("launch B");
    engine.send_queue_sweep().await.expect("drive both variants");
    for (id, state) in sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, state::text FROM mailing.mailings WHERE id = ANY($1)",
    )
    .bind(vec![a, b])
    .fetch_all(&db.pool)
    .await
    .expect("variant states")
    {
        assert_eq!(state, "done", "variant {id} must be done before promotion");
    }
    // Time control only: promote_at must stay FUTURE while the variants are
    // driven to done (a past-due test would let the same sweep attempt the
    // promotion before both variants settled), so the backdate is applied
    // after the send walk as a raw stamp. Every authorable cite rides the
    // command above.
    sqlx::query(
        "UPDATE mailing.mailing_ab_tests SET promote_at = now() - interval '1 minute' \
         WHERE id = $1",
    )
    .bind(test_id)
    .execute(&db.pool)
    .await
    .expect("backdate promote_at");
    (test_id, a, b, s1, s2)
}

// ── (6) an uncomposed billing seam skips promotion loudly ───────────────────

#[tokio::test]
async fn invoiced_amount_without_a_composed_seam_skips_promotion_loudly() {
    let Some(db) = TestDb::new("bridge-seam-refuse").await else {
        return skipped("bridge-seam-refuse");
    };
    let (test_id, _a, _b, _s1, _s2) = invoiced_edge_setup(&db).await;
    // NO billing port composed — the refusing default is the point.
    let engine = MailingWriteService::new(db.pool.clone());
    let out = engine.send_queue_sweep().await.expect("sweep 2");
    assert_eq!(out.ab_seam_refusals, 1, "the refusal is a loud, counted skip");
    assert_eq!(out.ab_promotions, 0, "never promote on a fabricated zero");
    let ab = AbTestWriteService::new(db.pool.clone());
    let view = ab.view(test_id).await.expect("view");
    assert!(!view.completed, "the test stays open for the next sweep");
    assert_eq!(view.winner_mailing_id, None);
    // The skip is retryable: a later sweep re-attempts (and refuses again
    // while the seam stays uncomposed).
    let out = engine.send_queue_sweep().await.expect("sweep 3");
    assert_eq!(out.ab_seam_refusals, 1);
    assert_eq!(out.ab_promotions, 0);
    db.dispose().await;
}

// ── (7) the composed seam ranks variants by source ──────────────────────────

#[tokio::test]
async fn invoiced_amount_ranks_variants_by_source_through_the_seam() {
    let Some(db) = TestDb::new("bridge-seam-rank").await else {
        return skipped("bridge-seam-rank");
    };
    let (test_id, a, _b, s1, s2) = invoiced_edge_setup(&db).await;
    let port = Arc::new(MapBackedInvoiced {
        amounts: [
            (s1, rust_decimal::Decimal::new(1_250_000, 2)), // 12,500.00
            (s2, rust_decimal::Decimal::new(90_000, 2)),    //    900.00
        ]
        .into_iter()
        .collect(),
        consulted: std::sync::Mutex::new(Vec::new()),
    });
    let engine = MailingWriteService::new(db.pool.clone())
        .with_sale_invoiced_amount_port(port.clone());
    let out = engine.send_queue_sweep().await.expect("sweep 2");
    assert_eq!(out.ab_seam_refusals, 0);
    assert_eq!(out.ab_promotions, 1, "the richer source's variant wins");
    let ab = AbTestWriteService::new(db.pool.clone());
    let view = ab.view(test_id).await.expect("view");
    assert!(view.completed);
    assert_eq!(
        view.winner_mailing_id,
        Some(a),
        "variant A cites the richer source"
    );

    // ONE consultation per DISTINCT source — exactly the shared-source
    // attribution contract (several campaigns may share a source; the seam
    // answers per source, not per campaign).
    let consulted = port.consulted.lock().unwrap().clone();
    assert_eq!(
        consulted.len(),
        2,
        "one call per distinct source: {consulted:?}"
    );
    assert!(consulted.contains(&s1) && consulted.contains(&s2));
    db.dispose().await;
}

// ── (8) the parallel sms winner axis + the mixed-channel compare ────────────

#[tokio::test]
async fn sms_winner_axis_and_mixed_channel_compare() {
    let Some(db) = TestDb::new("bridge-sms-axis").await else {
        return skipped("bridge-sms-axis");
    };
    let (audience_id, _) = seed_audience(&db.pool, "sms-axis", 2).await;
    let ab = AbTestWriteService::new(db.pool.clone());
    let engine = MailingWriteService::new(db.pool.clone());
    let campaign = Uuid::new_v4();
    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    let test_id = ab
        .create_ab_test(campaign, "opened_ratio", Some(future))
        .await
        .expect("test");

    // The PARALLEL sms axis declares independently of the mail axis — and
    // the closed enum refuses anything else.
    match ab
        .set_sms_winner_selection(test_id, "bogus_metric")
        .await
        .expect_err("closed enum refuses")
    {
        AbTestWriteError::Invalid(msg) => {
            assert!(msg.contains("bogus_metric"), "{msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    ab.set_sms_winner_selection(test_id, "clicks_ratio")
        .await
        .expect("sms axis");
    let view = ab.view(test_id).await.expect("view");
    assert_eq!(view.winner_selection, "opened_ratio");
    assert_eq!(view.winner_selection_sms.as_deref(), Some("clicks_ratio"));

    // Variant A — the mail channel, driven to done with real traces
    // (pc=100: the full audience joins deterministically).
    let a = engine
        .create_mailing(&audience_variant_cmd(audience_id, campaign, None, "Mail variant"))
        .await
        .expect("A");
    ab.bind_variant(a, test_id, 100).await.expect("bind A");
    engine.launch(a, "immediate", None).await.expect("launch A");

    // Variant B — the sms channel (seeded draft: the upsert command has no
    // plaintext body field), bound as a variant and launched.
    let b = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mailing.mailings
               (id, subject, body_html, body_plaintext, email_from, state,
                mailing_type, mailing_domain)
           VALUES ($1, 'Sms variant', '<p>x</p>', 'bridge sms body',
                   'c@example.id', 'draft', 'sms', '[]'::jsonb)"#,
    )
    .bind(b)
    .execute(&db.pool)
    .await
    .expect("sms variant seed");
    ab.bind_variant(b, test_id, 100).await.expect("bind B");
    engine.launch(b, "immediate", None).await.expect("launch B");

    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.completed, 1, "the mail variant completes");
    assert_eq!(
        out.parked, 1,
        "the sms variant parks: its send walk is a separate seam"
    );
    assert_eq!(out.ab_promotions, 0, "promote_at is still future here");

    // Settle the mail variant's traces: sent AND opened AND clicked.
    sqlx::query(
        r#"UPDATE mailing.mailing_traces
           SET sent_datetime = now(), trace_status = 'open', open_datetime = now(),
               links_click_datetime = now()
           WHERE mailing_id = $1"#,
    )
    .bind(a)
    .execute(&db.pool)
    .await
    .expect("settle traces");

    let cmp = ab.compare_channels(test_id).await.expect("compare");
    assert_eq!(cmp.winner_selection, "opened_ratio");
    assert_eq!(cmp.winner_selection_sms.as_deref(), Some("clicks_ratio"));
    assert_eq!(cmp.rows.len(), 2, "both variants appear, split per channel");
    let mail = cmp.rows.iter().find(|r| r.channel == "mail").expect("mail row");
    assert_eq!(mail.mailing_id, a);
    assert_eq!(mail.sent, 2);
    assert_eq!(mail.hits, 2, "the opened axis counts the opens");
    assert_eq!(mail.ratio, Some(100));
    assert!(!mail.parked);
    let sms = cmp.rows.iter().find(|r| r.channel == "sms").expect("sms row");
    assert_eq!(sms.mailing_id, b);
    assert_eq!(sms.sent, 0, "the sms variant carries no traces yet");
    assert_eq!(sms.hits, 0);
    assert_eq!(sms.ratio, None, "no fabricated 0% — nothing was sent");
    assert!(sms.parked, "the parked sms variant says so in its row");
    db.dispose().await;
}

// ── (11) the events-registrations bridge target (mass_mailing_event) ────────

/// Apply the events module's migrations to this test's scratch database
/// (the harness's own raw-SQL runner shape, scoped to this probe family
/// only — the shared dirs array stays untouched). The events migrations
/// are self-contained (schema `event`, no cross-schema references), so a
/// bare scratch database applies them cleanly. The events module is NOT a
/// Cargo dependency of this crate: the probe reads its registration read
/// model through SQL only, exactly like the host-composed adapter does.
async fn apply_events_migrations(pool: &sqlx::PgPool, marker: &str) -> bool {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dir = format!("{manifest}/../backbone-events/migrations");
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".up.sql"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) => {
            eprintln!("SKIPPED-DB: {marker}: cannot read {dir}: {e}");
            return false;
        }
    };
    files.sort();
    let mut conn = match pool.acquire().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIPPED-DB: {marker}: cannot acquire pool conn: {e}");
            return false;
        }
    };
    for file in files {
        let sql = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIPPED-DB: {marker}: cannot read {}: {e}", file.display());
                return false;
            }
        };
        if let Err(e) = sqlx::raw_sql(&sql).execute(&mut *conn).await {
            eprintln!("SKIPPED-DB: {marker}: migration {} failed: {e}", file.display());
            return false;
        }
    }
    true
}

/// Seed one event row and return its id (registrations carry a real FK to
/// it, RESTRICT). The event's own stage FK is satisfied by a seeded stage.
async fn seed_event(pool: &sqlx::PgPool) -> Uuid {
    let stage_id = Uuid::new_v4();
    sqlx::query(r#"INSERT INTO event.stages (id, name) VALUES ($1, 'Bridge Probe Stage')"#)
        .bind(stage_id)
        .execute(pool)
        .await
        .expect("stage seed");
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO event.events (id, name, stage_id, date_begin, date_end)
           VALUES ($1, 'Bridge Probe Expo', $2, now(), now() + interval '2 days')"#,
    )
    .bind(id)
    .bind(stage_id)
    .execute(pool)
    .await
    .expect("event seed");
    id
}

/// Monotonic barcode source — the events module shapes barcodes as decimal
/// strings only (`registrations_barcode_shape`).
static REGISTRATION_BARCODE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Seed one registration with explicit eligibility facts. `deleted` stamps
/// `metadata.deleted_at` (the soft-delete fence).
async fn seed_registration(
    pool: &sqlx::PgPool,
    event_id: Uuid,
    email: &str,
    name: &str,
    company_name: Option<&str>,
    state: &str,
    active: bool,
    deleted: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO event.registrations
             (id, event_id, name, email, company_name, state, active, barcode, metadata)
           VALUES ($1, $2, $3, $4, $5, $6::event_registration_state, $7, $8,
                   jsonb_build_object('deleted_at', CASE WHEN $9 THEN now() END))"#,
    )
    .bind(id)
    .bind(event_id)
    .bind(name)
    .bind(email)
    .bind(company_name)
    .bind(state)
    .bind(active)
    .bind(format!(
        "{}",
        REGISTRATION_BARCODE.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ))
    .bind(deleted)
    .execute(pool)
    .await
    .expect("registration seed");
    id
}

/// The DSL's own spelling of a field (the grammar's snake_case names, the
/// same strings `parse_domain` accepts and `DomainInvalid::UnknownField`
/// reports) — used so refusals name fields exactly as operators wrote them.
fn domain_field_name(f: &DomainField) -> &'static str {
    match f {
        DomainField::Email => "email",
        DomainField::Name => "name",
        DomainField::FirstName => "first_name",
        DomainField::LastName => "last_name",
        DomainField::CompanyName => "company_name",
        DomainField::CountryCode => "country_code",
        DomainField::MailingAudienceId => "mailing_audience_id",
    }
}

/// The typed-port probe double over the REAL registration read model —
/// the miniature of the host adapter: the events module's declared
/// mail-eligibility law (`state IN ('open','done') AND active`, plus not
/// soft-deleted and carrying an email) as the population predicate, and
/// the whitelisted DSL bound onto the registration columns (email, attendee
/// name, company name); every other whitelisted field REFUSES loudly —
/// never a silently dropped filter.
struct SeededRegistrationTarget {
    pool: sqlx::PgPool,
}

#[async_trait::async_trait]
impl backbone_mailing::application::service::EventRegistrationTargetResolver
    for SeededRegistrationTarget
{
    async fn resolve(
        &self,
        domain: &CompiledDomain,
    ) -> Result<Vec<ResolvedRecipient>, backbone_mailing::application::service::EventRegistrationTargetError>
    {
        use backbone_mailing::application::service::EventRegistrationTargetError as E;
        use sqlx::Arguments;

        let mut sql = String::from(
            "SELECT r.id AS recipient_id, r.email \
             FROM event.registrations r \
             WHERE r.state IN ('open', 'done') \
             AND r.active \
             AND (r.metadata->>'deleted_at') IS NULL \
             AND coalesce(r.email, '') <> ''",
        );
        let mut args = sqlx::postgres::PgArguments::default();
        let mut n = 0usize;
        for term in &domain.terms {
            let col = match term.field {
                DomainField::Email => "r.email",
                DomainField::Name => "r.name",
                DomainField::CompanyName => "r.company_name",
                other => {
                    return Err(E::Backend(format!(
                        "the event_registration target has no {} column to bind a domain \
                         term to — remove the term or target a model that carries it",
                        domain_field_name(&other)
                    )));
                }
            };
            n += 1;
            match term.op {
                DomainOp::Eq | DomainOp::In => {
                    sql.push_str(&format!(
                        " AND lower(coalesce({col}, '')) = ANY(${n})"
                    ));
                    let lowered: Vec<String> =
                        term.values.iter().map(|v| v.to_lowercase()).collect();
                    args.add(lowered).map_err(|e| E::Backend(e.to_string()))?;
                }
                DomainOp::Ne | DomainOp::NotIn => {
                    sql.push_str(&format!(
                        " AND lower(coalesce({col}, '')) <> ALL(${n})"
                    ));
                    let lowered: Vec<String> =
                        term.values.iter().map(|v| v.to_lowercase()).collect();
                    args.add(lowered).map_err(|e| E::Backend(e.to_string()))?;
                }
                DomainOp::Like => {
                    sql.push_str(&format!(
                        " AND coalesce({col}, '') ILIKE '%' || ${n} || '%'"
                    ));
                    let pat = term.values.first().cloned().unwrap_or_default();
                    args.add(pat).map_err(|e| E::Backend(e.to_string()))?;
                }
                DomainOp::NotLike => {
                    sql.push_str(&format!(
                        " AND coalesce({col}, '') NOT ILIKE '%' || ${n} || '%'"
                    ));
                    let pat = term.values.first().cloned().unwrap_or_default();
                    args.add(pat).map_err(|e| E::Backend(e.to_string()))?;
                }
            }
        }
        sql.push_str(" ORDER BY 2 LIMIT 100001");
        let rows: Vec<ResolvedRecipient> =
            sqlx::query_as_with::<sqlx::Postgres, ResolvedRecipient, _>(&sql, args)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| E::Backend(format!("event_registration resolve: {e}")))?;
        Ok(rows)
    }
}

#[tokio::test]
async fn event_registration_target_resolves_typed_against_seeded_registrations() {
    let Some(db) = TestDb::new("bridge-event-resolve").await else {
        return skipped("bridge-event-resolve");
    };
    if !apply_events_migrations(&db.pool, "bridge-event-resolve").await {
        db.dispose().await;
        return;
    }
    let event_id = seed_event(&db.pool).await;
    // The eligibility fence: only the first two rows are mail-eligible.
    seed_registration(&db.pool, event_id, "open@example.id", "Open Attendee", Some("Acme"), "open", true, false).await;
    seed_registration(&db.pool, event_id, "done@example.id", "Done Attendee", Some("Beta"), "done", true, false).await;
    seed_registration(&db.pool, event_id, "cancel@example.id", "Cancelled", None, "cancel", true, false).await;
    seed_registration(&db.pool, event_id, "archive@example.id", "Archived", None, "open", false, false).await;
    seed_registration(&db.pool, event_id, "deleted@example.id", "Deleted", None, "open", true, true).await;
    seed_registration(&db.pool, event_id, "", "No Email", None, "open", true, false).await;

    let engine = MailingWriteService::new(db.pool.clone()).with_event_registration_resolver(
        Arc::new(SeededRegistrationTarget { pool: db.pool.clone() }),
    );
    let id = engine
        .create_mailing(&bridge_cmd("event_registration", "Event bridge resolve"))
        .await
        .expect("create");
    // No default domain is injected (upstream's state filter is the
    // resolver's structural population predicate — recorded deviation).
    assert_eq!(
        stored_domain(&db.pool, id).await,
        serde_json::json!([]),
        "the events bridge ships no default domain"
    );
    engine.launch(id, "immediate", None).await.expect("launch");
    let out = engine.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 1);
    assert_eq!(out.recipients_resolved, 2, "only the eligible registrations");
    assert_eq!(out.minted, 2);
    assert_eq!(out.enqueued, 2);
    assert_eq!(out.completed, 1);
    assert_eq!(out.parked, 0);
    let emails: Vec<String> = sqlx::query_scalar(
        "SELECT recipient_email FROM mailing.mailing_traces WHERE mailing_id = $1 ORDER BY 1",
    )
    .bind(id)
    .fetch_all(&db.pool)
    .await
    .expect("traces");
    assert_eq!(emails, vec!["done@example.id", "open@example.id"]);

    // A typed domain term narrows within the eligible population.
    let resolver = SeededRegistrationTarget { pool: db.pool.clone() };
    let narrowed = resolver
        .resolve(&parse_json(
            r#"[{"field": "company_name", "op": "like", "value": "acme"}]"#,
        ))
        .await
        .expect("narrowed resolve");
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].email, "open@example.id");
    db.dispose().await;
}

/// Parse helper for probe-local domains (the module's own typed parser).
fn parse_json(raw: &str) -> CompiledDomain {
    backbone_mailing::application::service::mailing_write_service::parse_domain(
        &serde_json::from_str::<serde_json::Value>(raw).expect("probe domain json"),
    )
    .expect("probe domain parses")
}

#[tokio::test]
async fn event_registration_without_a_composed_resolver_parks_loudly() {
    let Some(db) = TestDb::new("bridge-event-park").await else {
        return skipped("bridge-event-park");
    };
    // NO resolver composed — the registry's fail-closed default.
    let svc = MailingWriteService::new(db.pool.clone());
    let id = svc
        .create_mailing(&bridge_cmd("event_registration", "No resolver"))
        .await
        .expect("create");
    svc.launch(id, "immediate", None).await.expect("launch");
    let out = svc.send_queue_sweep().await.expect("sweep");
    assert_eq!(out.claimed, 1);
    assert_eq!(out.parked, 1, "the events target parks, never sends");
    assert_eq!(out.recipients_resolved, 0);
    assert_eq!(out.completed, 0);
    let (state, err): (String, Option<String>) = sqlx::query_as(
        r#"SELECT state::text, metadata->>'send_error' FROM mailing.mailings WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .expect("mailing row");
    assert_eq!(state, "sending", "a parked mailing stays retryable");
    let err = err.expect("send_error present");
    assert!(
        err.contains("no event_registration resolver is composed"),
        "the park reason names the missing target seam: {err}"
    );
    let traces: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("trace count");
    assert_eq!(traces, 0, "never a silent zero-recipient sweep");

    // The typed refusing default refuses with the TYPED error naming the
    // install verb (the direct-port deny shape).
    let refusing =
        backbone_mailing::application::service::RefusingEventRegistrationTarget;
    let err = refusing.resolve(&CompiledDomain::default()).await.expect_err("refuses");
    assert!(
        matches!(
            err,
            backbone_mailing::application::service::EventRegistrationTargetError::NotComposed { .. }
        ),
        "the refusal is the typed NotComposed variant: {err:?}"
    );
    assert!(
        err.to_string().contains("with_event_registration_resolver"),
        "the typed refusal names the install verb: {err}"
    );
    db.dispose().await;
}

#[tokio::test]
async fn event_registration_refuses_overset_domains_not_coerces() {
    let Some(db) = TestDb::new("bridge-event-overset").await else {
        return skipped("bridge-event-overset");
    };
    let svc = MailingWriteService::new(db.pool.clone());

    // An INVALID domain (a field outside the whitelist — upstream's state
    // filter) is refused at create with the typed 422, never coerced.
    let mut overset = bridge_cmd("event_registration", "Overset");
    overset.mailing_domain_raw = serde_json::json!([["state", "=", "open"]]);
    let err = svc.create_mailing(&overset).await.expect_err("must refuse");
    match &err {
        MailingWriteError::Domain(DomainInvalid::UnknownField(field)) => {
            assert_eq!(field, "state");
        }
        other => panic!("expected Domain(UnknownField), got {other:?}"),
    }
    assert_eq!(err.http_status(), 422);

    // A WHITELISTED but unbindable field parses at create, then the
    // resolver REFUSES it loudly (a filter the target cannot honor is
    // never silently dropped).
    let resolver = SeededRegistrationTarget { pool: db.pool.clone() };
    let err = resolver
        .resolve(&parse_json(r#"[{"field": "country_code", "op": "=", "value": "ID"}]"#))
        .await
        .expect_err("unbindable field refuses");
    assert!(
        err.to_string().contains("country_code"),
        "the refusal names the unbindable field: {err}"
    );

    // On the sweep the same refusal surfaces as the loud typed Conflict —
    // the mailing sends NOTHING rather than mailing a mis-filtered
    // audience, and stays retryable for the fix.
    let engine = MailingWriteService::new(db.pool.clone())
        .with_event_registration_resolver(Arc::new(SeededRegistrationTarget { pool: db.pool.clone() }));
    let mut whitelisted = bridge_cmd("event_registration", "Unbindable at sweep");
    whitelisted.mailing_domain_raw =
        serde_json::json!([{"field": "first_name", "op": "=", "value": "Nope"}]);
    let id = engine.create_mailing(&whitelisted).await.expect("create");
    engine.launch(id, "immediate", None).await.expect("launch");
    let err = engine.send_queue_sweep().await.expect_err("sweep refuses");
    assert!(
        err.to_string().contains("event_registration resolver failed"),
        "the sweep error names the refusing target: {err}"
    );
    assert!(
        err.to_string().contains("first_name"),
        "the refusal names the unbindable field: {err}"
    );
    let traces: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mailing.mailing_traces WHERE mailing_id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .expect("trace count");
    assert_eq!(traces, 0, "refused, not coerced — zero sends");
    db.dispose().await;
}
