//! Public trace-route behavior (`/r/:code/m/:trace` + suffixes): the click
//! round-trip (301 + the declared stamp pair), the open pixel, the
//! unsubscribe leg's idempotency, the uniform consistency refusal, the
//! per-code throttle, and the fail-closed uncomposed port — fresh-DB,
//! disposable scratch per test, router driven in-process via tower's
//! `ServiceExt::oneshot`.

#[path = "behavior/common/mod.rs"]
mod common;

use common::*;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::util::ServiceExt;

use backbone_mailing::application::service::trace_click_ports::{
    CannedTraceClick, TraceClickSlot,
};
use backbone_mailing::infrastructure::persistence::trace_repository::TraceRepository;
use backbone_mailing::presentation::http::trace_routes::public_trace_routes;
use uuid::Uuid;

/// The 1×1 GIF the route file serves (asserted byte-for-byte so a future
/// "harmless" change to the pixel body is a conscious diff, not drift).
const PIXEL: [u8; 43] = [
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0xff,
    0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

/// A router over a fresh scratch DB + the canned click port (campaign +
/// target configured per test).
fn router(
    pool: &sqlx::PgPool,
    port: CannedTraceClick,
) -> axum::Router {
    let slot = TraceClickSlot::default();
    slot.install(Arc::new(port));
    public_trace_routes(pool.clone(), slot)
}

/// A router whose click port was NEVER composed — the deny-by-default slot.
fn uncomposed_router(pool: &sqlx::PgPool) -> axum::Router {
    public_trace_routes(pool.clone(), TraceClickSlot::default())
}

async fn req(
    router: &axum::Router,
    method: Method,
    uri: &str,
    content_type: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Vec<(header::HeaderName, String)>, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    let request = builder
        .body(Body::from(body.unwrap_or_default().to_owned()))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("oneshot");
    let status = response.status();
    let headers: Vec<(header::HeaderName, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.clone(), v.to_str().unwrap_or_default().to_owned()))
        .collect();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, headers, bytes.to_vec())
}

fn header_value<'a>(headers: &'a [(header::HeaderName, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.as_str() == name)
        .map(|(_, v)| v.as_str())
}

/// Mint a live outgoing trace under a real mailing row (campaign + domain
/// configurable — the consistency check and the unsubscribe walk read both).
async fn seed(
    pool: &sqlx::PgPool,
    campaign_id: Option<Uuid>,
    domain: &serde_json::Value,
    email: &str,
) -> (Uuid, Uuid) {
    let (mailing_id, trace_id, recipient_id) =
        (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query(
        r#"INSERT INTO mailing.mailings
               (id, subject, body_html, email_from, campaign_id, mailing_domain)
           VALUES ($1, 'route case', '<p>hi</p>', 'news@example.id', $2, $3)"#,
    )
    .bind(mailing_id)
    .bind(campaign_id)
    .bind(domain)
    .execute(&mut *tx)
    .await
    .expect("mailing");
    TraceRepository::mint_trace(
        &mut tx,
        trace_id,
        mailing_id,
        campaign_id,
        "mailing_contact",
        recipient_id,
        email,
        "outgoing",
        None,
        false,
    )
    .await
    .expect("trace");
    tx.commit().await.expect("commit");
    (mailing_id, trace_id)
}

/// One trace's (status, open_datetime, click_datetime) — the stamp pair the
/// legs are asserted against.
async fn stamps(pool: &sqlx::PgPool, trace_id: Uuid) -> (String, Option<String>, Option<String>) {
    let row: (String, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            r#"SELECT trace_status::text, open_datetime, links_click_datetime
               FROM mailing.mailing_traces WHERE id = $1"#,
        )
        .bind(trace_id)
        .fetch_one(pool)
        .await
        .expect("stamps");
    (
        row.0,
        row.1.map(|d| d.to_rfc3339()),
        row.2.map(|d| d.to_rfc3339()),
    )
}

fn canned(campaign_id: Option<Uuid>) -> CannedTraceClick {
    CannedTraceClick {
        campaign_id,
        url: "https://target.example.id/offer".to_string(),
        unknown: false,
    }
}

// ─── the click leg ────────────────────────────────────────────────────────────

#[tokio::test]
async fn click_round_trip_redirects_301_and_stamps_the_declared_pair() {
    let Some(db) = TestDb::new("trclick").await else {
        return skipped("trclick");
    };
    let campaign = Uuid::new_v4();
    let (_, trace_id) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "click@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    let (status, headers, _body) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}"),
        None,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        header_value(&headers, "location"),
        Some("https://target.example.id/offer")
    );
    assert_eq!(header_value(&headers, "cache-control"), Some("no-store, private"));

    let (state, opened, clicked) = stamps(&db.pool, trace_id).await;
    assert_eq!(state, "open");
    assert!(opened.is_some(), "open_datetime stamped by the pair");
    assert!(clicked.is_some(), "links_click_datetime stamped by the pair");
}

