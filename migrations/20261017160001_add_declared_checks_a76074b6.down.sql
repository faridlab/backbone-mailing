-- Down: drop the declared rule constraints for mailing module
ALTER TABLE IF EXISTS mailing.mailing_filters DROP CONSTRAINT IF EXISTS ck_mailing_filters_name_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_filters DROP CONSTRAINT IF EXISTS ck_mailing_filters_name_required__required;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_id__trace_recipient_required;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_message_id_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_phone_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_sms_uuid_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_email_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_email_required__required;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_id_required__required;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_model_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_recipient_model_required__required;
ALTER TABLE IF EXISTS mailing.mailing_traces DROP CONSTRAINT IF EXISTS ck_mailing_traces_mailing_id_required__required;
DO $$
BEGIN
    IF to_regclass('mailing.uq_mailing_traces_recipient_id__unique') IS NOT NULL AND to_regclass('mailing.idx_mailing_traces_mailing_id_recipient_id') IS NULL THEN
        ALTER INDEX mailing.uq_mailing_traces_recipient_id__unique RENAME TO idx_mailing_traces_mailing_id_recipient_id;
    END IF;
END $$;

ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_ab_testi_d3c7de78__campaign_required_for_ab_testing;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_mailing_type__email_from_required;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_reply_to_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_email_from_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_email_from_required__required;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_body_html_required__required;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_preview_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_subject_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailings DROP CONSTRAINT IF EXISTS ck_mailings_subject_required__required;
ALTER TABLE IF EXISTS mailing.mailing_opt_out_reasons DROP CONSTRAINT IF EXISTS ck_mailing_opt_out_reasons_name_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_opt_out_reasons DROP CONSTRAINT IF EXISTS ck_mailing_opt_out_reasons_name_required__required;
ALTER TABLE IF EXISTS mailing.mailing_subscriptions DROP CONSTRAINT IF EXISTS ck_mailing_subscriptions_mailing_audience_id_required__required;
ALTER TABLE IF EXISTS mailing.mailing_subscriptions DROP CONSTRAINT IF EXISTS ck_mailing_subscriptions_contact_id_required__required;
DO $$
BEGIN
    IF to_regclass('mailing.uq_mailing_subscriptions_mailing_audience_id__unique') IS NOT NULL AND to_regclass('mailing.idx_mailing_subscriptions_contact_id_mailing_audience_id') IS NULL THEN
        ALTER INDEX mailing.uq_mailing_subscriptions_mailing_audience_id__unique RENAME TO idx_mailing_subscriptions_contact_id_mailing_audience_id;
    END IF;
END $$;

ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_phone_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_country_code_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_company_name_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_email_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_email_required__required;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_last_name_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_first_name_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_contacts DROP CONSTRAINT IF EXISTS ck_mailing_contacts_name_max_length__max_length;
DO $$
BEGIN
    IF to_regclass('mailing.uq_mailing_contacts_email__unique') IS NOT NULL AND to_regclass('mailing.idx_mailing_contacts_email') IS NULL THEN
        ALTER INDEX mailing.uq_mailing_contacts_email__unique RENAME TO idx_mailing_contacts_email;
    END IF;
END $$;

ALTER TABLE IF EXISTS mailing.mailing_audiences DROP CONSTRAINT IF EXISTS ck_mailing_audiences_name_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_audiences DROP CONSTRAINT IF EXISTS ck_mailing_audiences_name_required__required;
ALTER TABLE IF EXISTS mailing.mailing_ab_tests DROP CONSTRAINT IF EXISTS ck_mailing_ab_tests_sampling_seed_max_length__max_length;
ALTER TABLE IF EXISTS mailing.mailing_ab_tests DROP CONSTRAINT IF EXISTS ck_mailing_ab_tests_sampling_seed_required__required;
ALTER TABLE IF EXISTS mailing.mailing_ab_tests DROP CONSTRAINT IF EXISTS ck_mailing_ab_tests_campaign_id_required__required;
DO $$
BEGIN
    IF to_regclass('mailing.uq_mailing_ab_tests_campaign_id__unique') IS NOT NULL AND to_regclass('mailing.idx_mailing_ab_tests_campaign_id') IS NULL THEN
        ALTER INDEX mailing.uq_mailing_ab_tests_campaign_id__unique RENAME TO idx_mailing_ab_tests_campaign_id;
    END IF;
END $$;

