DROP TRIGGER IF EXISTS mailings_share_named_lists ON mailing.mailings;
DROP FUNCTION IF EXISTS mailing.mailings_share_named_lists();
DROP TRIGGER IF EXISTS mailing_audiences_list_archive_guard ON mailing.mailing_audiences;
DROP FUNCTION IF EXISTS mailing.mailing_audiences_list_archive_guard();
DROP FUNCTION IF EXISTS mailing.audience_cited_by_live_mailing(uuid);