#[tokio::test]
async fn click_replay_is_idempotent_the_first_open_stamp_wins() {
    let Some(db) = TestDb::new("trreplay").await else {
        return skipped("trreplay");
    };
    let campaign = Uuid::new_v4();
    let (_, trace_id) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "replay@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    let (first, _, _) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}"),
        None,
        None,
    )
    .await;
    let (state1, opened1, clicked1) = stamps(&db.pool, trace_id).await;
    let (second, _, _) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}"),
        None,
        None,
    )
    .await;
    let (state2, opened2, clicked2) = stamps(&db.pool, trace_id).await;

    assert_eq!(first, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(second, StatusCode::MOVED_PERMANENTLY, "a replay still redirects");
    // Skipped-on-replay is the correct outcome: status never leaves `open`
    // and the FIRST open moment is the durable fact; the click datetime
    // carries the LAST click by declaration.
    assert_eq!(state1, "open");
    assert_eq!(state2, "open");
    assert_eq!(opened1, opened2, "open_datetime keeps its first stamp");
    assert!(clicked1.is_some() && clicked2.is_some());
}

// ─── the open-pixel leg ───────────────────────────────────────────────────────

#[tokio::test]
async fn pixel_stamps_open_only_and_answers_the_gif() {
    let Some(db) = TestDb::new("trpixel").await else {
        return skipped("trpixel");
    };
    let campaign = Uuid::new_v4();
    let (_, trace_id) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "pixel@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    let (status, headers, body) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}/pixel.gif"),
        None,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, "content-type"), Some("image/gif"));
    assert_eq!(header_value(&headers, "cache-control"), Some("no-store, private"));
    assert_eq!(body, PIXEL);

    let (state, opened, clicked) = stamps(&db.pool, trace_id).await;
    assert_eq!(state, "open");
    assert!(opened.is_some(), "set_opened fired");
    assert!(
        clicked.is_none(),
        "a pixel never stamps links_click_datetime"
    );
}

#[tokio::test]
async fn pixel_miss_answers_the_identical_gif_no_oracle() {
    let Some(db) = TestDb::new("trpixelmiss").await else {
        return skipped("trpixelmiss");
    };
    let campaign = Uuid::new_v4();
    let (_, live_trace) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "live@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    let (_, _, hit) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{live_trace}/pixel.gif"),
        None,
        None,
    )
    .await;
    let (miss_status, miss_headers, miss) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{}/pixel.gif", Uuid::new_v4()),
        None,
        None,
    )
    .await;
    // Mismatched pair: the canned port carries a different campaign.
    let other = router(&db.pool, canned(Some(Uuid::new_v4())));
    let (mm_status, mm_headers, mm) = req(
        &other,
        Method::GET,
        &format!("/r/abc123/m/{live_trace}/pixel.gif"),
        None,
        None,
    )
    .await;

    assert_eq!(miss_status, StatusCode::OK);
    assert_eq!(mm_status, StatusCode::OK);
    assert_eq!(miss, hit, "unknown trace: byte-identical pixel");
    assert_eq!(mm, hit, "inconsistent pair: byte-identical pixel");
    assert_eq!(
        header_value(&miss_headers, "cache-control"),
        Some("no-store, private")
    );
    assert_eq!(
        header_value(&mm_headers, "cache-control"),
        Some("no-store, private")
    );
}

// ─── the uniform refusal ──────────────────────────────────────────────────────

