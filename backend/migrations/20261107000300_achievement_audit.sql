-- S-07.3: preserve existing audit records while allowing achievement changes.
ALTER TABLE audit_logs
    DROP CONSTRAINT audit_logs_action_check,
    ADD CONSTRAINT audit_logs_action_check CHECK (action IN (
        'login', 'account_disabled', 'account_enabled', 'student_deleted',
        'teacher_added', 'teacher_removed', 'roster_added', 'roster_removed', 'password_reset',
        'classroom_archived', 'classroom_restored', 'classroom_deleted',
        'activity_archived', 'activity_restored', 'activity_deleted',
        'achievement_created', 'achievement_updated'
    )),
    ADD COLUMN old_value jsonb,
    ADD COLUMN new_value jsonb;
