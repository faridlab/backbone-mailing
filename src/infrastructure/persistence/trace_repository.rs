//! `TraceRepository` — the hand-written SQL behind the trace write path and
//! the stats read service (hand-authored, user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! Module rule: services orchestrate, repositories hold SQL. Runtime queries
//! — no `.sqlx` macros. Every method takes a connection inside the caller's
//! transaction; soft-delete filtering follows the `metadata->>'deleted_at'
//! IS NULL` convention.
//!
//! What lives here, and why:
//!
//! - **The mint** is a plain INSERT (never an upsert): the partial UNIQUE
//!   `(mailing_id, recipient_id) WHERE deleted_at IS NULL AND trace_status <>
//!   'cancel'` is the duplicate-mint FENCE — a concurrent writer surfaces as
//!   the index's unique-violation error code, which the service logs and
//!   skips. Converging with ON CONFLICT would turn the loud fence into the
//!   silent duplicate the fence exists to prevent.
//! - **The seven `set_*` verbs** are each ONE conditional UPDATE whose WHERE
//!   arm is exactly the declared transition's legal source set (the machine
//!   `trace_status`, schema/hooks/trace_status.hook.yaml) — the monotonic
//!   rank guard. A row already past the verb's rank matches zero rows and the
//!   service reads that as the idempotent skip, never a downgrade.
//! - **The stats query** is ONE grouped pass per mailing set (the ~30 KPI
//!   computes are read-side, non-stored). The documented sent/delivered
//!   asymmetry is preserved verbatim: `sent` counts `sent_datetime IS NOT
//!   NULL` (was ever submitted), `delivered` counts status currently in
//!   sent/open/reply.
//! - **Suppression cancel traces** (status `cancel`) sit OUTSIDE the mint
//!   fence and outside the seen-list probe — re-targeting after re-subscribe
//!   mints a fresh live trace.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

/// One row of the per-mailing stats aggregate (the grouped-query grain).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MailingTraceCounts {
    /// Live (non-canceled, non-deleted) traces minted — the audience reached.
    pub total: i64,
    /// Was ever submitted: `sent_datetime IS NOT NULL` (the KPI 'sent').
    pub sent: i64,
    /// Currently delivered: status in (sent, open, reply).
    pub delivered: i64,
    /// status in (open, reply) — bounce is NOT an open.
    pub opened: i64,
    /// `links_click_datetime IS NOT NULL` (the click fact — status-independent).
    pub clicked: i64,
    /// status = reply.
    pub replied: i64,
    /// status = bounce.
    pub bounced: i64,
    /// status = error.
    pub errored: i64,
    /// status = cancel (send-time suppression — visible, never silent).
    pub canceled: i64,
}

impl MailingTraceCounts {
    /// opened/sent as a percentage, rounded — None when nothing was sent.
    pub fn opened_ratio(&self) -> Option<i64> {
        ratio(self.opened, self.sent)
    }

    /// clicked/sent as a percentage, rounded — None when nothing was sent.
    pub fn clicks_ratio(&self) -> Option<i64> {
        ratio(self.clicked, self.sent)
    }

    /// replied/sent as a percentage, rounded — None when nothing was sent.
    pub fn replied_ratio(&self) -> Option<i64> {
        ratio(self.replied, self.sent)
    }
}

fn ratio(part: i64, whole: i64) -> Option<i64> {
    (whole > 0).then(|| (part as f64 / whole as f64 * 100.0).round() as i64)
}

/// Hand-written trace SQL. Services orchestrate; this holds SQL.
pub struct TraceRepository;

