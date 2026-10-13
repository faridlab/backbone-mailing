-- Down: drop the declared foreign keys for mailing module
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS fk_mailing_traces_mailing_id;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS fk_mailings_ab_test_id;
ALTER TABLE IF EXISTS mailing.mailing_subscriptions DROP CONSTRAINT IF EXISTS fk_mailing_subscriptions_opt_out_reason_id;
ALTER TABLE IF EXISTS mailing.mailing_subscriptions DROP CONSTRAINT IF EXISTS fk_mailing_subscriptions_mailing_audience_id;
ALTER TABLE IF EXISTS mailing.mailing_subscriptions DROP CONSTRAINT IF EXISTS fk_mailing_subscriptions_contact_id;
ALTER TABLE IF EXISTS mailing.mailing_ab_tests DROP CONSTRAINT IF EXISTS fk_mailing_ab_tests_winner_mailing_id;
