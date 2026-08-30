-- Down: drop mailing.mailing_opt_out_reasons table
DROP TABLE IF EXISTS mailing.mailing_opt_out_reasons CASCADE;
DROP FUNCTION IF EXISTS mailing.mailing_opt_out_reasons_audit_timestamp() CASCADE;