#[tokio::test]
async fn consistency_refusals_are_indistinguishable_from_misses() {
    let Some(db) = TestDb::new("trrefuse").await else {
        return skipped("trrefuse");
    };
    let campaign = Uuid::new_v4();
    let (_, trace_id) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "refuse@example.id").await;

    // Wrong code attribution for this trace.
    let mismatched = router(&db.pool, canned(Some(Uuid::new_v4())));
    let (s1, h1, b1) = req(
        &mismatched,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}"),
        None,
        None,
    )
    .await;
    // Unknown code.
    let unknown_code = CannedTraceClick { unknown: true, ..canned(Some(campaign)) };
    let unknown = router(&db.pool, unknown_code);
    let (s2, h2, b2) = req(
        &unknown,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}"),
        None,
        None,
    )
    .await;
    // Unknown trace.
    let miss = router(&db.pool, canned(Some(campaign)));
    let (s3, h3, b3) = req(
        &miss,
        Method::GET,
        &format!("/r/abc123/m/{}", Uuid::new_v4()),
        None,
        None,
    )
    .await;
    // Non-uuid trace half — the uniform miss, not a 400.
    let (s4, _, b4) = req(
        &miss,
        Method::GET,
        "/r/abc123/m/not-a-uuid",
        None,
        None,
    )
    .await;
    // Out-of-shape path (an unrecognized suffix) — the SAME 404 body, not
    // the router fallback's empty one.
    let (s5, _, b5) = req(
        &miss,
        Method::GET,
        &format!("/r/abc123/m/{}/junk", Uuid::new_v4()),
        None,
        None,
    )
    .await;

    assert_eq!(s1, StatusCode::NOT_FOUND);
    assert_eq!(s2, StatusCode::NOT_FOUND);
    assert_eq!(s3, StatusCode::NOT_FOUND);
    assert_eq!(s4, StatusCode::NOT_FOUND, "a malformed trace half answers the miss");
    assert_eq!(s5, StatusCode::NOT_FOUND, "an out-of-shape path answers the miss");
    assert_eq!(b1, b2, "mismatch and unknown code are one body");
    assert_eq!(b2, b3, "unknown code and unknown trace are one body");
    assert_eq!(b3, b4, "malformed and unknown are one body");
    assert_eq!(b4, b5, "in-shape and out-of-shape misses are one body");
    assert_eq!(header_value(&h1, "cache-control"), Some("no-store, private"));
    assert_eq!(header_value(&h2, "cache-control"), Some("no-store, private"));
    assert_eq!(header_value(&h3, "cache-control"), Some("no-store, private"));

    // Nothing was stamped by any refused click.
    let (state, opened, clicked) = stamps(&db.pool, trace_id).await;
    assert_eq!(state, "outgoing");
    assert!(opened.is_none() && clicked.is_none());
}

#[tokio::test]
async fn uncomposed_click_port_fails_closed() {
    let Some(db) = TestDb::new("trclosed").await else {
        return skipped("trclosed");
    };
    let campaign = Uuid::new_v4();
    let (_, trace_id) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "closed@example.id").await;
    let app = uncomposed_router(&db.pool);

    let (click, _, _) = req(
        &app,
        Method::GET,
        &format!("/r/abc123/m/{trace_id}"),
        None,
        None,
    )
    .await;
    let (unsub, _, _) = req(
        &app,
        Method::POST,
        &format!("/r/abc123/m/{trace_id}/unsubscribe"),
        None,
        None,
    )
    .await;

    assert_eq!(click, StatusCode::NOT_FOUND, "no port composed: refused, not faked");
    assert_eq!(unsub, StatusCode::NOT_FOUND);
    let (state, opened, _) = stamps(&db.pool, trace_id).await;
    assert_eq!(state, "outgoing");
    assert!(opened.is_none(), "no stamp without the seam");
}

// ─── the unsubscribe leg ──────────────────────────────────────────────────────

