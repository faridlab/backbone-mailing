//! `SubscriptionRepository` — the hand-written SQL behind the subscription
//! opt-out split (hand-authored, user-owned; see `metaphor.codegen.yaml`).
//!
//! The split: `mailing_subscriptions.opt_out` is a FLIP, never a DELETE —
//! opting out stamps `opt_out_datetime` (DB clock) and optionally
//! `opt_out_reason_id`; re-subscribing clears both and flips back. The row
//! survives forever, so the audience keeps its history and OPT-OUT-WINS can
//! be evaluated per contact across every list (the recorded deviation from
//! Odoo's opt-in-wins: one opt-out anywhere suppresses the contact
//! everywhere, evaluated at send time).
//!
//! The partial UNIQUE `(contact_id, mailing_audience_id) WHERE
//! (metadata->>'deleted_at') IS NULL` makes subscribe idempotent via ON
//! CONFLICT DO UPDATE — converging a second subscribe onto the existing live
//! row (and clearing any standing opt-out) instead of minting a duplicate.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

/// One resolved audience recipient (the send engine's target grain).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AudienceRecipient {
    pub contact_id: Uuid,
    pub email: String,
    pub name: Option<String>,
}

/// One live subscription row (the split's read model).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SubscriptionRow {
    pub id: Uuid,
    pub contact_id: Uuid,
    pub mailing_audience_id: Uuid,
    pub opt_out: bool,
    pub opt_out_datetime: Option<DateTime<Utc>>,
    pub opt_out_reason_id: Option<Uuid>,
}

/// Hand-written subscription/audience SQL. Services orchestrate; this holds
/// SQL.
pub struct SubscriptionRepository;

impl SubscriptionRepository {
    /// Idempotent subscribe / re-subscribe. ON CONFLICT converges onto the
    /// existing live row AND CLEARS a standing opt-out — a re-subscribe is
    /// the only sanctioned way out of an opt-out.
    pub async fn subscribe(
        conn: &mut PgConnection,
        contact_id: Uuid,
        audience_id: Uuid,
    ) -> Result<SubscriptionRow, sqlx::Error> {
        sqlx::query_as::<_, SubscriptionRow>(
            r#"INSERT INTO mailing.mailing_subscriptions (contact_id, mailing_audience_id, opt_out)
               VALUES ($1, $2, FALSE)
               ON CONFLICT (contact_id, mailing_audience_id)
                  WHERE (metadata->>'deleted_at') IS NULL
               DO UPDATE SET opt_out = FALSE,
                             opt_out_datetime = NULL,
                             opt_out_reason_id = NULL
               RETURNING id, contact_id, mailing_audience_id, opt_out,
                         opt_out_datetime, opt_out_reason_id"#,
        )
        .bind(contact_id)
        .bind(audience_id)
        .fetch_one(&mut *conn)
        .await
    }

