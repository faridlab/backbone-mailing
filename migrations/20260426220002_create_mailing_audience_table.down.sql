-- Down: drop mailing.mailing_audiences table
DROP TABLE IF EXISTS mailing.mailing_audiences CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_audiences_audit_timestamp() CASCADE;
