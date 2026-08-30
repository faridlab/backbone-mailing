-- Down: drop mailing.mailing_contacts table
DROP TABLE IF EXISTS mailing.mailing_contacts CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_contacts_audit_timestamp() CASCADE;
