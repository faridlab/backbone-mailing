-- The cycle-44 bridge targets' schema delta (enum values + the A/B
-- control's parallel sms winner axis).
--
-- Zero NEW tables: the mass_mailing_crm / mass_mailing_sale bridges land as
-- CLOSED-ENUM target values + a nullable column (schema/models/{mailing,
-- abtesting}.model.yaml are the source of truth; this file is their DB
-- expression, user_owned under the migrations/*bridge_targets* glob).
--
-- MECHANICS (hard constraint, the sms_channel_overlay precedent): NO
-- statement in this file USES a newly added enum value. Postgres refuses a
-- new enum value inside the transaction that added it, and both runners
-- (sqlx::migrate's per-file transaction; the behavior harness's single
-- raw_sql batch) are one-transaction-per-file. The statements here are
-- value-only ALTER TYPEs and a value-free column add (nullable, no
-- default literal).

-- The bridge targets (mass_mailing_crm: lead + deal; mass_mailing_sale:
-- customers). The *_sms twins ride the SAME values on the existing sms
-- channel — no twin enum, no twin module.
ALTER TYPE mailing_target_model ADD VALUE IF NOT EXISTS 'crm_lead';
ALTER TYPE mailing_target_model ADD VALUE IF NOT EXISTS 'crm_deal';
ALTER TYPE mailing_target_model ADD VALUE IF NOT EXISTS 'selling_customer';

-- The bridge winner metric (the mass_mailing_sale axis, attributed by utm
-- source through the declared billing-side seam port).
ALTER TYPE ab_winner_selection ADD VALUE IF NOT EXISTS 'sale_invoiced_amount';

-- The parallel sms-channel ranking axis (upstream's
-- ab_testing_sms_winner_selection): the SAME enum on its own nullable
-- column — unset means the sms variants rank by the mail axis.
ALTER TABLE mailing.mailing_ab_tests
    ADD COLUMN IF NOT EXISTS winner_selection_sms ab_winner_selection;
