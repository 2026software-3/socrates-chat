CREATE TABLE book_recommendations (
    conversation_id uuid PRIMARY KEY REFERENCES conversations (id) ON DELETE CASCADE,
    status text NOT NULL CHECK (status IN ('pending', 'ready', 'no_results', 'failed', 'interrupted')),
    attempt bigint NOT NULL DEFAULT 1 CHECK (attempt > 0),
    started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    rules_version text NOT NULL,
    error_code text,
    snapshot jsonb,
    CHECK ((status IN ('ready', 'no_results')) = (snapshot IS NOT NULL)),
    CHECK ((status = 'failed') = (error_code IS NOT NULL))
);

CREATE INDEX book_recommendations_pending_idx ON book_recommendations (conversation_id)
    WHERE status = 'pending';

-- Invalidate in the same transaction as the eligibility change, including a temporary
-- archive or roster removal that is reverted before the worker tries to save.
CREATE FUNCTION interrupt_book_recommendations() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE book_recommendations r SET status = 'interrupted'
    FROM conversations c
    WHERE r.conversation_id = c.id AND r.status = 'pending'
      AND CASE TG_TABLE_NAME
          WHEN 'classrooms' THEN c.classroom_id = (to_jsonb(OLD)->>'id')::uuid
          WHEN 'activities' THEN c.activity_id = (to_jsonb(OLD)->>'id')::uuid
          WHEN 'users' THEN c.user_id = (to_jsonb(OLD)->>'id')::uuid
          WHEN 'enrollments' THEN c.classroom_id = (to_jsonb(OLD)->>'classroom_id')::uuid
                               AND c.user_id = (to_jsonb(OLD)->>'user_id')::uuid
      END;
    RETURN NULL;
END;
$$;

CREATE TRIGGER interrupt_recommendations_on_classroom_archive
    AFTER UPDATE OF is_archived ON classrooms FOR EACH ROW
    WHEN (NEW.is_archived AND NOT OLD.is_archived) EXECUTE FUNCTION interrupt_book_recommendations();

CREATE TRIGGER interrupt_recommendations_on_activity_archive
    AFTER UPDATE OF is_archived ON activities FOR EACH ROW
    WHEN (NEW.is_archived AND NOT OLD.is_archived) EXECUTE FUNCTION interrupt_book_recommendations();

CREATE TRIGGER interrupt_recommendations_on_account_disable
    AFTER UPDATE OF disabled_at ON users FOR EACH ROW
    WHEN (NEW.disabled_at IS NOT NULL AND OLD.disabled_at IS NULL) EXECUTE FUNCTION interrupt_book_recommendations();

CREATE TRIGGER interrupt_recommendations_on_roster_removal
    AFTER DELETE ON enrollments FOR EACH ROW EXECUTE FUNCTION interrupt_book_recommendations();
