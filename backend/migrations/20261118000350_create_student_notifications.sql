CREATE TABLE student_notifications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('student_number_corrected')),
    created_at timestamptz NOT NULL DEFAULT now(),
    read_at timestamptz
);

CREATE INDEX student_notifications_unread_idx
    ON student_notifications (user_id, created_at, id) WHERE read_at IS NULL;
