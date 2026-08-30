-- Down: drop mailing.mailing_subscriptions table
DROP TABLE IF EXISTS mailing.mailing_subscriptions CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_subscriptions_audit_timestamp() CASCADE;
