-- Down: drop mailing.mailing_ab_tests table
DROP TABLE IF EXISTS mailing.mailing_ab_tests CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_ab_tests_audit_timestamp() CASCADE;
