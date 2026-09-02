//! `MailingSendRepository` — the hand-written SQL behind the send engine and
//! the mailing lifecycle verbs (hand-authored, user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! Services orchestrate; this holds SQL. The engine's flow and its invariants:
//!
//! - **Claim** is `FOR UPDATE SKIP LOCKED` (ADR-0020 / MMB-4): two sweep
//!   workers never claim the same mailing in the same instant. The lock is
//!   held only for claim + state flip; cross-sweep overlap is contained by
//!   the mint fence + seen-list, not by long locks — the two-workers probe
//!   proves exactly that containment.
//! - **Recipient resolution** compiles the persisted domain into ONE
//!   parameterized query over `mailing_contacts` (+ subscription membership
//!   EXISTS/NOT EXISTS for audience terms). Every value is a bind — the
//!   compiled field/op whitelist in the service is the injection fence.
//! - **Suppression probes are batched** (one query per sweep per layer, keyed
//!   on the resolved email set) — never N+1 per recipient.
//! - **Auto-blacklist** runs entirely on the DATABASE clock (`now()`), the
//!   same clock the audit triggers stamp `metadata->>'created_at'` with, so
//!   the 13-week window cannot skew against its own fact source.
//! - **A/B promotion** ranks done siblings by the persisted metric and
//!   stamps `winner_mailing_id` + `completed` idempotently (guarded UPDATE —
//!   a second sweep matches zero rows).

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use sqlx::postgres::PgArguments;
use sqlx::Arguments;
use uuid::Uuid;

/// One term of a compiled mailing domain (the repository-executable shape).
/// The service OWNS parsing/validation (refuse-loudly: `DomainInvalid`);
/// this type can only express what the whitelist already admitted.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainTerm {
    pub field: DomainField,
    pub op: DomainOp,
    /// One value for =/!=/like/not like; many for in/not in.
    pub values: Vec<String>,
}

/// Whitelisted domain fields — the exact set the resolver SQL knows how to
/// bind. Anything else is rejected at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainField {
    Email,
    Name,
    FirstName,
    LastName,
    CompanyName,
    CountryCode,
    /// Audience membership — resolves through `mailing_subscriptions`.
    MailingAudienceId,
}

impl DomainField {
    /// The contact column each field binds to (None for the audience seam).
    fn column(self) -> Option<&'static str> {
        match self {
            DomainField::Email => Some("c.email"),
            DomainField::Name => Some("c.name"),
            DomainField::FirstName => Some("c.first_name"),
            DomainField::LastName => Some("c.last_name"),
            DomainField::CompanyName => Some("c.company_name"),
            DomainField::CountryCode => Some("c.country_code"),
            DomainField::MailingAudienceId => None,
        }
    }
}

/// Whitelisted domain operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainOp {
    Eq,
    Ne,
    In,
    NotIn,
    Like,
    NotLike,
}

/// A fully-parsed, whitelisted domain — the only shape the resolver accepts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompiledDomain {
    pub terms: Vec<DomainTerm>,
}

/// One resolved send target (both contact- and party-resolved paths yield
/// this).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ResolvedRecipient {
    pub recipient_id: Uuid,
    pub email: String,
}

/// The mailing row the claim locks and the engine drives.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClaimedMailing {
    pub id: Uuid,
    pub subject: String,
    pub body_html: String,
    pub email_from: String,
    pub reply_to: Option<String>,
    pub mailing_domain: serde_json::Value,
    pub target_model: String,
    /// Channel selector ('mail' | 'sms') — the sweep branches on it: mail
    /// completes synchronously at walk end; sms marks the walk complete and
    /// leaves done to the delivery-tracker pump.
    pub mailing_type: String,
    /// Whether the sms walk already ended (the durable `sms_walk_complete`
    /// metadata marker). A walked-out sms mailing is NOT send work: the
    /// drive leaves it untouched and the delivery-tracker pump (the sweep's
    /// last step) owns its completion.
    pub sms_walk_done: bool,
    pub use_exclusion_list: bool,
    pub campaign_id: Option<Uuid>,
    pub ab_testing_enabled: bool,
    pub ab_testing_pc: i32,
    pub ab_test_id: Option<Uuid>,
    pub state: String,
}

/// Hand-written send-engine SQL.
pub struct MailingSendRepository;

impl MailingSendRepository {
    // ── claim + lifecycle ─────────────────────────────────────────────────────

