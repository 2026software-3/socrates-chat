-- 加速教室學生「是否曾登入」的統計查詢
CREATE INDEX audit_logs_login_actor_idx
ON audit_logs (actor_id)
WHERE action = 'login';
