-- The SMS-channel overlay's schema delta (columns + enum values; the
-- channel's DB CHECKs ride the NEXT stamp — see
-- 20260830100001_mailing_sms_hardening).
--
-- Zero NEW tables: the channel rides the existing entities as overlay
-- columns + enum variants (schema/models/{mailing,trace,audience}.model.yaml
-- are the source of truth; this file is their DB expression, user_owned
-- under the migrations/*sms_channel_overlay* glob).
--
-- MECHANICS (hard constraint): NO statement in this file USES a newly
-- added enum value. Postgres refuses a new enum value inside the
-- transaction that added it, and both runners (sqlx::migrate's per-file
-- transaction; the behavior harness's single raw_sql batch) are
-- one-transaction-per-file. The statements here are value-only ALTER TYPEs
-- and value-free DDL; the CHECKs that compare against 'sms' live in the
-- next stamp's transaction. Precedent: the bucket module's value-only
-- variant stamp — data translation rides the next stamp.

-- Channel selectors.
ALTER TYPE mailing_type ADD VALUE IF NOT EXISTS 'sms';
ALTER TYPE trace_type ADD VALUE IF NOT EXISTS 'sms';

-- The SMS failure vocabulary (the trace model's TraceFailureType delta):
-- send-side / mass-mode codes, then the delivery-report codes. Mirrors the
-- mail channel's NotificationFailureType keys verbatim; the bridge-bound
-- twilio_* codes are NOT ported. 'sms_blacklist' is NOT repeated here —
-- 20260426220013 already added it.
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_number_missing';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_number_format';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_country_not_supported';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_registration_needed';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_credit';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_server';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_acc';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_duplicate';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_optout';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_expired';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_invalid_destination';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_not_allowed';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_not_delivered';
ALTER TYPE trace_failure_type ADD VALUE IF NOT EXISTS 'sms_rejected';

-- The SMS body (required at launch for mailing_type='sms'; the launch
-- verb pre-checks and the hardening CHECK owns raw SQL).
ALTER TABLE mailing.mailings
    ADD COLUMN IF NOT EXISTS body_plaintext TEXT;

-- The trace's gateway seam + the canonical phone twin of recipient_email.
-- sms_uuid is VARCHAR(64) matching the model's declared @max(64); the
-- values themselves are 32-char uuid-simple strings.
ALTER TABLE mailing.mailing_traces
    ADD COLUMN IF NOT EXISTS sms_uuid VARCHAR(64),
    ADD COLUMN IF NOT EXISTS recipient_phone VARCHAR(16);

-- The contact's RAW as-entered mobile (canonicalization happens at send
-- through mail's public phone_format; the canonical lands only on traces).
ALTER TABLE mailing.mailing_contacts
    ADD COLUMN IF NOT EXISTS phone VARCHAR(64);

-- The pump join arm + the phone-keyed lookup arm.
CREATE INDEX IF NOT EXISTS idx_mailing_traces_sms_uuid
    ON mailing.mailing_traces (sms_uuid);
CREATE INDEX IF NOT EXISTS idx_mailing_traces_recipient_phone
    ON mailing.mailing_traces (recipient_phone);
