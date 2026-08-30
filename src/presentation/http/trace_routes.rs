//! The public mailing trace-route family (hand-written; user-owned).
//!
//! `/r/:code/m/:trace` + suffixes — the deferred MMF-H seam, now landed:
//! the per-recipient tracked-click redirect, the open pixel, and the
//! unsubscribe leg. Mailing owns this family (the recorded design call):
//! the link/code half resolves in the short-link module, so resolution and
//! the click mint are DELEGATED there through [`TraceClickPort`] (the host
//! composes the implementation; deny-by-default) while every trace stamp
//! goes through this module's own state helpers.
//!
//! ## Capability model (ADR-0019 action_link class)
//!
//! The (code, trace) pair in the path IS the authorization — no session, no
//! grant chain, anonymous by design. The pair is checked for CONSISTENCY:
//! the code's campaign attribution must equal the trace's denormalized
//! campaign (two unattributed halves are consistent). A wrong code, an
//! unknown code, an unknown trace, and a soft-deleted trace all answer the
//! SAME body — the refusal never says which half failed, so unauthenticated
//! enumeration cannot tell a miss from a mismatch. Trace ids are uuid v4
//! (unguessable capabilities); the per-code throttle below bounds
//! brute-force and burn attempts regardless.
//!
//! ## Throttle: 120/min per code
//!
//! The whole family is throttled 120 requests / 60 seconds PER CODE (the
//! ADR-0019 rate for public action routes — the same 120/60 numbers as the
//! short-link and rating mounts; here the limit key is the code path
//! segment, applied BEFORE any database work so an exhausted code 429s
//! identically whether it exists or not). The stock limiter middleware keys
//! on a request header, so this file drives the same
//! `backbone_rate_limit::RateLimitMiddleware` directly with a code-derived
//! key. The bucket is per-process (in-memory), the same posture as the
//! sibling mounts; a host fronting many replicas applies its proxy-level
//! limit on top.
//!
//! ## Cache discipline
//!
//! Every response this family emits — redirect, pixel, JSON, refusal, 429 —
//! carries `Cache-Control: no-store, private`, applied once in the
//! middleware so no leg can forget it.
//!
//! ## Leg shapes
//!
//! | Method | Path | Behavior |
//! |---|---|---|
//! | GET | /r/:code/m/:trace | consistency → delegate resolve+click mint → stamp `set_opened`+`set_clicked` → **301** to the utm-injected target |
//! | GET | /r/:code/m/:trace/pixel.gif | consistency (read-only probe) → `set_opened` → the identical 1×1 GIF on EVERY outcome |
//! | POST | /r/:code/m/:trace/unsubscribe | consistency → per-audience opt-out flip through the subscription write service → idempotent JSON outcome |
//!
//! The click answers **301**, the upstream-faithful status for this family
//! (the short-link module's own bare `/r/:code` mount chose 302 for its
//! target-edit-propagation reason — a deliberate divergence between the two
//! mounts; this family keeps the ported shape and adds `no-store` on its
//! own responses).
//!
//! The pixel answers the identical GIF whether or not anything was stamped:
//! an open pixel has no failure surface an email client could act on, and
//! byte-identical responses are the strongest no-oracle shape (misses and
//! internal failures surface through tracing, not the wire).
//!
//! The unsubscribe leg is mailing's own opt-out — the per-audience
//! subscription flip — and is NOT the RFC 8058 digest form; it shares the
//! action-link CLASS (POST form, capability path, idempotent). The form's
//! optional `reason` field is an opt-out reason id.
//!
//! Bare-URL clicks stay invisible by design (the MSM-B-9 ruling): only
//! links rewritten into this `/r/:code/m/:trace` shape record anything; a
//! plain URL in a mailing body is untracked and that is documented
//! behavior, not a defect. Upstream's bot-skip heuristic on the click leg
//! is not ported — visitor identity here is the dedup grain the short-link
//! mint already applies, not a user-agent allowlist.
//!
//! Mount BARE at the site root (next to the short-link module's own
//! `/r/:code` mount — the path shapes are distinct and do not shadow each
//! other): `root.merge(mailing.public_trace_routes())`. There is no
//! `/api/v1/mailing` prefix here — these are public capability routes, not
//! the guarded CRUD surface.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use backbone_rate_limit::{InMemoryStorage, RateLimitMiddleware};

