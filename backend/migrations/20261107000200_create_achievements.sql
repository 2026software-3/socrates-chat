-- F-18.1 / S-07.3: achievement catalog (seeded, then maintained by admins), durable
-- completion events, and one-time unlock records. Awards go through the point ledger.
CREATE TABLE achievements (
    id text NOT NULL CHECK (id ~ '^[a-z][a-z0-9_]{0,63}$'),
    level integer NOT NULL CHECK (level > 0),
    condition_type text NOT NULL CHECK (condition_type IN (
        'completed_conversations', 'consecutive_days', 'dimension_score',
        'first_task_claim', 'first_purchase'
    )),
    threshold integer NOT NULL CHECK (
        CASE condition_type
            WHEN 'dimension_score' THEN threshold BETWEEN 0 AND 5
            WHEN 'first_task_claim' THEN threshold = 1
            WHEN 'first_purchase' THEN threshold = 1
            ELSE threshold > 0
        END
    ),
    points integer NOT NULL DEFAULT 10 CHECK (points >= 0),
    name_zh_tw text NOT NULL CHECK (btrim(name_zh_tw) <> ''),
    name_en text NOT NULL CHECK (btrim(name_en) <> ''),
    name_es text NOT NULL CHECK (btrim(name_es) <> ''),
    description_zh_tw text NOT NULL CHECK (btrim(description_zh_tw) <> ''),
    description_en text NOT NULL CHECK (btrim(description_en) <> ''),
    description_es text NOT NULL CHECK (btrim(description_es) <> ''),
    source_language text CHECK (source_language IN ('zh-TW', 'en', 'es')),
    pending_languages text[] NOT NULL DEFAULT ARRAY[]::text[] CHECK (pending_languages <@ ARRAY['zh-TW', 'en', 'es']::text[]),
    enabled boolean NOT NULL DEFAULT true,
    sort_order integer NOT NULL DEFAULT 0,
    PRIMARY KEY (id, level)
);

CREATE TABLE achievement_completion_events (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conversation_id uuid NOT NULL,
    completed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, conversation_id)
);
CREATE INDEX achievement_completion_events_user_time_idx
    ON achievement_completion_events (user_id, completed_at);

CREATE TABLE achievement_unlocks (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    achievement_id text NOT NULL,
    level integer NOT NULL CHECK (level > 0),
    unlocked_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, achievement_id, level),
    FOREIGN KEY (achievement_id, level) REFERENCES achievements(id, level)
);

-- Thresholds of one achievement must strictly increase with its level (S-07.3 2.3).
-- Checked when the transaction commits so several levels can be edited in one statement batch.
CREATE FUNCTION achievements_thresholds_increase() RETURNS trigger AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM achievements a
        JOIN achievements b ON b.id = a.id AND b.level > a.level AND b.threshold <= a.threshold
        WHERE a.id = NEW.id
    ) THEN
        RAISE EXCEPTION 'achievement % thresholds must strictly increase with level', NEW.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE CONSTRAINT TRIGGER achievements_thresholds_increase
    AFTER INSERT OR UPDATE ON achievements
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION achievements_thresholds_increase();

INSERT INTO achievements
    (id, level, condition_type, threshold, points, name_zh_tw, name_en, name_es,
     description_zh_tw, description_en, description_es, sort_order)
VALUES
    ('first_dialogue', 1, 'completed_conversations', 1, 10,
     '初試問答', 'First Dialogue', 'Primer diálogo',
     '完成你的第一場對話', 'Complete your first dialogue', 'Completa tu primer diálogo', 1),
    ('dialogue_path', 1, 'completed_conversations', 5, 10,
     '思辨之路', 'Path of Inquiry', 'Camino de la indagación',
     '累積完成 {n} 場對話', 'Complete {n} dialogues in total', 'Completa {n} diálogos en total', 2),
    ('dialogue_path', 2, 'completed_conversations', 20, 10,
     '思辨之路', 'Path of Inquiry', 'Camino de la indagación',
     '累積完成 {n} 場對話', 'Complete {n} dialogues in total', 'Completa {n} diálogos en total', 3),
    ('dialogue_path', 3, 'completed_conversations', 50, 10,
     '思辨之路', 'Path of Inquiry', 'Camino de la indagación',
     '累積完成 {n} 場對話', 'Complete {n} dialogues in total', 'Completa {n} diálogos en total', 4),
    ('sharp_reasoner', 1, 'dimension_score', 5, 10,
     '思辨高手', 'Sharp Reasoner', 'Razonamiento brillante',
     '在任一場對話中，任一維度拿到 5 分', 'Score 5 on any dimension in any dialogue', 'Obtén 5 puntos en cualquier dimensión de un diálogo', 5),
    ('three_day_streak', 1, 'consecutive_days', 3, 10,
     '連續三天', 'Three-Day Streak', 'Racha de tres días',
     '連續 3 天，每天至少完成 1 場對話', 'Complete at least one dialogue a day for 3 days in a row', 'Completa al menos un diálogo al día durante 3 días seguidos', 6),
    ('first_task_reward', 1, 'first_task_claim', 1, 10,
     '第一桶金', 'First Reward', 'Primera recompensa',
     '第一次領取任務獎勵', 'Claim a task reward for the first time', 'Reclama por primera vez la recompensa de una tarea', 7),
    ('first_character_part', 1, 'first_purchase', 1, 10,
     '時尚入門', 'First Outfit', 'Primer atuendo',
     '第一次用點數解鎖角色部件', 'Unlock a character part with points for the first time', 'Desbloquea por primera vez una pieza del personaje con puntos', 8);

-- 既有對話完成點數代表已產生總結。保留帳本時間作為完成時間，包含已刪除的對話。
INSERT INTO achievement_completion_events (user_id, conversation_id, completed_at)
SELECT user_id, source_id::uuid, created_at
FROM point_ledger
WHERE source_type = 'conversation'
ON CONFLICT DO NOTHING;

-- 對歷史完成事件立即判定並一次入帳，與即時判定使用相同唯一鍵。
WITH days AS (
    SELECT DISTINCT user_id, (completed_at AT TIME ZONE 'Asia/Taipei')::date AS day
    FROM achievement_completion_events
), grouped_days AS (
    SELECT user_id, day - (row_number() OVER (PARTITION BY user_id ORDER BY day))::int AS grp
    FROM days
), streaks AS (
    SELECT user_id, max(streak)::bigint AS longest
    FROM (SELECT user_id, count(*) AS streak FROM grouped_days GROUP BY user_id, grp) AS runs
    GROUP BY user_id
), totals AS (
    SELECT user_id, count(*)::bigint AS completed FROM achievement_completion_events GROUP BY user_id
), inserted AS (
    INSERT INTO achievement_unlocks (user_id, achievement_id, level)
    SELECT totals.user_id, a.id, a.level
    FROM totals JOIN streaks ON streaks.user_id = totals.user_id
    JOIN achievements a ON a.enabled AND (
        (a.condition_type = 'completed_conversations' AND a.threshold <= totals.completed)
        OR (a.condition_type = 'consecutive_days' AND a.threshold <= streaks.longest)
    )
    ON CONFLICT DO NOTHING
    RETURNING user_id, achievement_id, level
)
INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id)
SELECT i.user_id, a.points, 'Achievement ' || i.achievement_id || ' level ' || i.level,
       'achievement', i.achievement_id || ':' || i.level
FROM inserted i JOIN achievements a ON a.id = i.achievement_id AND a.level = i.level
WHERE a.points > 0
ON CONFLICT (user_id, source_type, source_id) DO NOTHING;
