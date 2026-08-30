-- Down: drop mailing.mailings table
DROP TABLE IF EXISTS mailing.mailings CASCADE;
DROP FUNCTION IF EXISTS mailing.mailings_audit_timestamp() CASCADE;
