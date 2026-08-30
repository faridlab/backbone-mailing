//! Shared harness: one DISPOSABLE scratch database per test.
//!
//! The suite never runs against a shared DB: each test mints
//! `mailing_seat_<marker>_<hex>` on the local test Postgres (5433,
//! postgres/postgres), applies BOTH modules' migrations with a raw SQL file
//! runner (the two modules share the `2026042622000N` version numbering, so
//! `sqlx::migrate` would trip VersionMismatch across modules — per-module
//! sorted order is what matters, and each module's files are independent),
//! migrates the outbox crate's table (mail's public services stage bus
//! events into `messaging.outbox_events`), runs, and drops the database.
//!
//! `TestDb::dispose()` is the explicit teardown; `Drop` is the leak guard
//! (best-effort DROP on a throwaway runtime) for tests that panic.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

/// The scratch Postgres every test database is born on and dropped from.
pub const SCRATCH_ADMIN_URL: &str = "postgres://postgres:postgres@localhost:5433/postgres";

fn admin_url() -> String {
    std::env::var("MAILING_TEST_ADMIN_URL").unwrap_or_else(|_| SCRATCH_ADMIN_URL.into())
}

/// One disposable scratch database, migrations applied.
pub struct TestDb {
    pub pool: PgPool,
    name: String,
    admin: PgPool,
}

impl TestDb {
    /// Mint a fresh scratch DB (or `None` when the scratch Postgres is
    /// unreachable — the caller skips loudly rather than faking results,
    /// and every failure branch here prints WHY).
    pub async fn new(marker: &str) -> Option<Self> {
        let admin_url = admin_url();
        let admin = match PgPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&admin_url)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("SKIPPED-DB: {marker}: admin connect to {admin_url} failed: {e}");
                return None;
            }
        };
        let suffix: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let name = format!("mailing_seat_{marker}_{suffix}");
        // Disposable by construction: a stale DB of the same name goes first.
        let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#))
            .execute(&admin)
            .await;
        if let Err(e) = sqlx::query(&format!("CREATE DATABASE \"{}\"", name))
            .execute(&admin)
            .await
        {
            eprintln!("SKIPPED-DB: {marker}: create database {name} failed: {e}");
            return None;
        }
        // Splice ONLY the trailing path segment — a plain substring replace
        // would also rewrite the scheme/username/password when they match
        // the admin database's name.
        let db_url = match admin_url.rfind('/') {
            Some(i) => format!("{}{}", &admin_url[..=i], name),
            None => admin_url.clone(),
        };
        let pool = match PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("SKIPPED-DB: {marker}: connect to {db_url} failed: {e}");
                return None;
            }
        };
        if !apply_module_migrations(&pool, marker).await {
            return None;
        }
        match backbone_outbox::outbox::migrate(&pool, "messaging").await {
            Ok(()) => Some(Self { pool, name, admin }),
            Err(e) => {
                eprintln!("SKIPPED-DB: {marker}: outbox migrate failed: {e}");
                None
            }
        }
    }

    /// Explicit teardown: drop the scratch database entirely.
    pub async fn dispose(self) {
        self.drop_db().await;
    }

    async fn drop_db(&self) {
        // FORCE: connected test pool may still hold an idle session.
        let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{}" WITH (FORCE)"#, self.name))
            .execute(&self.admin)
            .await;
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let name = self.name.clone();
        let url = admin_url();
        // Leak-guard teardown for panicking tests; dispose() is the happy path.
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(async move {
                    if let Ok(admin) = sqlx::PgPool::connect(&url).await {
                        let _ = sqlx::query(&format!(
                            r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#
                        ))
                        .execute(&admin)
                        .await;
                    }
                });
            }
        });
    }
}

/// Apply this module's migrations, then the sibling mail module's (the
/// enqueue seam's schema). Raw SQL file runner — see the module docs.
async fn apply_module_migrations(pool: &PgPool, marker: &str) -> bool {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dirs = [
        format!("{manifest}/migrations"),
        format!("{manifest}/../backbone-mail/migrations"),
    ];
    for dir in dirs {
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
                eprintln!(
                    "SKIPPED-DB: {marker}: migration {} failed: {e}",
                    file.display()
                );
                return false;
            }
        }
    }
    true
}

// ── seeding helpers (direct SQL — tests may bypass the repositories) ────────

/// Seed an audience, N subscribed contacts, and return (audience_id, emails).
pub async fn seed_audience(
    pool: &PgPool,
    audience_name: &str,
    n: usize,
) -> (uuid::Uuid, Vec<String>) {
    let audience_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mailing.mailing_audiences (id, name) VALUES ($1, $2)"#,
    )
    .bind(audience_id)
    .bind(audience_name)
    .execute(pool)
    .await
    .expect("seed audience");
    let mut emails = Vec::with_capacity(n);
    for i in 0..n {
        let email = format!("member{i:05}.{audience_name}@example.id");
        let contact_id = uuid::Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO mailing.mailing_contacts (id, email, name)
               VALUES ($1, $2, $3)"#,
        )
        .bind(contact_id)
        .bind(&email)
        .bind(format!("Member {i}"))
        .execute(pool)
        .await
        .expect("seed contact");
        sqlx::query(
            r#"INSERT INTO mailing.mailing_subscriptions (contact_id, mailing_audience_id)
               VALUES ($1, $2)"#,
        )
        .bind(contact_id)
        .bind(audience_id)
        .execute(pool)
        .await
        .expect("seed subscription");
        emails.push(email);
    }
    (audience_id, emails)
}

/// The DB-required marker — a missing scratch Postgres on 5433 is an
/// environment failure, not a pass: fail the test instead of printing and
/// returning, so a DB-less run can never produce a vacuous green suite.
pub fn skipped(marker: &str) -> ! {
    panic!(
        "SKIPPED-DB: {marker} — no scratch Postgres on 5433; refusing to fake results \
         (these tests require the local test Postgres to be up)"
    );
}

/// Count helper.
pub async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .expect("count probe")
}
