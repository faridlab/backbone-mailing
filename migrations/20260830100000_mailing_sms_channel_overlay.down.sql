-- Down for the SMS-channel overlay's columns + indexes.
--
-- The enum VALUES added by the up file CANNOT be dropped in place (Postgres
-- has no ALTER TYPE ... DROP VALUE) — they remain, inert until a write uses
-- them, exactly like every enum-add stamp in this tree (see
-- 20260426220013's no-honest-down note). The columns and indexes do drop.

DROP INDEX IF EXISTS mailing.idx_mailing_traces_recipient_phone;
DROP INDEX IF EXISTS mailing.idx_mailing_traces_sms_uuid;

ALTER TABLE mailing.mailing_contacts
    DROP COLUMN IF EXISTS phone;

ALTER TABLE mailing.mailing_traces
    DROP COLUMN IF EXISTS recipient_phone,
    DROP COLUMN IF EXISTS sms_uuid;

ALTER TABLE mailing.mailings
    DROP COLUMN IF EXISTS body_plaintext;

-- Enum values ('sms' on mailing_type/trace_type, the 14 sms_* failure
-- codes): remain by necessity; no statement here.
SELECT 1;
