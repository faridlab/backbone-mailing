-- Hand-written hardening constraints for mailing.mailings.
--
-- The schema DSL cannot express row-level CHECK constraints, so the two
-- cross-column invariants are installed here (user_owned: this file matches
-- the migrations/*mailing*hardening* glob in metaphor.codegen.yaml and is
-- never touched by the generator).
--
-- G-MM1: the A/B fragment percentage must stay inside 0..100. The write
--        service validates it too; this CHECK is the guard that fires on
--        raw SQL and any future writer that skips the verb.
--
-- G-MM2: a mail-type mailing must carry a sender address. The column is
--        already NOT NULL (the DSL's @required), so today this is redundant
--        defense in depth — it becomes the load-bearing channel-conditional
--        guard if a later channel variant ever relaxes that nullability.

ALTER TABLE mailing.mailings
    ADD CONSTRAINT chk_mailings_ab_testing_pc CHECK (ab_testing_pc BETWEEN 0 AND 100);

ALTER TABLE mailing.mailings
    ADD CONSTRAINT chk_mailings_email_from_for_mail
    CHECK (mailing_type <> 'mail' OR email_from IS NOT NULL);