use crate::application::service::trace_click_ports::TraceClickSlot;
use crate::application::service::trace_route_service::{TraceRouteError, TraceRouteService};

/// The per-code throttle: 120 requests…
pub const TRACE_RATE_LIMIT_MAX: u64 = 120;
/// …per 60-second window (ADR-0019's public action-route rate).
pub const TRACE_RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// The shared route state: the orchestration service + the family's own
/// limiter (per-code keyed — see [`code_throttle_key`]).
#[derive(Clone)]
pub struct TraceRouteState {
    service: Arc<TraceRouteService>,
    limiter: Arc<RateLimitMiddleware<InMemoryStorage>>,
}

/// The 1×1 transparent GIF every pixel call answers (upstream's
/// `blank.gif`, inlined — no asset fetch on the hot path).
const PIXEL_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0xff,
    0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

/// The uniform public refusal — unknown code, unknown trace, inconsistent
/// pair, and internal trouble all land here, one body, one status (the
/// short-link mount's shape; internals surface through tracing).
fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Fold a service error into the public surface: the two "pair" refusals
/// answer the uniform 404 quietly; internal-class failures ALSO answer the
/// uniform 404 but trace their detail first — a composition or data bug
/// must not hide, it just must not leak.
fn trace_route_err(context: &str, e: TraceRouteError) -> Response {
    match e {
        TraceRouteError::UnknownTrace | TraceRouteError::Inconsistent => not_found(),
        other => {
            tracing::warn!(context, error = %other, "trace route refused (internal class)");
            not_found()
        }
    }
}

/// The visitor IP as the host saw it (the short-link mount's extraction):
/// the first hop of `X-Forwarded-For` when fronted by a proxy, else none —
/// the click dedup grain degrades to "unknown", never a mint storm.
fn visitor_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The geo header the click grain optionally records.
fn visitor_country(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-visitor-country")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
}

// ─── the click leg ────────────────────────────────────────────────────────────

async fn click_redirect(
    State(state): State<TraceRouteState>,
    headers: HeaderMap,
    Path((code, trace_raw)): Path<(String, String)>,
) -> Response {
    // The trace half parses HERE, not in the extractor: a non-uuid trace
    // must answer the uniform miss, not a 400 that would split the
    // refusal surface.
    let Ok(trace_id) = Uuid::parse_str(&trace_raw) else {
        return not_found();
    };
    match state
        .service
        .record_click(
            &code,
            trace_id,
            visitor_ip(&headers).as_deref(),
            visitor_country(&headers).as_deref(),
        )
        .await
    {
        Ok(target) => (
            StatusCode::MOVED_PERMANENTLY,
            [(header::LOCATION, target)],
        )
            .into_response(),
        Err(e) => trace_route_err("click", e),
    }
}

// ─── the open-pixel leg ───────────────────────────────────────────────────────

async fn open_pixel(
    State(state): State<TraceRouteState>,
    Path((code, trace_raw)): Path<(String, String)>,
) -> Response {
    let gif = || {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/gif")],
            PIXEL_GIF,
        )
            .into_response()
    };
    // Every outcome answers the identical GIF — no oracle, nothing for an
    // email client to render as an error. Stamps happen only on the
    // consistent live pair.
    let Ok(trace_id) = Uuid::parse_str(&trace_raw) else {
        return gif();
    };
    match state.service.record_open(&code, trace_id).await {
        Ok(()) => gif(),
        Err(e) => {
            trace_route_err("pixel", e);
            gif()
        }
    }
}

// ─── the unsubscribe leg ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UnsubscribeForm {
    /// Optional opt-out reason id (the reasons master's uuid).
    pub reason: Option<Uuid>,
}

