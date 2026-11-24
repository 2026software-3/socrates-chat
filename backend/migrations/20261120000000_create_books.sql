-- F-14.3: global book catalog, topic defaults and independent activity-topic lists.
CREATE TABLE books (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    title text NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 255),
    authors text CHECK (char_length(authors) <= 1000),
    language text,
    publication_year text,
    isbn text,
    source_url text CHECK (char_length(source_url) <= 8192),
    description text CHECK (char_length(description) <= 5000),
    recommendation_note text CHECK (char_length(recommendation_note) <= 2000),
    external_sources jsonb NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(external_sources) = 'array'),
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    is_active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX books_created_by_idx ON books(created_by);
CREATE INDEX books_isbn_idx ON books(isbn) WHERE isbn IS NOT NULL;

CREATE TABLE topic_books (
    topic_id uuid NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    book_id uuid NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    PRIMARY KEY(topic_id,book_id)
);
CREATE INDEX topic_books_book_idx ON topic_books(book_id);

CREATE TABLE activity_topic_books (
    activity_id uuid NOT NULL,
    topic_id uuid NOT NULL,
    book_id uuid NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    PRIMARY KEY(activity_id,topic_id,book_id),
    FOREIGN KEY(activity_id,topic_id) REFERENCES activity_topics(activity_id,topic_id) ON DELETE CASCADE
);
CREATE INDEX activity_topic_books_book_idx ON activity_topic_books(book_id);

-- Reordering must not delete retained activity_topics, which would cascade customized lists.
ALTER TABLE activity_topics DROP CONSTRAINT activity_topics_position_unique;
ALTER TABLE activity_topics ADD CONSTRAINT activity_topics_position_unique
    UNIQUE(activity_id,position) DEFERRABLE INITIALLY IMMEDIATE;

-- Copy once when a topic joins an activity, not when its default list later changes.
CREATE FUNCTION copy_activity_topic_books() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- List writes lock this topic FOR UPDATE; serialize the snapshot with those writes.
    PERFORM id FROM topics WHERE id=NEW.topic_id FOR SHARE;
    INSERT INTO activity_topic_books(activity_id,topic_id,book_id)
    SELECT NEW.activity_id,NEW.topic_id,b.id
    FROM topic_books tb JOIN books b ON b.id=tb.book_id
    WHERE tb.topic_id=NEW.topic_id AND b.is_active
    ORDER BY b.id FOR SHARE OF b;
    RETURN NEW;
END;
$$;
CREATE TRIGGER copy_activity_topic_books AFTER INSERT ON activity_topics
    FOR EACH ROW EXECUTE FUNCTION copy_activity_topic_books();