    /// The SKIP LOCKED claim: due mailings (in_queue, or sending with pass
    /// budget left from a capped earlier pass), schedule honored. FRESH
    /// `in_queue` work is preferred over `sending` resumes — a second worker
    /// starting mid-drive claims the untouched queue, not the mailing the
    /// first worker is actively driving (resumes are only taken when no
    /// fresh work remains). Oldest first within each class. Locks are held
    /// for claim + flip only — overlap across sweeps is contained by the
    /// mint fence, not by this lock.
    pub async fn claim_due_mailings(
        conn: &mut PgConnection,
        limit: i64,
    ) -> Result<Vec<ClaimedMailing>, sqlx::Error> {
        sqlx::query_as::<_, ClaimedMailing>(
            r#"SELECT id, subject, body_html, email_from, reply_to, mailing_domain,
                      target_model::text AS target_model,
                      mailing_type::text AS mailing_type,
                      (metadata ? 'sms_walk_complete') AS sms_walk_done,
                      use_exclusion_list,
                      campaign_id, ab_testing_enabled, ab_testing_pc, ab_test_id,
                      state::text AS state
               FROM mailing.mailings
               WHERE state IN ('in_queue', 'sending')
                 AND (metadata->>'deleted_at') IS NULL
                 AND (schedule_date IS NULL OR schedule_date <= now())
               ORDER BY (state = 'in_queue') DESC,
                        (metadata->>'created_at') NULLS LAST, id
               LIMIT $1
               FOR UPDATE SKIP LOCKED"#,
        )
        .bind(limit)
        .fetch_all(&mut *conn)
        .await
    }