#[tokio::test]
async fn unsubscribe_flips_every_targeted_audience_and_is_idempotent() {
    use backbone_mailing::application::service::subscription_write_service::SubscriptionWriteService;

    let Some(db) = TestDb::new("trunsub").await else {
        return skipped("trunsub");
    };
    let campaign = Uuid::new_v4();
    let subscriptions = SubscriptionWriteService::new(db.pool.clone());
    let audience_a = subscriptions.create_audience("route-case-a", false).await.expect("a");
    let audience_b = subscriptions.create_audience("route-case-b", false).await.expect("b");
    subscriptions
        .subscribe("reader@example.id", audience_a, Some("Reader"))
        .await
        .expect("subscribe");
    // The recipient is NOT a member of audience B — the flip reports it
    // truthfully instead of silently succeeding.

    let domain = serde_json::json!([
        ["mailing_audience_id", "in", [audience_a.to_string(), audience_b.to_string()]]
    ]);
    let (_, trace_id) = seed(&db.pool, Some(campaign), &domain, "reader@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    let (status, _, body) = req(
        &app,
        Method::POST,
        &format!("/r/abc123/m/{trace_id}/unsubscribe"),
        Some("application/x-www-form-urlencoded"),
        Some("reason=not-a-uuid"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an unparseable optional reason is ignored, not a 422");
    let first: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(first["count"], 2, "both targeted audiences are reported");
    let a_entry = first["audiences"]
        .as_array()
        .expect("audiences")
        .iter()
        .find(|v| v["audience"] == audience_a.to_string())
        .expect("audience a");
    let b_entry = first["audiences"]
        .as_array()
        .expect("audiences")
        .iter()
        .find(|v| v["audience"] == audience_b.to_string())
        .expect("audience b");
    assert_eq!(a_entry["changed"], true, "member audience flipped now");
    assert_eq!(a_entry["optedOut"], true);
    assert_eq!(b_entry["changed"], false, "non-member audience: nothing to flip");
    assert_eq!(b_entry["optedOut"], false);

    let first_stamp: String = sqlx::query_scalar(
        r#"SELECT s.opt_out_datetime::text
           FROM mailing.mailing_subscriptions s
           JOIN mailing.mailing_contacts c ON c.id = s.contact_id
           WHERE c.email = 'reader@example.id' AND s.mailing_audience_id = $1"#,
    )
    .bind(audience_a)
    .fetch_one(&db.pool)
    .await
    .expect("stamp");

    // The replay: same capability, same outcome shape — the FIRST opt-out
    // moment is the durable fact (changed=false, no restamp).
    let (status2, _, body2) = req(
        &app,
        Method::POST,
        &format!("/r/abc123/m/{trace_id}/unsubscribe"),
        None,
        None,
    )
    .await;
    assert_eq!(status2, StatusCode::OK);
    let second: serde_json::Value = serde_json::from_slice(&body2).expect("json 2");
    let a2 = second["audiences"]
        .as_array()
        .expect("audiences 2")
        .iter()
        .find(|v| v["audience"] == audience_a.to_string())
        .expect("audience a 2");
    assert_eq!(a2["changed"], false, "replay: already out");
    assert_eq!(a2["optedOut"], true);

    let second_stamp: String = sqlx::query_scalar(
        r#"SELECT s.opt_out_datetime::text
           FROM mailing.mailing_subscriptions s
           JOIN mailing.mailing_contacts c ON c.id = s.contact_id
           WHERE c.email = 'reader@example.id' AND s.mailing_audience_id = $1"#,
    )
    .bind(audience_a)
    .fetch_one(&db.pool)
    .await
    .expect("stamp 2");
    assert_eq!(first_stamp, second_stamp, "the first opt-out moment is durable");
}

#[tokio::test]
async fn unsubscribe_on_unbucketed_mailing_reports_zero_truthfully() {
    let Some(db) = TestDb::new("trunsub0").await else {
        return skipped("trunsub0");
    };
    let campaign = Uuid::new_v4();
    // An empty domain targets the whole (unbucketed) population: there is
    // no per-audience flip to perform and the response says so — count 0,
    // still 200, still capability-checked.
    let (_, trace_id) = seed(&db.pool, Some(campaign), &serde_json::json!([]), "all@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    let (status, _, body) = req(
        &app,
        Method::POST,
        &format!("/r/abc123/m/{trace_id}/unsubscribe"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let out: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(out["count"], 0);
    assert_eq!(out["audiences"].as_array().map(Vec::len), Some(0));
}

// ─── the per-code throttle ────────────────────────────────────────────────────

#[tokio::test]
async fn throttle_is_120_per_minute_per_code() {
    let Some(db) = TestDb::new("trthrottle").await else {
        return skipped("trthrottle");
    };
    let campaign = Uuid::new_v4();
    let (_, trace_id) =
        seed(&db.pool, Some(campaign), &serde_json::json!([]), "burn@example.id").await;
    let app = router(&db.pool, canned(Some(campaign)));

    // 120 requests on one code: all pass (the pixel leg — cheap and
    // idempotent against a live trace).
    let mut last_ok = StatusCode::OK;
    for _ in 0..120 {
        let (status, _, _) = req(
            &app,
            Method::GET,
            &format!("/r/burncode/m/{trace_id}/pixel.gif"),
            None,
            None,
        )
        .await;
        last_ok = status;
    }
    assert_eq!(last_ok, StatusCode::OK);

    // The 121st on the SAME code: 429, in the limiter's own shape, still
    // cache-defeated — and it never touched the database (the key is the
    // path's code segment, checked before any handler).
    let (over, headers, body) = req(
        &app,
        Method::GET,
        &format!("/r/burncode/m/{trace_id}/pixel.gif"),
        None,
        None,
    )
    .await;
    assert_eq!(over, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(header_value(&headers, "cache-control"), Some("no-store, private"));
    assert!(
        !body.is_empty(),
        "the 429 carries the limiter's JSON body, not an empty refusal"
    );

    // A DIFFERENT code has its own bucket: not 429.
    let (other, _, _) = req(
        &app,
        Method::GET,
        &format!("/r/othercode/m/{trace_id}/pixel.gif"),
        None,
        None,
    )
    .await;
    assert_eq!(other, StatusCode::OK, "per-code, not global");
}