    /// The opt-out flip. `now()` is the DATABASE clock (the same clock the
    /// auto-blacklist window reads — no host-clock skew between the two
    /// facts). Guarded on `NOT opt_out` so a repeated unsubscribe never
    /// restamps the datetime (the FIRST opt-out moment is the durable fact).
    /// Returns the row when the flip happened; None when it was already out.
    pub async fn opt_out(
        conn: &mut PgConnection,
        contact_id: Uuid,
        audience_id: Uuid,
        reason_id: Option<Uuid>,
    ) -> Result<Option<SubscriptionRow>, sqlx::Error> {
        sqlx::query_as::<_, SubscriptionRow>(
            r#"UPDATE mailing.mailing_subscriptions
               SET opt_out = TRUE,
                   opt_out_datetime = now(),
                   opt_out_reason_id = $3
               WHERE contact_id = $1 AND mailing_audience_id = $2
                 AND NOT opt_out
                 AND (metadata->>'deleted_at') IS NULL
               RETURNING id, contact_id, mailing_audience_id, opt_out,
                         opt_out_datetime, opt_out_reason_id"#,
        )
        .bind(contact_id)
        .bind(audience_id)
        .bind(reason_id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Cross-list OPT-OUT-WINS probe, evaluated at send time: does this
    /// email carry ANY live opt-out on ANY audience? (The recorded deviation
    /// from Odoo's per-list opt-in-wins.)
    pub async fn opted_out_anywhere(
        conn: &mut PgConnection,
        email: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS (
                   SELECT 1
                   FROM mailing.mailing_subscriptions s
                   JOIN mailing.mailing_contacts c ON c.id = s.contact_id
                   WHERE lower(c.email) = lower($1)
                     AND s.opt_out
                     AND (s.metadata->>'deleted_at') IS NULL
                     AND (c.metadata->>'deleted_at') IS NULL
               )"#,
        )
        .bind(email)
        .fetch_one(&mut *conn)
        .await
    }

    /// One live subscription by (contact, audience).
    pub async fn find_live(
        conn: &mut PgConnection,
        contact_id: Uuid,
        audience_id: Uuid,
    ) -> Result<Option<SubscriptionRow>, sqlx::Error> {
        sqlx::query_as::<_, SubscriptionRow>(
            r#"SELECT id, contact_id, mailing_audience_id, opt_out,
                      opt_out_datetime, opt_out_reason_id
               FROM mailing.mailing_subscriptions
               WHERE contact_id = $1 AND mailing_audience_id = $2
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(contact_id)
        .bind(audience_id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Audience members for a plain audience domain term: live, subscribed
    /// (flip FALSE), non-deleted contacts — ordered for deterministic batch
    /// shapes.
    pub async fn audience_recipients(
        conn: &mut PgConnection,
        audience_id: Uuid,
        limit: i64,
        after_email: Option<&str>,
    ) -> Result<Vec<AudienceRecipient>, sqlx::Error> {
        sqlx::query_as::<_, AudienceRecipient>(
            r#"SELECT c.id AS contact_id, c.email, c.name
               FROM mailing.mailing_subscriptions s
               JOIN mailing.mailing_contacts c ON c.id = s.contact_id
               WHERE s.mailing_audience_id = $1
                 AND NOT s.opt_out
                 AND (s.metadata->>'deleted_at') IS NULL
                 AND (c.metadata->>'deleted_at') IS NULL
                 AND ($2::text IS NULL OR c.email > $2)
               ORDER BY c.email
               LIMIT $3"#,
        )
        .bind(audience_id)
        .bind(after_email)
        .bind(limit)
        .fetch_all(&mut *conn)
        .await
    }

    /// R-M7 archive guard: mailings still in flight whose canonical domain
    /// JSON cites this audience. The canonical form stores each term as
    /// `{"field":"mailing_audience_id","op":"=","value":"<uuid>"}` — the
    /// probe flattens every jsonb value in the array and looks for the uuid
    /// text anywhere in the set, so `=`, `in`, and `not in` citations all
    /// block the archive.
    pub async fn audience_in_use_by_mailings(
        conn: &mut PgConnection,
        audience_id: Uuid,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"SELECT m.id
               FROM mailing.mailings m,
                    jsonb_array_elements(m.mailing_domain) term,
                    jsonb_each(term) kv
               WHERE m.state IN ('draft', 'in_queue', 'sending')
                 AND (m.metadata->>'deleted_at') IS NULL
                 AND kv.value #>> '{}' = $1::text
               GROUP BY m.id"#,
        )
        .bind(audience_id.to_string())
        .fetch_all(&mut *conn)
        .await
    }

    /// Soft-delete one live subscription row (admin removal — distinct from
    /// the opt-out flip: removes membership entirely, keeps the audit trail).
    pub async fn remove(
        conn: &mut PgConnection,
        contact_id: Uuid,
        audience_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_subscriptions
               SET metadata = metadata
                   || jsonb_build_object('deleted_at', to_jsonb(now()))
               WHERE contact_id = $1 AND mailing_audience_id = $2
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(contact_id)
        .bind(audience_id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Ensure the default opt-out reason catalog exists (idempotent seed —
    /// the unsubscribe route records a reason even on a fresh install).
    /// Mirrors the Odoo mass_mailing default reasons.
    pub async fn ensure_default_reasons(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
        for (idx, (name, is_feedback)) in [
            ("Not Interested", false),
            ("Unsubscribed", false),
            ("Too many emails", true),
            ("Never subscribed", true),
            ("Other", true),
        ]
        .into_iter()
        .enumerate()
        {
            sqlx::query(
                r#"INSERT INTO mailing.mailing_opt_out_reasons (name, sequence, is_feedback)
                   SELECT $1, $2, $3
                   WHERE NOT EXISTS (
                       SELECT 1 FROM mailing.mailing_opt_out_reasons
                       WHERE name = $1 AND (metadata->>'deleted_at') IS NULL
                   )"#,
            )
            .bind(name)
            .bind((idx as i32 + 1) * 10)
            .bind(is_feedback)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    /// Default unsubscribe reason id (the route's fallback when the contact
    /// picks nothing).
    pub async fn default_reason_id(conn: &mut PgConnection) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id FROM mailing.mailing_opt_out_reasons
               WHERE name = 'Unsubscribed' AND (metadata->>'deleted_at') IS NULL
               ORDER BY sequence LIMIT 1"#,
        )
        .fetch_optional(&mut *conn)
        .await
    }

    /// Soft-delete an audience — the archive verb's write, AFTER the R-M7
    /// in-use guard passed.
    pub async fn soft_delete_audience(
        conn: &mut PgConnection,
        audience_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_audiences
               SET metadata = metadata
                   || jsonb_build_object('deleted_at', to_jsonb(now()))
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(audience_id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Live member count (subscribed, not opted out) — the archive verb's
    /// observability echo.
    pub async fn active_member_count(
        conn: &mut PgConnection,
        audience_id: Uuid,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar::<_, i64>(
            r#"SELECT count(*) FROM mailing.mailing_subscriptions s
               JOIN mailing.mailing_contacts c ON c.id = s.contact_id
               WHERE s.mailing_audience_id = $1
                 AND NOT s.opt_out
                 AND (s.metadata->>'deleted_at') IS NULL
                 AND (c.metadata->>'deleted_at') IS NULL"#,
        )
        .bind(audience_id)
        .fetch_one(&mut *conn)
        .await
    }

    /// Find a live contact id by email (the unsubscribe route's seam: it
    /// arrives with an email, not a contact id).
    pub async fn contact_id_by_email(
        conn: &mut PgConnection,
        email: &str,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id FROM mailing.mailing_contacts
               WHERE lower(email) = lower($1)
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(email)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Upsert a contact by email (partial-unique converge) — returns the
    /// contact id. The subscribe verb's first half.
    pub async fn upsert_contact(
        conn: &mut PgConnection,
        email: &str,
        name: Option<&str>,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_contacts (email, name)
               VALUES ($1, $2)
               ON CONFLICT (email) WHERE (metadata->>'deleted_at') IS NULL
               DO UPDATE SET name = COALESCE(EXCLUDED.name, mailing.mailing_contacts.name)
               RETURNING id"#,
        )
        .bind(email)
        .bind(name)
        .fetch_one(&mut *conn)
        .await
    }

    /// Create an audience (returns its id).
    pub async fn insert_audience(
        conn: &mut PgConnection,
        id: Uuid,
        name: &str,
        is_public: bool,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_audiences (id, name, is_public)
               VALUES ($1, $2, $3) RETURNING id"#,
        )
        .bind(id)
        .bind(name)
        .bind(is_public)
        .fetch_one(&mut *conn)
        .await
    }
}