async fn unsubscribe(
    State(state): State<TraceRouteState>,
    Path((code, trace_raw)): Path<(String, String)>,
    form: Option<Form<UnsubscribeForm>>,
) -> Response {
    let Ok(trace_id) = Uuid::parse_str(&trace_raw) else {
        return not_found();
    };
    // A body that fails to parse (wrong content type, non-uuid reason)
    // is treated as no reason — the POST form is the effect, the reason
    // is optional metadata.
    let reason = form.and_then(|Form(f)| f.reason);
    match state.service.unsubscribe(&code, trace_id, reason).await {
        Ok(outcome) => {
            let audiences: Vec<_> = outcome
                .iter()
                .map(|a| {
                    json!({
                        "audience": a.audience_id,
                        "changed": a.changed,
                        "optedOut": a.opted_out,
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(json!({
                    "trace": trace_id,
                    "audiences": audiences,
                    // A mailing that targeted an unbucketed population has
                    // no per-audience flip to perform — the count says so
                    // truthfully; it is not a silent success (the sender's
                    // preferences surface is the right leg for that case).
                    "count": outcome.len(),
                })),
            )
                .into_response()
        }
        Err(e) => trace_route_err("unsubscribe", e),
    }
}

// ─── the per-code throttle + cache discipline ────────────────────────────────

/// The throttle key for one request: the code path segment. The family's
/// paths are `/r/<code>/m/<trace>…` (mount-point prefixes tolerated — the
/// first `r` segment anchors the scan); anything else on this router
/// buckets together under `other`.
pub fn code_throttle_key(path: &str) -> String {
    let mut segments = path.split('/');
    while let Some(segment) = segments.next() {
        if segment == "r" {
            if let Some(code) = segments.next() {
                if !code.is_empty() {
                    return format!("mailing-trace:{code}");
                }
            }
            break;
        }
    }
    "mailing-trace:other".to_string()
}

/// Cache-defeat every response the family emits, then hand it on.
fn no_store(mut res: Response) -> Response {
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store, private"),
    );
    res
}

/// The per-code throttle middleware: checks the limiter BEFORE any handler
/// (so a throttled request touches no database and cannot leak whether the
/// code exists), 429s in the limiter's own response shape, and fails open
/// on limiter trouble — the stock middleware's posture.
async fn per_code_throttle(
    State(limiter): State<Arc<RateLimitMiddleware<InMemoryStorage>>>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, Response> {
    let key = code_throttle_key(req.uri().path());
    match limiter.check(&key).await {
        Ok(resp) if resp.allowed => Ok(no_store(next.run(req).await)),
        Ok(resp) => {
            let mut res = Json(resp).into_response();
            *res.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            Err(no_store(res))
        }
        Err(e) => {
            tracing::error!("trace-route rate limit check failed for key {key}: {e}");
            Ok(no_store(next.run(req).await))
        }
    }
}

// ─── the composers ────────────────────────────────────────────────────────────

/// The PUBLIC trace-route family — a BARE mount (the (code, trace) pair is
/// the capability), throttled 120/min per code, every response
/// cache-defeated. Mount at the site ROOT next to the short-link module's
/// own `/r/:code` group (distinct path shapes — both can coexist in one
/// router).
pub fn public_composer(service: Arc<TraceRouteService>) -> Router {
    let state = TraceRouteState {
        service,
        limiter: backbone_rate_limit::middleware(
            TRACE_RATE_LIMIT_MAX,
            TRACE_RATE_LIMIT_WINDOW_SECS,
        ),
    };
    Router::new()
        .route("/r/:code/m/:trace", get(click_redirect))
        .route("/r/:code/m/:trace/pixel.gif", get(open_pixel))
        .route("/r/:code/m/:trace/unsubscribe", post(unsubscribe))
        // Out-of-shape paths answer the SAME 404 body as in-shape misses —
        // axum's default fallback would answer an EMPTY 404 and split the
        // refusal surface by path shape.
        .fallback(|| async { not_found() })
        .route_layer(axum::middleware::from_fn_with_state(
            state.limiter.clone(),
            per_code_throttle,
        ))
        .with_state(state)
}

/// Convenience: build the service + family straight from a pool and a
/// composed click port slot (the module's `public_trace_routes()` wires
/// this for the host).
pub fn public_trace_routes(pool: sqlx::PgPool, clicks: TraceClickSlot) -> Router {
    public_composer(Arc::new(TraceRouteService::new(pool, clicks)))
}
