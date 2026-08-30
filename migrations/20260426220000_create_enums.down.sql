-- Down: drop enum types for mailing module
DROP TYPE IF EXISTS trace_failure_type CASCADE;
DROP TYPE IF EXISTS trace_status CASCADE;
DROP TYPE IF EXISTS trace_type CASCADE;
DROP TYPE IF EXISTS mailing_target_model CASCADE;
DROP TYPE IF EXISTS mailing_type CASCADE;
DROP TYPE IF EXISTS mailing_schedule_type CASCADE;
DROP TYPE IF EXISTS mailing_state CASCADE;
DROP TYPE IF EXISTS ab_winner_selection CASCADE;
