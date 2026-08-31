-- Down for the cycle-44 bridge targets' delta.
--
-- The enum VALUES added by the up file CANNOT be dropped in place (Postgres
-- has no ALTER TYPE ... DROP VALUE) — they remain, inert until a write uses
-- them, exactly like every enum-add stamp in this tree. The column drops.

ALTER TABLE mailing.mailing_ab_tests
    DROP COLUMN IF EXISTS winner_selection_sms;

-- Enum values ('crm_lead' / 'crm_deal' / 'selling_customer' on
-- mailing_target_model, 'sale_invoiced_amount' on ab_winner_selection):
-- remain by necessity; no statement here.
SELECT 1;
