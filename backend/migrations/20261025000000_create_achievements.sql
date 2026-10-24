-- F-18.1 / S-07.3: static achievement catalog, durable completion events,
-- one-time unlock records, and the achievement points ledger entries.
CREATE TABLE achievements (
    id text NOT NULL,
    level integer NOT NULL CHECK (level > 0),
    condition_type text NOT NULL CHECK (condition_type IN (
        'completed_conversations', 'consecutive_days', 'dimension_score',
        'first_task_claim', 'first_purchase'
    )),
    threshold integer NOT NULL CHECK (
        (condition_type = 'dimension_score' AND threshold BETWEEN 0 AND 5)
        OR (condition_type <> 'dimension_score' AND threshold > 0)
    ),
    points integer NOT NULL DEFAULT 10 CHECK (points >= 0),
    name_zh_tw text NOT NULL,
    name_en text NOT NULL,
    name_es text NOT NULL,
    description_zh_tw text NOT NULL,
    description_en text NOT NULL,
    description_es text NOT NULL,
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
CREATE INDEX achievement_unlocks_user_idx ON achievement_unlocks (user_id, unlocked_at DESC);

-- This slice records only achievement awards. The unified point ledger is owned by F-19.1.
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
