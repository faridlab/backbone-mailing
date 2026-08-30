-- The delivery-tracker pump's scan arm (declared in
-- schema/models/trace.model.yaml — the SSoT; this file is its DB
-- expression, user_owned under the migrations/*sms_pump_scan_index* glob).
--
-- Split from 20260830100000_mailing_sms_channel_overlay rather than edited
-- into it: value-free DDL in its own stamp keeps the overlay stamp's
-- value-only discipline intact and lands without disturbing the sibling
-- statements' review state.
--
-- Why composite and not the single-column trace_status index the base
-- schema already carries: the pump scans ONLY sms-type transient rows, and
-- the mail channel's outgoing traces (awaiting SMTP verdicts) share the
-- transient statuses with them — the composite keeps that mixed fleet off
-- the pump's scan entirely.

CREATE INDEX IF NOT EXISTS idx_mailing_traces_trace_type_status
    ON mailing.mailing_traces (trace_type, trace_status);
