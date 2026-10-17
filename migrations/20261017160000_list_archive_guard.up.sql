-- A mailing list is not archived (soft deleted, turned inactive or deleted) while a mailing still
-- in draft, in queue or sending names it anywhere in its recipient domain, an `in` or `not in`
-- term included. The trigger holds it for every writer, the generic CRUD surface among them.
--
-- A mailing write that names lists takes a share lock on each, so an archive and a mailing naming
-- the list take turns: the archive either sees the mailing or the mailing waits for it.
CREATE OR REPLACE FUNCTION mailing.audience_cited_by_live_mailing(audience uuid) RETURNS boolean AS $$
    SELECT EXISTS (
        SELECT 1 FROM mailing.mailings m
         WHERE m.state IN ('draft', 'in_queue', 'sending')
           AND (m.metadata->>'deleted_at') IS NULL
           AND jsonb_path_exists(m.mailing_domain, '$.** ? (@ == $id)', jsonb_build_object('id', audience::text))
    );
$$ LANGUAGE sql STABLE;

CREATE OR REPLACE FUNCTION mailing.mailing_audiences_list_archive_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'UPDATE'
       AND NOT ((OLD.active AND NOT NEW.active)
             OR ((OLD.metadata->>'deleted_at') IS NULL AND (NEW.metadata->>'deleted_at') IS NOT NULL)) THEN
        RETURN NEW;
    END IF;
    IF mailing.audience_cited_by_live_mailing(OLD.id) THEN
        RAISE EXCEPTION 'list_in_use: mailing list % is named by a mailing not yet done', OLD.id
            USING ERRCODE = 'check_violation', CONSTRAINT = 'ck_mailing_audiences_list_archive_guard__list_in_use';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS mailing_audiences_list_archive_guard ON mailing.mailing_audiences;
CREATE TRIGGER mailing_audiences_list_archive_guard
    BEFORE UPDATE OR DELETE ON mailing.mailing_audiences
    FOR EACH ROW EXECUTE FUNCTION mailing.mailing_audiences_list_archive_guard();

CREATE OR REPLACE FUNCTION mailing.mailings_share_named_lists() RETURNS trigger AS $$
BEGIN
    IF NEW.state IN ('draft', 'in_queue', 'sending') THEN
        PERFORM 1 FROM mailing.mailing_audiences a
         WHERE a.id IN (
               SELECT (v #>> '{}')::uuid
                 FROM jsonb_path_query(NEW.mailing_domain,
                      '$.** ? (@.type() == "string" && @ like_regex "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")') v)
         FOR SHARE OF a;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS mailings_share_named_lists ON mailing.mailings;
CREATE TRIGGER mailings_share_named_lists
    BEFORE INSERT OR UPDATE OF mailing_domain, state ON mailing.mailings
    FOR EACH ROW EXECUTE FUNCTION mailing.mailings_share_named_lists();
