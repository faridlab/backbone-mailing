-- Reverse of 20260426220012_mailing_hardening_constraints.up.sql:
-- drop the two hardening CHECKs. Table and columns are untouched.

ALTER TABLE mailing.mailings
    DROP CONSTRAINT IF EXISTS chk_mailings_email_from_for_mail;

ALTER TABLE mailing.mailings
    DROP CONSTRAINT IF EXISTS chk_mailings_ab_testing_pc;
