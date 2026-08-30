-- Down for the SMS-channel hardening constraints: drop the three CHECKs.
ALTER TABLE mailing.mailing_traces
    DROP CONSTRAINT IF EXISTS trace_channel_link_exclusive;
ALTER TABLE mailing.mailing_traces
    DROP CONSTRAINT IF EXISTS trace_recipient_phone_canonical;
ALTER TABLE mailing.mailings
    DROP CONSTRAINT IF EXISTS mailing_sms_body_present;
