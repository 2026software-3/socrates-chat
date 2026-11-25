-- Keep edition identities and all existing list selections; infer no work grouping.
CREATE TABLE book_works (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    title text NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 255),
    is_active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO book_works(id,title,created_at,updated_at)
SELECT id,title,created_at,updated_at FROM books;
ALTER TABLE books ADD COLUMN work_id uuid;
UPDATE books SET work_id=id;
ALTER TABLE books ALTER COLUMN work_id SET NOT NULL;
ALTER TABLE books ADD CONSTRAINT books_work_id_fkey
    FOREIGN KEY(work_id) REFERENCES book_works(id) ON DELETE CASCADE;
CREATE INDEX books_work_id_idx ON books(work_id);
ALTER TABLE topic_books DROP CONSTRAINT topic_books_book_id_fkey;
ALTER TABLE topic_books ADD CONSTRAINT topic_books_book_id_fkey
    FOREIGN KEY(book_id) REFERENCES book_works(id) ON DELETE CASCADE;
ALTER TABLE activity_topic_books DROP CONSTRAINT activity_topic_books_book_id_fkey;
ALTER TABLE activity_topic_books ADD CONSTRAINT activity_topic_books_book_id_fkey
    FOREIGN KEY(book_id) REFERENCES book_works(id) ON DELETE CASCADE;
COMMENT ON COLUMN topic_books.book_id IS 'Work ID (historical column name retained)';
COMMENT ON COLUMN activity_topic_books.book_id IS 'Work ID (historical column name retained)';

-- Every creation path (including the per-row CSV transaction) atomically creates a work.
CREATE FUNCTION create_edition_work() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.work_id IS NULL THEN
        INSERT INTO book_works(id,title) VALUES (NEW.id,NEW.title);
        NEW.work_id := NEW.id;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER create_edition_work BEFORE INSERT ON books
    FOR EACH ROW EXECUTE FUNCTION create_edition_work();

-- Only deleting the final edition removes its work. Moving an edition preserves the source.
CREATE FUNCTION remove_empty_deleted_edition_work() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM book_works WHERE id=OLD.work_id
        AND NOT EXISTS (SELECT 1 FROM books WHERE work_id=OLD.work_id);
    RETURN OLD;
END;
$$;
CREATE TRIGGER remove_empty_deleted_edition_work AFTER DELETE ON books
    FOR EACH ROW EXECUTE FUNCTION remove_empty_deleted_edition_work();

CREATE OR REPLACE FUNCTION copy_activity_topic_books() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM topics WHERE id=NEW.topic_id FOR SHARE;
    INSERT INTO activity_topic_books(activity_id,topic_id,book_id)
    SELECT NEW.activity_id,NEW.topic_id,w.id
    FROM topic_books tb JOIN book_works w ON w.id=tb.book_id
    WHERE tb.topic_id=NEW.topic_id AND w.is_active
      AND EXISTS (SELECT 1 FROM books b WHERE b.work_id=w.id AND b.is_active)
    ORDER BY w.id FOR SHARE OF w;
    RETURN NEW;
END;
$$;
