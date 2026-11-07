-- S-07.4: two configurable periodic completion tasks.
CREATE TABLE tasks (
    id text PRIMARY KEY,
    period text NOT NULL UNIQUE CHECK (period IN ('daily', 'weekly')),
    goal integer NOT NULL CHECK (goal > 0),
    reward integer NOT NULL CHECK (reward > 0)
);
INSERT INTO tasks (id, period, goal, reward) VALUES
    ('daily_dialogue', 'daily', 1, 5),
    ('weekly_dialogue', 'weekly', 5, 20);

-- Extend the existing audit action constraint without removing actions added by another slice.
DO $$
DECLARE existing_rule text;
BEGIN
    SELECT pg_get_expr(conbin, conrelid) INTO STRICT existing_rule
    FROM pg_constraint WHERE conrelid = 'audit_logs'::regclass AND conname = 'audit_logs_action_check';
    ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check;
    EXECUTE format('ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action = %L)', existing_rule, 'task_updated');
END;
$$;
