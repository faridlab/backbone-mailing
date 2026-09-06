-- Down for the mass_mailing_event bridge target's delta.
--
-- The enum VALUE added by the up file CANNOT be dropped in place (Postgres
-- has no ALTER TYPE ... DROP VALUE) — it remains, inert until a write uses
-- it, exactly like every enum-add stamp in this tree (the bridge_targets
-- precedent).

-- Enum value ('event_registration' on mailing_target_model): remains by
-- necessity; no statement here.
SELECT 1;
