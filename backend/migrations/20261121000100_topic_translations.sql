-- Original titles and ordered questions already contain the zh-TW source.
-- Missing language entries represent pending translations, including for existing rows.
ALTER TABLE topics ADD COLUMN translations jsonb NOT NULL DEFAULT '{}';
