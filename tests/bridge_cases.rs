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
    MailingUpsertCommand, MailingWriteError, MailingWriteService, PartyRecipientResolver,
    TargetRecipientResolver,
};
use backbone_mailing::application::service::sale_invoiced_amount_port::{
    SaleInvoicedAmountError, SaleInvoicedAmountPort, SourceInvoicedAmount,
};
use backbone_mailing::infrastructure::persistence::mailing_send_repository::{
    CompiledDomain, ResolvedRecipient,
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
