-- Down: drop mailing.mailing_traces table
DROP TABLE IF EXISTS mailing.mailing_traces CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_traces_audit_timestamp() CASCADE;
