-- Down: drop mailing.mailing_filters table
DROP TABLE IF EXISTS mailing.mailing_filters CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_filters_audit_timestamp() CASCADE;
