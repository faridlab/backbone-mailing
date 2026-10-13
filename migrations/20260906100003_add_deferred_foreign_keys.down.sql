-- Down: drop deferred foreign keys for mailing module
ALTER TABLE mailing.mailings DROP CONSTRAINT IF EXISTS fk_mailings_ab_test_id;
