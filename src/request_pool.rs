//! The composer-installed request pool and org scope, made visible to the mailing services.
//!
//! ADR-0029's pool law: modules are tenant-agnostic and the composing service owns routing to
//! the right database. The composer's tenant router inserts the request's `PgPool` into the
//! request extensions; [`bind_request_pool`] copies it into a task-local for the handler future,
//! and every mailing service resolves its database through [`current`] with its composed pool as
//! the fallback — so a mount without tenant routing behaves exactly as before, and a request
//! under a tenant mount reads and writes that tenant's database.
//!
//! The services open their own transactions, which do not ride the request connection the
//! composer's org scope is bound on. [`begin`] therefore binds the ambient org scope onto each
//! transaction it opens, so a composer that fences the mailing tables sees its fence honored on
//! every hand-written statement. With no scope bound, nothing is bound and nothing is invented.

use axum::{body::Body, http::Request, middleware::Next, response::Response};
use sqlx::{PgPool, Postgres, Transaction};

tokio::task_local! {
    static REQUEST_POOL: PgPool;
}

/// The pool this request's tenant router installed, if any.
pub fn current() -> Option<PgPool> {
    REQUEST_POOL.try_with(|pool| pool.clone()).ok()
}

/// The request's pool, else the composed one.
pub fn resolve(fallback: &PgPool) -> PgPool {
    current().unwrap_or_else(|| fallback.clone())
}

/// Open a transaction on the request's database, with the ambient org scope (when the composer
/// bound one) set on it transaction-locally.
pub async fn begin(fallback: &PgPool) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = resolve(fallback).begin().await?;
    if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
        backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
    }
    Ok(tx)
}

/// Middleware that binds the composer-inserted pool for the whole handler call. Requests
/// without an inserted pool pass straight through.
pub async fn bind_request_pool(req: Request<Body>, next: Next) -> Response {
    match req.extensions().get::<PgPool>().cloned() {
        Some(pool) => REQUEST_POOL.scope(pool, next.run(req)).await,
        None => next.run(req).await,
    }
}

/// Run a future with a pool bound as this module's request pool — for host jobs and consumers
/// that call the mailing services outside an HTTP request.
pub async fn with_pool_scope<F: std::future::Future<Output = O>, O>(pool: PgPool, fut: F) -> O {
    REQUEST_POOL.scope(pool, fut).await
}
