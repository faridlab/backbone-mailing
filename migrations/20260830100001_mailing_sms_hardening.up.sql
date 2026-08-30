-- Hand-written SMS-channel hardening constraints (user_owned: this file
-- matches the migrations/*mailing*hardening* glob in metaphor.codegen.yaml
-- and is never touched by the generator).
--
-- Split from 20260830100000_mailing_sms_channel_overlay INTO A SEPARATE
-- STAMP because every CHECK here USES the 'sms' enum value — Postgres
-- refuses a new enum value inside the transaction that added it, and both
-- migration runners are one-transaction-per-file. This transaction is
-- strictly later than the one that added the value.
--
-- mailing_sms_body_present: an SMS mailing must carry its body. The launch
--        verb pre-checks; this CHECK is the guard that fires on raw SQL
--        and any future writer that skips the verb (the G-class posture).
--
-- trace_recipient_phone_canonical: the phone twin of recipient_email holds
--        EXACTLY the sanitizer's canonical form — E164Number's invariant
--        ('+', a 1..=9 country digit, then 6..=14 digits) enforced at the
--        DB, the class the schema DSL cannot express.
--
-- trace_channel_link_exclusive: a trace rides EXACTLY ONE channel — a mail
--        link and an sms link never coexist. NOTE the deliberate absence of
--        the converse arm ("sms must have uuid"): a suppression-canceled
--        SMS trace is minted pre-canceled with NO gateway row, so
--        sms_uuid IS NULL on legitimate sms traces.

ALTER TABLE mailing.mailings
    ADD CONSTRAINT mailing_sms_body_present
    CHECK (mailing_type <> 'sms' OR body_plaintext IS NOT NULL);

ALTER TABLE mailing.mailing_traces
    ADD CONSTRAINT trace_recipient_phone_canonical
    CHECK (recipient_phone IS NULL OR recipient_phone ~ '^\+[1-9]\d{6,14}$');

ALTER TABLE mailing.mailing_traces
    ADD CONSTRAINT trace_channel_link_exclusive
    CHECK (mail_id IS NULL OR sms_uuid IS NULL);