    /// State-guarded flip `in_queue → sending` (the launch/pickup edge).
    /// Zero rows = someone else already flipped; read as a skip.
    pub async fn flip_sending(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'sending'
               WHERE id = $1 AND state = 'in_queue'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// PARK a mailing whose domain failed to parse AT SEND TIME (refuse
    /// loudly, never a silent zero-recipient sweep): flip to `sending` (so no
    /// other worker re-claims it into another attempt this instant) and
    /// record the parse failure in `metadata.send_error`. Zero sends. A human
    /// fixes the domain and re-queues.
    pub async fn park_mailing(
        conn: &mut PgConnection,
        id: Uuid,
        reason: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'sending',
                   metadata = metadata || jsonb_build_object(
                       'send_error', to_jsonb($2),
                       'send_error_at', to_jsonb(now()))
               WHERE id = $1 AND state IN ('in_queue', 'sending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .bind(reason)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Clear a send_error park (the re-queue path after a domain fix).
    pub async fn unpark_mailing(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'in_queue',
                   metadata = metadata - 'send_error' - 'send_error_at'
               WHERE id = $1 AND state = 'sending'
                 AND metadata ? 'send_error'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// The complete edge: `sending → done`, stamp `sent_date` (once — the
    /// KPI fact), and raise `kpi_mail_required` when this was the first
    /// completion.
    pub async fn complete_mailing(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'done',
                   sent_date = now(),
                   kpi_mail_required = kpi_mail_required OR sent_date IS NULL
               WHERE id = $1 AND state = 'sending'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// The SMS channel's walk-end marker: the send walk finished its last
    /// pass (every trace minted, every enqueue handed to the channel), so
    /// DONE is now purely a delivery question — the delivery-tracker pump
    /// closes the mailing when no transient trace remains. Guarded metadata
    /// stamp, not a state move: the mailing STAYS `sending` (the machine's
    /// `sending → done` edge stays reserved for the state-guarded complete
    /// verb), and the marker is idempotent — a replayed sweep pass or a
    /// concurrent walk end stamps nothing new (the `NOT metadata ?
    /// 'sms_walk_complete'` arm matches zero rows).
    ///
    /// This marker is what makes premature-done impossible: the pump only
    /// ever considers mailings whose walk ended, so pass-budgeted audiences
    /// (traces minted across several sweeps) and freshly minted outgoing
    /// traces never look "already done" to the inference.
    pub async fn mark_sms_walk_complete(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET metadata = metadata || jsonb_build_object(
                       'sms_walk_complete', 'true'::jsonb,
                       'sms_walk_completed_at', to_jsonb(now()))
               WHERE id = $1
                 AND state = 'sending'
                 AND NOT metadata ? 'sms_walk_complete'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// The delivery-tracker pump's done-inference candidates: sms-type
    /// mailings whose walk ended (the marker above) and which are not
    /// parked on a send error. Read WITHOUT a lock here — the per-candidate
    /// re-check under `lock_mailing_for_done` is what makes the inference
    /// race-free; this scan only decides who is worth locking.
    pub async fn sms_done_inference_candidates(
        conn: &mut PgConnection,
        limit: i64,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id
               FROM mailing.mailings
               WHERE mailing_type = 'sms'
                 AND state = 'sending'
                 AND metadata->>'sms_walk_complete' = 'true'
                 AND NOT metadata ? 'send_error'
                 AND (metadata->>'deleted_at') IS NULL
               ORDER BY (metadata->>'created_at') NULLS LAST, id
               LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&mut *conn)
        .await
    }

    /// Lock ONE done-inference candidate `FOR UPDATE` under its full
    /// candidacy predicate — the claim_due_mailings precedent applied to
    /// the completion edge: two pump passes (or a pump racing the sweep's
    /// own last-trace completion) serialize here, and the SECOND locker
    /// re-runs the remaining-count under the lock, sees the row already
    /// `done`, matches zero rows, and no-ops. The lock is held across the
    /// remaining-check + the complete verb and released at commit — never
    /// longer.
    pub async fn lock_mailing_for_done(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id
               FROM mailing.mailings
               WHERE id = $1
                 AND mailing_type = 'sms'
                 AND state = 'sending'
                 AND metadata->>'sms_walk_complete' = 'true'
                 AND NOT metadata ? 'send_error'
                 AND (metadata->>'deleted_at') IS NULL
               FOR UPDATE"#,
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// The complete-empty edge: audience resolved to zero recipients —
    /// `in_queue → done` directly, still stamping `sent_date` (the mailing
    /// DID run; its audience was empty).
    pub async fn complete_empty(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'done', sent_date = now()
               WHERE id = $1 AND state IN ('in_queue', 'sending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// `retry_failed`'s re-queue: `done → in_queue` (guarded — done is
    /// terminal until this verb says otherwise).
    pub async fn requeue_mailing(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'in_queue'
               WHERE id = $1 AND state = 'done'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Create a mailing row with the CANONICAL parsed domain (the service
    /// parsed + validated before it ever reached here).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_mailing(
        conn: &mut PgConnection,
        id: Uuid,
        subject: &str,
        preview: Option<&str>,
        body_html: &str,
        email_from: &str,
        reply_to: Option<&str>,
        mailing_domain: &serde_json::Value,
        target_model: &str,
        schedule_type: &str,
        schedule_date: Option<DateTime<Utc>>,
        use_exclusion_list: bool,
        campaign_id: Option<Uuid>,
        source_id: Option<Uuid>,
        ab_testing_enabled: bool,
        ab_testing_pc: i32,
        ab_test_id: Option<Uuid>,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailings
                   (id, subject, preview, body_html, email_from, reply_to,
                    mailing_domain, target_model, schedule_type, schedule_date,
                    use_exclusion_list, campaign_id, source_id,
                    ab_testing_enabled, ab_testing_pc, ab_test_id)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8::mailing_target_model,
                       $9::mailing_schedule_type, $10, $11, $12, $13,
                       $14, $15, $16)
               RETURNING id"#,
        )
        .bind(id)
        .bind(subject)
        .bind(preview)
        .bind(body_html)
        .bind(email_from)
        .bind(reply_to)
        .bind(mailing_domain)
        .bind(target_model)
        .bind(schedule_type)
        .bind(schedule_date)
        .bind(use_exclusion_list)
        .bind(campaign_id)
        .bind(source_id)
        .bind(ab_testing_enabled)
        .bind(ab_testing_pc)
        .bind(ab_test_id)
        .fetch_one(&mut *conn)
        .await
    }

    /// Update a DRAFT mailing's editable fields incl. re-validated domain.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_draft(
        conn: &mut PgConnection,
        id: Uuid,
        subject: &str,
        preview: Option<&str>,
        body_html: &str,
        email_from: &str,
        reply_to: Option<&str>,
        mailing_domain: &serde_json::Value,
        schedule_type: &str,
        schedule_date: Option<DateTime<Utc>>,
        use_exclusion_list: bool,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET subject = $2, preview = $3, body_html = $4, email_from = $5,
                   reply_to = $6, mailing_domain = $7,
                   schedule_type = $9::mailing_schedule_type, schedule_date = $10,
                   use_exclusion_list = $11
               WHERE id = $1 AND state = 'draft'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .bind(subject)
        .bind(preview)
        .bind(body_html)
        .bind(email_from)
        .bind(reply_to)
        .bind(mailing_domain)
        .bind(schedule_type)
        .bind(schedule_date)
        .bind(use_exclusion_list)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Queue a draft for sending (the launch edge `draft → in_queue`),
    /// stamping the schedule pair.
    pub async fn queue_mailing(
        conn: &mut PgConnection,
        id: Uuid,
        schedule_type: &str,
        schedule_date: Option<DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'in_queue',
                   schedule_type = $2::mailing_schedule_type,
                   schedule_date = $3
               WHERE id = $1 AND state = 'draft'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .bind(schedule_type)
        .bind(schedule_date)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// A mailing's campaign attribution (the arming event's optional field).
    /// RowNotFound when the mailing itself is gone.
    pub async fn mailing_campaign_id(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        match sqlx::query_scalar::<_, Option<Uuid>>(
            r#"SELECT campaign_id FROM mailing.mailings
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        {
            Some(campaign) => Ok(campaign),
            None => Err(sqlx::Error::RowNotFound),
        }
    }

    /// The launch pre-check's input: a live mailing's channel + SMS body
    /// (the launch verb refuses an sms mailing without a usable body —
    /// the typed refusal ahead of the DB CHECK backstop).
    pub async fn mailing_launch_shape(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
        sqlx::query_as::<_, (String, Option<String>)>(
            r#"SELECT mailing_type::text, body_plaintext
               FROM mailing.mailings
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// One live mailing by id (the verbs' probe).
    pub async fn find_live_state(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<(Uuid, String, Option<DateTime<Utc>>)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, String, Option<DateTime<Utc>>)>(
            r#"SELECT id, state::text, schedule_date
               FROM mailing.mailings
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// A live mailing's targeting domain (the raw json the unsubscribe leg
    /// re-parses for its audience terms). None when the mailing is gone or
    /// soft-deleted — the caller refuses loudly, it never guesses an
    /// audience set.
    pub async fn find_live_mailing_domain(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        sqlx::query_scalar::<_, serde_json::Value>(
            r#"SELECT mailing_domain
               FROM mailing.mailings
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// The cancel edge: [in_queue, sending, done] → draft, clearing the
    /// schedule pair (schedule_type back to 'immediate'). Traces are KEPT
    /// for audit — there is no canceled state value (the machine's
    /// declaration).
    pub async fn cancel_mailing(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'draft',
                   schedule_type = 'immediate',
                   schedule_date = NULL
               WHERE id = $1 AND state IN ('in_queue', 'sending', 'done')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    // ── recipient resolution (parameterized compile) ──────────────────────────

    /// Resolve the send targets for a compiled domain over
    /// `mailing_contacts`. ONE parameterized query: field terms bind against
    /// contact columns; audience terms compile to subscription
    /// EXISTS / NOT EXISTS seams. `hard_cap` is the resolver safety ceiling
    /// (a mis-sized domain fails loudly to the caller, never silently
    /// truncates: the engine asserts `len < hard_cap`).
    #[expect(clippy::expect_used, reason = "the domain grammar guarantees every term in this branch is column-backed")]
    pub async fn resolve_contact_recipients(
        conn: &mut PgConnection,
        domain: &CompiledDomain,
        hard_cap: i64,
    ) -> Result<Vec<ResolvedRecipient>, sqlx::Error> {
        let mut sql = String::from(
            "SELECT c.id AS recipient_id, c.email \
             FROM mailing.mailing_contacts c \
             WHERE (c.metadata->>'deleted_at') IS NULL",
        );
        let mut args = PgArguments::default();
        let mut n = 0usize;

        // Audience membership: positive terms require a subscription in the
        // union; negative terms forbid one (cross-list NOT semantics).
        let pos_auds: Vec<&str> = domain
            .terms
            .iter()
            .filter(|t| t.field == DomainField::MailingAudienceId && matches!(t.op, DomainOp::Eq | DomainOp::In))
            .flat_map(|t| t.values.iter().map(String::as_str))
            .collect();
        let neg_auds: Vec<&str> = domain
            .terms
            .iter()
            .filter(|t| t.field == DomainField::MailingAudienceId && matches!(t.op, DomainOp::Ne | DomainOp::NotIn))
            .flat_map(|t| t.values.iter().map(String::as_str))
            .collect();

        if !pos_auds.is_empty() {
            n += 1;
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM mailing.mailing_subscriptions s \
                 WHERE s.contact_id = c.id AND NOT s.opt_out \
                 AND (s.metadata->>'deleted_at') IS NULL \
                 AND s.mailing_audience_id = ANY(${n}::uuid[]))"
            ));
            let bound: Vec<Uuid> = pos_auds
                .iter()
                .map(|v| Uuid::parse_str(v).unwrap_or_else(|_| Uuid::nil()))
                .collect();
            args.add(bound).map_err(sqlx::Error::Encode)?;
        }
        if !neg_auds.is_empty() {
            n += 1;
            sql.push_str(&format!(
                " AND NOT EXISTS (SELECT 1 FROM mailing.mailing_subscriptions s \
                 WHERE s.contact_id = c.id \
                 AND (s.metadata->>'deleted_at') IS NULL \
                 AND s.mailing_audience_id = ANY(${n}::uuid[]))"
            ));
            let bound: Vec<Uuid> = neg_auds
                .iter()
                .map(|v| Uuid::parse_str(v).unwrap_or_else(|_| Uuid::nil()))
                .collect();
            args.add(bound).map_err(sqlx::Error::Encode)?;
        }

        // Contact-column terms — each a bound predicate (no string
        // interpolation of values anywhere).
        for term in domain
            .terms
            .iter()
            .filter(|t| t.field != DomainField::MailingAudienceId)
        {
            let col = term.field.column().expect("column-backed field");
            let predicate = match term.op {
                DomainOp::Eq | DomainOp::In => format!("lower(coalesce({col}, '')) = ANY(${})", n + 1),
                DomainOp::Ne | DomainOp::NotIn => {
                    format!("lower(coalesce({col}, '')) <> ALL(${})", n + 1)
                }
                DomainOp::Like => format!("coalesce({col}, '') ILIKE '%' || ${} || '%'", n + 1),
                DomainOp::NotLike => {
                    format!("coalesce({col}, '') NOT ILIKE '%' || ${} || '%'", n + 1)
                }
            };
            // Array-bind shape: Eq/Ne also arrive as one-element vecs so the
            // predicate family is uniform.
            let lowered: Vec<String> = match term.op {
                DomainOp::Like | DomainOp::NotLike => term.values.clone(),
                _ => term.values.iter().map(|v| v.to_lowercase()).collect(),
            };
            sql.push_str(" AND ");
            sql.push_str(&predicate);
            if matches!(term.op, DomainOp::Like | DomainOp::NotLike) {
                // single pattern value per term (parser guarantees len 1)
                let pat = lowered.first().cloned().unwrap_or_default();
                args.add(pat).map_err(sqlx::Error::Encode)?;
            } else {
                args.add(lowered).map_err(sqlx::Error::Encode)?;
            }
            n += 1;
        }

        sql.push_str(&format!(" ORDER BY c.email LIMIT {hard_cap}"));
        sqlx::query_as_with::<sqlx::Postgres, ResolvedRecipient, _>(&sql, args)
            .fetch_all(&mut *conn)
            .await
    }

    // ── batched suppression probes ────────────────────────────────────────────

    /// Exclusion-list probe: which of these emails sit ACTIVE on the mail
    /// blacklist? (Batched — one query per sweep.)
    pub async fn blacklisted_emails(
        conn: &mut PgConnection,
        emails: &[String],
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            r#"SELECT lower(b.email)
               FROM messaging.mail_blacklists b
               WHERE b.active
                 AND lower(b.email) = ANY($1)"#,
        )
        .bind(emails.to_vec())
        .fetch_all(&mut *conn)
        .await
    }

    /// Cross-list opt-out probe: which of these emails carry ANY live
    /// opt-out flip? (Batched; OPT-OUT-WINS.)
    pub async fn opted_out_emails(
        conn: &mut PgConnection,
        emails: &[String],
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            r#"SELECT DISTINCT lower(c.email)
               FROM mailing.mailing_subscriptions s
               JOIN mailing.mailing_contacts c ON c.id = s.contact_id
               WHERE s.opt_out
                 AND (s.metadata->>'deleted_at') IS NULL
                 AND (c.metadata->>'deleted_at') IS NULL
                 AND lower(c.email) = ANY($1)"#,
        )
        .bind(emails.to_vec())
        .fetch_all(&mut *conn)
        .await
    }

    /// Disposition probe — the drive loop's per-recipient memory, ONE query:
    /// (recipient_id, seen_active, suppressed_here).
    ///
    /// `seen_active`: a live NON-cancel trace on this mailing — or, when the
    /// mailing runs inside an A/B campaign, on the CAMPAIGN (the
    /// cross-variant disjointness). Such recipients are skipped silently:
    /// already sent, or already claimed by a sibling variant.
    ///
    /// `suppressed_here`: a live CANCEL trace on THIS mailing only — the
    /// recipient was already visibly suppressed in an earlier capped pass;
    /// re-suppressing would duplicate cancel rows every pass, so these are
    /// skipped too (the first suppression stays the visible one).
    pub async fn recipient_disposition(
        conn: &mut PgConnection,
        mailing_id: Uuid,
        campaign_id: Option<Uuid>,
    ) -> Result<Vec<(Uuid, bool, bool)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, bool, bool)>(
            r#"SELECT recipient_id,
                      bool_or(trace_status <> 'cancel'
                              AND (mailing_id = $1
                                   OR ($2::uuid IS NOT NULL AND campaign_id = $2))) AS seen_active,
                      bool_or(trace_status = 'cancel' AND mailing_id = $1) AS suppressed_here
               FROM mailing.mailing_traces
               WHERE (mailing_id = $1 OR ($2::uuid IS NOT NULL AND campaign_id = $2))
                 AND (metadata->>'deleted_at') IS NULL
               GROUP BY recipient_id"#,
        )
        .bind(mailing_id)
        .bind(campaign_id)
        .fetch_all(&mut *conn)
        .await
    }

    /// The sampling seed for a mailing's A/B test (minted once at test
    /// creation; persisted — the membership function is stable forever).
    pub async fn sampling_seed(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            r#"SELECT sampling_seed FROM mailing.mailing_ab_tests
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(ab_test_id)
        .fetch_optional(&mut *conn)
        .await
    }

    // ── retry_failed's trace cleanup ──────────────────────────────────────────

    /// Soft-delete error/bounce traces in batches of `batch` — soft delete
    /// OPENS the mint fence (a re-send mints a fresh live trace) while the
    /// audit trail stays queryable. Returns total soft-deleted.
    pub async fn soft_delete_failed_traces(
        conn: &mut PgConnection,
        mailing_id: Uuid,
        batch: i64,
    ) -> Result<u64, sqlx::Error> {
        let mut total = 0u64;
        loop {
            let affected = sqlx::query(
                r#"UPDATE mailing.mailing_traces
                   SET metadata = metadata
                       || jsonb_build_object('deleted_at', to_jsonb(now()))
                   WHERE id IN (
                       SELECT id FROM mailing.mailing_traces
                       WHERE mailing_id = $1
                         AND trace_status IN ('error', 'bounce')
                         AND (metadata->>'deleted_at') IS NULL
                       ORDER BY id
                       LIMIT $2
                   )"#,
            )
            .bind(mailing_id)
            .bind(batch)
            .execute(&mut *conn)
            .await?
            .rows_affected();
            total += affected;
            if affected < batch as u64 {
                break;
            }
        }
        Ok(total)
    }

    /// Count of live failed (error/bounce) traces — `retry_failed`'s typed
    /// refusal probe when zero.
    pub async fn count_failed_traces(
        conn: &mut PgConnection,
        mailing_id: Uuid,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar::<_, i64>(
            r#"SELECT count(*) FROM mailing.mailing_traces
               WHERE mailing_id = $1
                 AND trace_status IN ('error', 'bounce')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(mailing_id)
        .fetch_one(&mut *conn)
        .await
    }

    // ── auto-blacklist (DB clock throughout) ──────────────────────────────────

    /// The auto-blacklist sweep: emails with >= `max_bounces` bounce traces
    /// inside `window_weeks`, SPREAD over more than `spread_days` (one
    /// incidental bounce can never blacklist; a persistent pattern does),
    /// inserted into `messaging.mail_blacklists` idempotently. Returns how
    /// many NEW blacklist rows landed.
    pub async fn auto_blacklist_sweep(
        conn: &mut PgConnection,
        window_weeks: i32,
        max_bounces: i64,
        spread_days: i32,
    ) -> Result<u64, sqlx::Error> {
        sqlx::query(
            r#"WITH offenders AS (
                   SELECT recipient_email
                   FROM mailing.mailing_traces
                   WHERE failure_type = 'mail_bounce'
                     AND (metadata->>'deleted_at') IS NULL
                     AND (metadata->>'created_at')::timestamptz
                           >= now() - make_interval(weeks => $1)
                   GROUP BY recipient_email
                   HAVING count(*) >= $2
                      AND max((metadata->>'created_at')::timestamptz)
                          - min((metadata->>'created_at')::timestamptz)
                          > make_interval(days => $3)
               )
               INSERT INTO messaging.mail_blacklists (email, active)
               SELECT DISTINCT o.recipient_email, TRUE FROM offenders o
               ON CONFLICT (email) DO NOTHING"#,
        )
        .bind(window_weeks)
        .bind(max_bounces)
        .bind(spread_days)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected())
    }

    // ── A/B promotion ─────────────────────────────────────────────────────────

    /// Tests past their promote_at moment, not yet completed — the sweep's
    /// ordered promotion step (keyed by completed + winner_mailing_id, so
    /// re-runs no-op).
    pub async fn ab_tests_due_for_promotion(
        conn: &mut PgConnection,
    ) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, String)>(
            r#"SELECT id, winner_selection::text
               FROM mailing.mailing_ab_tests
               WHERE NOT completed
                 AND promote_at IS NOT NULL
                 AND promote_at <= now()
                 AND (metadata->>'deleted_at') IS NULL
               ORDER BY promote_at, id"#,
        )
        .fetch_all(&mut *conn)
        .await
    }

    /// One A/B test row (the promotion probe + the read side):
    /// (id, campaign_id, winner_selection, winner_selection_sms, promote_at,
    /// completed, winner_mailing_id, sampling_seed).
    #[allow(clippy::type_complexity)]
    pub async fn find_ab_test(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
    ) -> Result<
        Option<(
            Uuid,
            Uuid,
            String,
            Option<String>,
            Option<DateTime<Utc>>,
            bool,
            Option<Uuid>,
            String,
        )>,
        sqlx::Error,
    > {
        sqlx::query_as::<_, (
            Uuid,
            Uuid,
            String,
            Option<String>,
            Option<DateTime<Utc>>,
            bool,
            Option<Uuid>,
            String,
        )>(
            r#"SELECT id, campaign_id, winner_selection::text,
                      winner_selection_sms::text, promote_at,
                      completed, winner_mailing_id, sampling_seed
               FROM mailing.mailing_ab_tests
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(ab_test_id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Declare the PARALLEL sms winner axis on a live test (MVX-2): guarded
    /// on NOT completed — a completed test's axes are historical record.
    /// Returns false when the test is gone or already completed.
    pub async fn set_ab_test_sms_selection(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
        selection: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_ab_tests
               SET winner_selection_sms = $2::ab_winner_selection
               WHERE id = $1 AND NOT completed
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(ab_test_id)
        .bind(selection)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Rank a test's done variants by the persisted metric and return them
    /// best-first: (mailing_id, ratio percentage). The metric expression is
    /// chosen from a FIXED whitelist here — unknown metrics fall to the
    /// opened default (the service already whitelists before calling).
    pub async fn rank_variants_by_metric(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
        metric: &str,
    ) -> Result<Vec<(Uuid, i64)>, sqlx::Error> {
        let metric_expr = Self::metric_expr(metric);
        let sql = format!(
            r#"WITH sent AS (
                   SELECT m.id, count(t.id) AS sent_n
                   FROM mailing.mailings m
                   LEFT JOIN mailing.mailing_traces t
                     ON t.mailing_id = m.id
                    AND (t.metadata->>'deleted_at') IS NULL
                    AND t.sent_datetime IS NOT NULL
                   WHERE m.ab_test_id = $1
                     AND m.state = 'done'
                     AND (m.metadata->>'deleted_at') IS NULL
                   GROUP BY m.id
               )
               SELECT m.id,
                      COALESCE(round(100.0 * (
                          SELECT count(*) FROM mailing.mailing_traces t
                          WHERE t.mailing_id = m.id
                            AND (t.metadata->>'deleted_at') IS NULL
                            AND t.sent_datetime IS NOT NULL
                            AND {metric_expr}
                      ) / NULLIF(s.sent_n, 0)), 0)::bigint AS ratio
               FROM sent s
               JOIN mailing.mailings m ON m.id = s.id
               ORDER BY ratio DESC, m.id"#
        );
        sqlx::query_as::<_, (Uuid, i64)>(&sql)
            .bind(ab_test_id)
            .fetch_all(&mut *conn)
            .await
    }

    /// The FIXED trace-hit expression per selection value — the same
    /// whitelist `rank_variants_by_metric` and the mixed-channel compare
    /// share. The invoiced-amount axis is NOT here by design: it is not a
    /// stored trace number (it reads through the declared billing seam at
    /// promotion time), so the compare surfaces it via the opened default.
    fn metric_expr(metric: &str) -> &'static str {
        match metric {
            "clicks_ratio" => "t.links_click_datetime IS NOT NULL",
            "replied_ratio" => "t.trace_status = 'reply'",
            _ => "t.trace_status IN ('open', 'reply')",
        }
    }

    /// A test's DONE variants with their cited engagement SOURCE — the
    /// `sale_invoiced_amount` ranking's input (MVX-4: attribution keys on
    /// the utm source the mailing cites; a variant citing no source
    /// attributes nothing and ranks last).
    pub async fn variant_source_ids(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
            r#"SELECT id, source_id
               FROM mailing.mailings
               WHERE ab_test_id = $1
                 AND state = 'done'
                 AND (metadata->>'deleted_at') IS NULL
               ORDER BY (metadata->>'created_at') NULLS LAST, id"#,
        )
        .bind(ab_test_id)
        .fetch_all(&mut *conn)
        .await
    }

    /// The mixed-channel compare (MVX-2): one grouped query over a test's
    /// live variants, split per channel, ranking on STORED trace numbers
    /// with each channel's own axis. Returns
    /// (mailing_id, channel, sent, hits, parked) — `parked` marks a
    /// variant currently parked on `metadata.send_error` (the sms send
    /// walk's loud park: an sms variant typically carries ZERO traces until
    /// that walk is composed, so its ratio is honestly `None`, never a
    /// fabricated 0%).
    pub async fn mixed_channel_metric_rows(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
        mail_axis: &str,
        sms_axis: &str,
    ) -> Result<Vec<(Uuid, String, i64, i64, bool)>, sqlx::Error> {
        let mail_expr = Self::metric_expr(mail_axis);
        let sms_expr = Self::metric_expr(sms_axis);
        let sql = format!(
            r#"SELECT m.id,
                      m.mailing_type::text AS channel,
                      COALESCE(count(t.id) FILTER (
                          WHERE t.sent_datetime IS NOT NULL), 0)::bigint AS sent,
                      COALESCE(count(t.id) FILTER (
                          WHERE t.sent_datetime IS NOT NULL
                            AND CASE WHEN m.mailing_type = 'sms'
                                     THEN {sms_expr}
                                     ELSE {mail_expr} END), 0)::bigint AS hits,
                      (m.metadata ? 'send_error') AS parked
               FROM mailing.mailings m
               LEFT JOIN mailing.mailing_traces t
                 ON t.mailing_id = m.id
                AND (t.metadata->>'deleted_at') IS NULL
               WHERE m.ab_test_id = $1
                 AND (m.metadata->>'deleted_at') IS NULL
               GROUP BY m.id, m.mailing_type
               ORDER BY m.id"#
        );
        sqlx::query_as::<_, (Uuid, String, i64, i64, bool)>(&sql)
            .bind(ab_test_id)
            .fetch_all(&mut *conn)
            .await
    }

    /// Mint the winner copy: the winning variant's content re-queued for the
    /// REMAINING audience (ab_testing_pc = 100, ab_testing_enabled = FALSE —
    /// the copy is a plain full mailing, not another variant).
    #[allow(clippy::too_many_arguments)]
    pub async fn promote_winner(
        conn: &mut PgConnection,
        new_id: Uuid,
        winner_mailing_id: Uuid,
        ab_test_id: Uuid,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailings
                   (id, subject, preview, body_html, email_from, reply_to,
                    mailing_domain, target_model, schedule_type,
                    use_exclusion_list, campaign_id, ab_testing_enabled,
                    ab_testing_pc, ab_test_id)
               SELECT $1, subject, preview, body_html, email_from, reply_to,
                      mailing_domain, target_model, 'immediate',
                      use_exclusion_list, campaign_id, FALSE, 100, $3
               FROM mailing.mailings
               WHERE id = $2 AND (metadata->>'deleted_at') IS NULL
               RETURNING id"#,
        )
        .bind(new_id)
        .bind(winner_mailing_id)
        .bind(ab_test_id)
        .fetch_one(&mut *conn)
        .await
    }

    /// Stamp the test's winner + completion — guarded on NOT completed so a
    /// second sweep matches zero rows (idempotent).
    pub async fn complete_ab_test(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
        winner_mailing_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_ab_tests
               SET completed = TRUE, winner_mailing_id = $2
               WHERE id = $1 AND NOT completed
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(ab_test_id)
        .bind(winner_mailing_id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Queue the promoted winner (launch edge on the copy).
    pub async fn queue_promoted_winner(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailings
               SET state = 'in_queue'
               WHERE id = $1 AND state = 'draft'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Mint a new A/B test row with a fresh sampling seed (the ONLY place
    /// seeds are born; never regenerated afterwards).
    pub async fn insert_ab_test(
        conn: &mut PgConnection,
        id: Uuid,
        campaign_id: Uuid,
        winner_selection: &str,
        promote_at: Option<DateTime<Utc>>,
        sampling_seed: &str,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_ab_tests
                   (id, campaign_id, winner_selection, promote_at, sampling_seed)
               VALUES ($1, $2, $3::ab_winner_selection, $4, $5)
               RETURNING id"#,
        )
        .bind(id)
        .bind(campaign_id)
        .bind(winner_selection)
        .bind(promote_at)
        .bind(sampling_seed)
        .fetch_one(&mut *conn)
        .await
    }

    /// Live mailings bound to an A/B test (the variants probe).
    pub async fn variant_mailing_ids(
        conn: &mut PgConnection,
        ab_test_id: Uuid,
    ) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, String)>(
            r#"SELECT id, state::text FROM mailing.mailings
               WHERE ab_test_id = $1 AND (metadata->>'deleted_at') IS NULL
               ORDER BY (metadata->>'created_at') NULLS LAST, id"#,
        )
        .bind(ab_test_id)
        .fetch_all(&mut *conn)
        .await
    }

    /// Bind a mailing as a variant of an A/B test: sets the test id, enables
    /// fragment sampling at `pc` percent, and stamps the campaign
    /// attribution from the test (variants share the campaign — the
    /// cross-variant seen-list dedupe keys on it). Draft-only guard.
    pub async fn bind_variant(
        conn: &mut PgConnection,
        mailing_id: Uuid,
        ab_test_id: Uuid,
        pc: i32,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE mailing.mailings
               SET ab_test_id = $2,
                   ab_testing_enabled = TRUE,
                   ab_testing_pc = $3,
                   campaign_id = COALESCE(
                       campaign_id,
                       (SELECT campaign_id FROM mailing.mailing_ab_tests WHERE id = $2))
               WHERE id = $1 AND state = 'draft'
                 AND (metadata->>'deleted_at') IS NULL
               RETURNING id"#,
        )
        .bind(mailing_id)
        .bind(ab_test_id)
        .bind(pc)
        .fetch_optional(&mut *conn)
        .await
    }
}