impl TraceRepository {
    /// Mint one trace (status `outgoing` at creation). Plain INSERT — the
    /// partial unique is the fence; a unique violation is the caller's to
    /// log-and-skip, never to converge silently.
    #[allow(clippy::too_many_arguments)]
    pub async fn mint_trace(
        conn: &mut PgConnection,
        id: Uuid,
        mailing_id: Uuid,
        campaign_id: Option<Uuid>,
        recipient_model: &str,
        recipient_id: Uuid,
        recipient_email: &str,
        trace_status: &str,
        failure_type: Option<&str>,
        is_test_trace: bool,
    ) -> Result<Uuid, sqlx::Error> {
        let out = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_traces
                   (id, trace_type, is_test_trace, mailing_id, campaign_id,
                    recipient_model, recipient_id, recipient_email,
                    trace_status, failure_type, metadata)
               VALUES ($1, 'mail', $2, $3, $4, $5, $6, $7, $8::trace_status,
                       $9::trace_failure_type,
                       jsonb_build_object('created_at', to_jsonb(now())))
               RETURNING id"#,
        )
        .bind(id)
        .bind(is_test_trace)
        .bind(mailing_id)
        .bind(campaign_id)
        .bind(recipient_model)
        .bind(recipient_id)
        .bind(recipient_email)
        .bind(trace_status)
        .bind(failure_type)
        .fetch_one(&mut *conn)
        .await?;
        Ok(out)
    }

    /// The engine's outgoing mint: same row, same fence — but a fence hit is
    /// absorbed as `Ok(false)` instead of a unique-violation error.
    ///
    /// Why this shape: the send engine mints a BATCH of traces inside one
    /// `commit_per_batch` transaction, and Postgres ABORTS a transaction at
    /// the first unique violation — a mid-batch fence hit would poison every
    /// later statement in that transaction (25P02) and fail the whole sweep.
    /// `ON CONFLICT DO NOTHING` is NOT convergence: nothing is updated, no
    /// duplicate row is ever created — the caller just learns, per row,
    /// whether THIS worker minted it or a concurrent worker already did
    /// (the skip the engine logs + counts as `fence_skips`). The strict
    /// `mint_trace` above stays the verb for every single-mint path that
    /// wants the violation surfaced.
    #[allow(clippy::too_many_arguments)]
    pub async fn mint_trace_fenced(
        conn: &mut PgConnection,
        id: Uuid,
        mailing_id: Uuid,
        campaign_id: Option<Uuid>,
        recipient_model: &str,
        recipient_id: Uuid,
        recipient_email: &str,
        trace_status: &str,
        failure_type: Option<&str>,
        is_test_trace: bool,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_traces
                   (id, trace_type, is_test_trace, mailing_id, campaign_id,
                    recipient_model, recipient_id, recipient_email,
                    trace_status, failure_type, metadata)
               VALUES ($1, 'mail', $2, $3, $4, $5, $6, $7, $8::trace_status,
                       $9::trace_failure_type,
                       jsonb_build_object('created_at', to_jsonb(now())))
               ON CONFLICT DO NOTHING
               RETURNING id"#,
        )
        .bind(id)
        .bind(is_test_trace)
        .bind(mailing_id)
        .bind(campaign_id)
        .bind(recipient_model)
        .bind(recipient_id)
        .bind(recipient_email)
        .bind(trace_status)
        .bind(failure_type)
        .fetch_optional(&mut *conn)
        .await
        .map(|r| r.is_some())
    }

    /// The channel-parameterized mint: the same row, same fence, with the
    /// channel (`trace_type`) and the phone-channel's canonical recipient
    /// number as arguments instead of the hardcoded `'mail'` above.
    ///
    /// A suppressed phone recipient's pre-canceled trace mints through HERE
    /// (`trace_type='sms'`): a phone-channel trace stamped `'mail'` would be
    /// invisible to the delivery pump's sms-scoped verdict join and miscounted
    /// by the completion counter — the channel must ride the mint, never the
    /// caller's assumption. `recipient_phone` is the sanitized E.164 number
    /// (mail traces pass `None`; the DB CHECK enforces canonical form when
    /// present). `recipient_email` stays the typed anchor it is on every
    /// channel — a phone-only recipient carries `""` (never a join key).
    #[allow(clippy::too_many_arguments)]
    pub async fn mint_trace_channel(
        conn: &mut PgConnection,
        id: Uuid,
        mailing_id: Uuid,
        campaign_id: Option<Uuid>,
        recipient_model: &str,
        recipient_id: Uuid,
        recipient_email: &str,
        recipient_phone: Option<&str>,
        trace_type: &str,
        trace_status: &str,
        failure_type: Option<&str>,
        is_test_trace: bool,
    ) -> Result<Uuid, sqlx::Error> {
        let out = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_traces
                   (id, trace_type, is_test_trace, mailing_id, campaign_id,
                    recipient_model, recipient_id, recipient_email,
                    recipient_phone, trace_status, failure_type, metadata)
               VALUES ($1, $10::trace_type, $2, $3, $4, $5, $6, $7, $8,
                       $9::trace_status, $11::trace_failure_type,
                       jsonb_build_object('created_at', to_jsonb(now())))
               RETURNING id"#,
        )
        .bind(id)
        .bind(is_test_trace)
        .bind(mailing_id)
        .bind(campaign_id)
        .bind(recipient_model)
        .bind(recipient_id)
        .bind(recipient_email)
        .bind(recipient_phone)
        .bind(trace_status)
        .bind(trace_type)
        .bind(failure_type)
        .fetch_one(&mut *conn)
        .await?;
        Ok(out)
    }

    /// The fenced channel mint — [`Self::mint_trace_channel`] with the
    /// batch-transaction fence semantics of [`Self::mint_trace_fenced`]: a
    /// partial-unique hit is absorbed as `Ok(false)` so one mid-batch fence
    /// hit cannot poison the surrounding transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn mint_trace_channel_fenced(
        conn: &mut PgConnection,
        id: Uuid,
        mailing_id: Uuid,
        campaign_id: Option<Uuid>,
        recipient_model: &str,
        recipient_id: Uuid,
        recipient_email: &str,
        recipient_phone: Option<&str>,
        trace_type: &str,
        trace_status: &str,
        failure_type: Option<&str>,
        is_test_trace: bool,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO mailing.mailing_traces
                   (id, trace_type, is_test_trace, mailing_id, campaign_id,
                    recipient_model, recipient_id, recipient_email,
                    recipient_phone, trace_status, failure_type, metadata)
               VALUES ($1, $10::trace_type, $2, $3, $4, $5, $6, $7, $8,
                       $9::trace_status, $11::trace_failure_type,
                       jsonb_build_object('created_at', to_jsonb(now())))
               ON CONFLICT DO NOTHING
               RETURNING id"#,
        )
        .bind(id)
        .bind(is_test_trace)
        .bind(mailing_id)
        .bind(campaign_id)
        .bind(recipient_model)
        .bind(recipient_id)
        .bind(recipient_email)
        .bind(recipient_phone)
        .bind(trace_status)
        .bind(trace_type)
        .bind(failure_type)
        .fetch_optional(&mut *conn)
        .await
        .map(|r| r.is_some())
    }

    // ── the seven set_* verbs (each a rank-guarded conditional UPDATE) ────────

    /// `set_process` (SMS channel only) — outgoing → process: the delivery
    /// tracker reports the channel is holding the message (queued at the
    /// gateway before dispatch). Same one-conditional-UPDATE shape as the
    /// mail verbs; the WHERE arm is exactly the declared edge's source set.
    pub async fn set_process(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        Self::advance(
            conn,
            trace_id,
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'process'
               WHERE id = $1
                 AND trace_status = 'outgoing'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .await
    }

    /// `set_pending` (SMS channel only) — [outgoing, process] → pending:
    /// handed to the gateway, awaiting the delivery report (displays
    /// 'Sent'). `outgoing` is a legal source because the pump observes at
    /// sweep cadence — a tracker that already moved past 'process' advances
    /// the trace in one compressed step, the same partial-order the upstream
    /// monotonic table admits.
    pub async fn set_pending(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        Self::advance(
            conn,
            trace_id,
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'pending'
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'process')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .await
    }

    /// `set_sent` — from outgoing/process/pending; stamps `sent_datetime`
    /// (first stamp wins), clears the failure pair.
    pub async fn set_sent(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        Self::advance(
            conn,
            trace_id,
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'sent', failure_type = NULL, failure_reason = NULL,
                   sent_datetime = COALESCE(sent_datetime, now())
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .await
    }

    /// `set_opened` — idempotent vs later states: rows already open/reply/
    /// bounce/error/cancel match zero rows (never a downgrade).
    pub async fn set_opened(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        Self::advance(
            conn,
            trace_id,
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'open', open_datetime = COALESCE(open_datetime, now())
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'sent', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .await
    }

    /// `set_replied` — also stamps `open_datetime` (a reply is a touch).
    pub async fn set_replied(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        Self::advance(
            conn,
            trace_id,
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'reply', reply_datetime = now(),
                   open_datetime = COALESCE(open_datetime, now())
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'sent', 'open', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .await
    }

    /// `set_bounced_sms` (SMS channel only) — [outgoing, process, pending] →
    /// bounce with the SMS failure code carried VERBATIM. Channel purity is
    /// the whole point of this verb's existence: it never writes
    /// `mail_bounce` (the EMAIL auto-blacklist window's fact source scans
    /// failure_type='mail_bounce' — an SMS bounce must not enroll a phone
    /// recipient there) and never stamps open_datetime (an SMS bounce is not
    /// a mail touch). The failure code must already be a member of
    /// trace_failure_type — the pump whitelists before calling; a non-member
    /// cast would abort the caller's transaction.
    pub async fn set_bounced_sms(
        conn: &mut PgConnection,
        trace_id: Uuid,
        failure_type: &str,
        failure_reason: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'bounce', failure_type = $2::trace_failure_type,
                   failure_reason = $3
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .bind(failure_type)
        .bind(failure_reason)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// `set_bounced` — sets failure_type='mail_bounce' (the auto-blacklist
    /// window's fact source); stamps open_datetime (a bounce is a touch).
    pub async fn set_bounced(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        Self::advance(
            conn,
            trace_id,
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'bounce', failure_type = 'mail_bounce',
                   open_datetime = COALESCE(open_datetime, now())
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'sent', 'open', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .await
    }

    /// `set_failed` — from outgoing/sent/process/pending into `error` with
    /// the failure pair.
    pub async fn set_failed(
        conn: &mut PgConnection,
        trace_id: Uuid,
        failure_type: &str,
        failure_reason: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'error', failure_type = $2::trace_failure_type,
                   failure_reason = $3
               WHERE id = $1
                 AND trace_status IN ('outgoing', 'sent', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .bind(failure_type)
        .bind(failure_reason)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// `set_canceled` — send-time suppression; only from `outgoing`.
    pub async fn set_canceled(
        conn: &mut PgConnection,
        trace_id: Uuid,
        failure_type: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET trace_status = 'cancel', failure_type = $2::trace_failure_type
               WHERE id = $1
                 AND trace_status = 'outgoing'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .bind(failure_type)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// `set_clicked` — stamps the LAST click datetime WITHOUT moving state by
    /// itself (the click route calls set_opened + set_clicked together).
    pub async fn set_clicked(conn: &mut PgConnection, trace_id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET links_click_datetime = now()
               WHERE id = $1
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Stamp the trace's mail row link (the mirrored no-FK seam — the trace
    /// survives queue-row GC). Conditional on NULL so a replayed repair pass
    /// never overwrites a standing link.
    pub async fn attach_mail_id(
        conn: &mut PgConnection,
        trace_id: Uuid,
        mail_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET mail_id = $2
               WHERE id = $1 AND mail_id IS NULL
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .bind(mail_id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Stamp the trace's sms row link — the SMS twin of `attach_mail_id`:
    /// the enqueued sms row's EXTERNAL uuid, conditional on NULL so a
    /// replayed enqueue repair never overwrites a standing link. No DB
    /// constraint either side; the delivery-tracker pump joins
    /// messaging.sms_trackers through it read-only.
    pub async fn attach_sms_uuid(
        conn: &mut PgConnection,
        trace_id: Uuid,
        sms_uuid: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET sms_uuid = $2
               WHERE id = $1 AND sms_uuid IS NULL
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .bind(sms_uuid)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    /// Stamp the RFC Message-ID (the future inbound reply/bounce match key).
    pub async fn set_message_id(
        conn: &mut PgConnection,
        trace_id: Uuid,
        message_id: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(
            r#"UPDATE mailing.mailing_traces
               SET message_id = $2
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .map(|r| r.rows_affected() > 0)
    }

    async fn advance(
        conn: &mut PgConnection,
        trace_id: Uuid,
        sql: &'static str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query(sql)
            .bind(trace_id)
            .execute(&mut *conn)
            .await
            .map(|r| r.rows_affected() > 0)
    }

    // ── probes ────────────────────────────────────────────────────────────────

    /// The repair arm's input: live outgoing traces whose enqueue never
    /// completed (mail_id still NULL) and which are OLD ENOUGH that the gap
    /// cannot be another worker's in-flight mint→enqueue window. Without
    /// the grace interval, a concurrent sweep driving the same mailing
    /// would "repair" traces the first worker is between mint and enqueue
    /// on RIGHT NOW — enqueueing them twice (two mail rows, two sends).
    /// Mail-channel only by explicit filter: sms traces also carry a NULL
    /// mail_id (their linkage is the sms_uuid seam), so without the filter
    /// this arm would sweep them into the EMAIL enqueue.
    pub async fn outgoing_without_mail(
        conn: &mut PgConnection,
        mailing_id: Uuid,
        grace_minutes: i32,
    ) -> Result<Vec<(Uuid, Uuid, String)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, Uuid, String)>(
            r#"SELECT id, recipient_id, recipient_email
               FROM mailing.mailing_traces
               WHERE mailing_id = $1 AND trace_status = 'outgoing'
                 AND trace_type = 'mail'
                 AND mail_id IS NULL
                 AND (metadata->>'deleted_at') IS NULL
                 AND (metadata->>'created_at')::timestamptz
                       <= now() - make_interval(mins => $2)
               ORDER BY recipient_email"#,
        )
        .bind(mailing_id)
        .bind(grace_minutes)
        .fetch_all(&mut *conn)
        .await
    }

    /// The reconcile arm's input: live outgoing traces whose mail row HAS
    /// settled — (trace_id, mails.state, mails.failure_type). GLOBAL by
    /// design: SMTP verdicts land asynchronously, so a DONE mailing's final
    /// traces still need this pass — the sweep reconciles every unsettled
    /// outgoing trace it can see, not just freshly claimed mailings.
    pub async fn settled_mails_for_reconcile(
        conn: &mut PgConnection,
    ) -> Result<Vec<(Uuid, String, Option<String>)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, String, Option<String>)>(
            r#"SELECT t.id, m.state::text, m.failure_type::text
               FROM mailing.mailing_traces t
               JOIN messaging.mails m ON m.id = t.mail_id
               WHERE t.trace_status = 'outgoing'
                 AND m.state IN ('sent', 'exception')
                 AND (t.metadata->>'deleted_at') IS NULL
               ORDER BY t.id
               LIMIT 5000"#,
        )
        .fetch_all(&mut *conn)
        .await
    }

    /// The delivery-tracker pump's advance-pass input: live sms-type traces
    /// still in a transient status, each joined to its delivery tracker by
    /// the sms_uuid seam. Read-only cross-schema consumption — the same
    /// precedent as `settled_mails_for_reconcile`'s messaging.mails join, in
    /// the opposite direction: this side NEVER writes into messaging, the
    /// tracker is simply the durable delivery fact.
    ///
    /// The tracker (not the sms queue row) is the join target because BOTH
    /// verdict paths mirror it in their own transaction — the signed
    /// delivery-report webhook AND the drainer's own outcome application —
    /// and it survives sms-row GC (unique(sms_uuid)). An inner join is
    /// therefore not a stuck-mailing risk: every sms-row enqueue mints the
    /// tracker in the same transaction, so a live sms-type trace without a
    /// tracker is a substrate invariant violation worth leaving visible.
    pub async fn sms_traces_with_tracker_verdicts(
        conn: &mut PgConnection,
    ) -> Result<Vec<SmsTraceVerdict>, sqlx::Error> {
        sqlx::query_as::<_, SmsTraceVerdict>(
            r#"SELECT t.id AS trace_id,
                      tr.state::text AS tracker_state,
                      tr.failure_type::text AS failure_type,
                      tr.failure_reason
               FROM mailing.mailing_traces t
               JOIN messaging.sms_trackers tr ON tr.sms_uuid = t.sms_uuid
               WHERE t.trace_type = 'sms'
                 AND t.trace_status IN ('outgoing', 'process', 'pending')
                 AND (t.metadata->>'deleted_at') IS NULL
               ORDER BY t.id
               LIMIT 5000"#,
        )
        .fetch_all(&mut *conn)
        .await
    }

    /// The done-inference remaining-check, run UNDER the mailing-row lock:
    /// live traces of this mailing still in a transient status. `outgoing`
    /// counts as remaining on purpose — the pump observes at sweep cadence,
    /// so a trace the pump has not visited yet is indistinguishable from one
    /// the channel never dispatched; closing on it would be premature done.
    /// (Upstream's check counts process-only because its traces are written
    /// synchronously in the same pass; this port's pump is asynchronous, so
    /// the transient set is the honest remaining set.)
    pub async fn count_transient_sms_traces(
        conn: &mut PgConnection,
        mailing_id: Uuid,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar::<_, i64>(
            r#"SELECT count(*)
               FROM mailing.mailing_traces
               WHERE mailing_id = $1
                 AND trace_type = 'sms'
                 AND trace_status IN ('outgoing', 'process', 'pending')
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(mailing_id)
        .fetch_one(&mut *conn)
        .await
    }

    /// One live trace by id (the verbs' row probe).
    pub async fn find_live(
        conn: &mut PgConnection,
        trace_id: Uuid,
    ) -> Result<Option<TraceRow>, sqlx::Error> {
        sqlx::query_as::<_, TraceRow>(
            r#"SELECT id, trace_status::text AS trace_status, failure_type::text AS failure_type,
                      sent_datetime, open_datetime, reply_datetime, links_click_datetime, mail_id
               FROM mailing.mailing_traces
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// One live trace by id, in the public trace-route projection — the
    /// fields the `/r/:code/m/:trace` family needs: the consistency check's
    /// campaign half (the trace's denormalized copy), the unsubscribe leg's
    /// mailing + recipient anchor.
    pub async fn find_route_trace(
        conn: &mut PgConnection,
        trace_id: Uuid,
    ) -> Result<Option<RouteTraceRow>, sqlx::Error> {
        sqlx::query_as::<_, RouteTraceRow>(
            r#"SELECT id, trace_status::text AS trace_status, mailing_id, campaign_id,
                      recipient_email
               FROM mailing.mailing_traces
               WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(trace_id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// The stats grouped query — ONE pass per mailing set, filtered to live
    /// (non-deleted) traces. Canceled rows count as `canceled` only.
    pub async fn counts_for_mailings(
        conn: &mut PgConnection,
        mailing_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, MailingTraceCounts)>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (Uuid, i64, i64, i64, i64, i64, i64, i64, i64, i64)>(
            r#"SELECT mailing_id,
                      count(*) FILTER (WHERE trace_status <> 'cancel') AS total,
                      count(*) FILTER (WHERE sent_datetime IS NOT NULL) AS sent,
                      count(*) FILTER (WHERE trace_status IN ('sent', 'open', 'reply')) AS delivered,
                      count(*) FILTER (WHERE trace_status IN ('open', 'reply')) AS opened,
                      count(*) FILTER (WHERE links_click_datetime IS NOT NULL) AS clicked,
                      count(*) FILTER (WHERE trace_status = 'reply') AS replied,
                      count(*) FILTER (WHERE trace_status = 'bounce') AS bounced,
                      count(*) FILTER (WHERE trace_status = 'error') AS errored,
                      count(*) FILTER (WHERE trace_status = 'cancel') AS canceled
               FROM mailing.mailing_traces
               WHERE mailing_id = ANY($1)
                 AND (metadata->>'deleted_at') IS NULL
               GROUP BY mailing_id"#,
        )
        .bind(mailing_ids)
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, total, sent, delivered, opened, clicked, replied, bounced, errored, canceled)| {
                (
                    id,
                    MailingTraceCounts {
                        total, sent, delivered, opened, clicked, replied, bounced, errored, canceled,
                    },
                )
            })
            .collect())
    }

    /// The source-grouped audit read (MVX-4): aggregate the trace metrics of
    /// the given mailings grouped by the engagement SOURCE each mailing
    /// cites — winner metrics attribute by utm source, not campaign. The
    /// `campaigns` arm counts the DISTINCT campaigns served by each source
    /// across ALL live mailings citing it (not just the queried set — the
    /// shared-source property belongs to the source, not the query), so the
    /// read can surface the shared-source caveat: one source may serve
    /// several campaigns, and its totals are NOT any single campaign's.
    /// Returns (source_id, campaigns, mailings, total, sent, opened,
    /// clicked, replied). Mailings citing no source are out of this read —
    /// they attribute nothing.
    #[allow(clippy::type_complexity)]
    pub async fn source_grouped_counts(
        conn: &mut PgConnection,
        mailing_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, i64, i64, i64, i64, i64, i64, i64)>, sqlx::Error> {
        sqlx::query_as::<_, (Uuid, i64, i64, i64, i64, i64, i64, i64)>(
            r#"WITH scope AS (
                   SELECT m.id, m.source_id
                   FROM mailing.mailings m
                   WHERE m.id = ANY($1)
                     AND m.source_id IS NOT NULL
                     AND (m.metadata->>'deleted_at') IS NULL
               ),
               per_source AS (
                   SELECT s.source_id,
                          count(DISTINCT t.mailing_id) AS mailings,
                          count(*) FILTER (WHERE t.trace_status <> 'cancel') AS total,
                          count(*) FILTER (WHERE t.sent_datetime IS NOT NULL) AS sent,
                          count(*) FILTER (WHERE t.trace_status IN ('open', 'reply')) AS opened,
                          count(*) FILTER (WHERE t.links_click_datetime IS NOT NULL) AS clicked,
                          count(*) FILTER (WHERE t.trace_status = 'reply') AS replied
                   FROM scope s
                   JOIN mailing.mailing_traces t ON t.mailing_id = s.id
                    AND (t.metadata->>'deleted_at') IS NULL
                   GROUP BY s.source_id
               ),
               source_campaigns AS (
                   SELECT m.source_id, count(DISTINCT m.campaign_id) AS campaigns
                   FROM mailing.mailings m
                   WHERE m.source_id IN (SELECT source_id FROM per_source)
                     AND m.campaign_id IS NOT NULL
                     AND (m.metadata->>'deleted_at') IS NULL
                   GROUP BY m.source_id
               )
               SELECT p.source_id, COALESCE(c.campaigns, 0), p.mailings, p.total,
                      p.sent, p.opened, p.clicked, p.replied
               FROM per_source p
               LEFT JOIN source_campaigns c ON c.source_id = p.source_id
               ORDER BY p.source_id"#,
        )
        .bind(mailing_ids)
        .fetch_all(&mut *conn)
        .await
    }
}

/// The delivery-tracker pump's advance-pass row: one transient sms-type
/// trace with its tracker's verdict. `failure_type` arrives as the
/// NotificationFailureType name; the pump maps it into the trace failure
/// vocabulary before any verb call (a non-member cast would abort the
/// caller's transaction, so the whitelist lives in the service).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SmsTraceVerdict {
    pub trace_id: Uuid,
    pub tracker_state: String,
    pub failure_type: Option<String>,
    pub failure_reason: Option<String>,
}

/// The trace-row projection the write verbs return to callers.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TraceRow {
    pub id: Uuid,
    pub trace_status: String,
    pub failure_type: Option<String>,
    pub sent_datetime: Option<DateTime<Utc>>,
    pub open_datetime: Option<DateTime<Utc>>,
    pub reply_datetime: Option<DateTime<Utc>>,
    pub links_click_datetime: Option<DateTime<Utc>>,
    pub mail_id: Option<Uuid>,
}

/// The public trace-route projection: everything the `/r/:code/m/:trace`
/// family reads — the consistency half (`campaign_id`, the denormalized
/// copy minted with the trace) and the unsubscribe anchors (`mailing_id`
/// for the targeted-audience walk, `recipient_email` for the opt-out).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RouteTraceRow {
    pub id: Uuid,
    pub trace_status: String,
    pub mailing_id: Uuid,
    pub campaign_id: Option<Uuid>,
    pub recipient_email: String,
}
