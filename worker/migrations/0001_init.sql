-- Workers 試驗版的 D1（SQLite）schema：等同 backend/migrations 14 筆 PostgreSQL migration 套用後的最終狀態。
-- 只給全新資料庫用（不搬移既有資料）；歷史資料的補算（achievement 回填）因此省略。
--
-- 型別對應：
--   uuid        → TEXT（小寫、含連字號；預設值以 randomblob 產生 v4 格式）
--   timestamptz → TEXT，UTC 的 'YYYY-MM-DDTHH:MM:SS.mmmZ'，字串比較即時間比較
--   boolean     → INTEGER 0／1
--   bytea       → TEXT 十六進位
--   text[]、jsonb → TEXT（JSON）
--   正規表示式 CHECK → GLOB
-- D1 預設啟用外鍵（PRAGMA foreign_keys = ON），ON DELETE CASCADE 照常運作。

CREATE TABLE users (
    id           TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    google_sub   TEXT UNIQUE CHECK (google_sub <> ''),
    email        TEXT UNIQUE CHECK (email = lower(email) AND email <> ''),
    student_id   TEXT UNIQUE CHECK (student_id GLOB '[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][A-Z]'),
    imported_name TEXT CHECK (imported_name <> '' AND length(imported_name) <= 30),
    -- PBKDF2-SHA256 的 PHC 字串（Workers 版改用 WebCrypto，見 worker/src/password.rs）
    password_hash TEXT CHECK (password_hash <> ''),
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    display_name TEXT,
    language     TEXT NOT NULL DEFAULT 'zh-TW' CHECK (language IN ('zh-TW', 'en', 'es')),
    language_initialized INTEGER NOT NULL DEFAULT 0 CHECK (language_initialized IN (0, 1)),
    is_admin     INTEGER NOT NULL DEFAULT 0 CHECK (is_admin IN (0, 1)),
    disabled_at  TEXT,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE (id, student_id),
    CHECK (email IS NOT NULL OR student_id IS NOT NULL),
    CHECK (student_id IS NULL OR (imported_name IS NOT NULL AND NOT is_admin))
);

CREATE TABLE sessions (
    id           TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE CHECK (length(token_hash) = 64),
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1))
);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);

CREATE TABLE oauth_login_states (
    state         TEXT PRIMARY KEY,
    nonce         TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    link_user_id  TEXT REFERENCES users (id) ON DELETE CASCADE,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE login_failures (
    login_key      TEXT NOT NULL CHECK (length(login_key) = 64 AND login_key NOT GLOB '*[^0-9a-f]*'),
    ip             TEXT NOT NULL DEFAULT '',
    failures       INTEGER NOT NULL DEFAULT 0,
    locked_until   TEXT,
    last_failed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (login_key, ip)
);

CREATE TABLE classrooms (
    id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    name        TEXT NOT NULL CHECK (
        trim(name, ' ' || char(9) || char(10) || char(11) || char(12) || char(13)) <> ''
        AND length(name) <= 255
    ),
    year_label  TEXT CHECK (length(year_label) <= 100),
    description TEXT CHECK (length(description) <= 2000),
    is_archived INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0, 1)),
    created_by  TEXT NOT NULL REFERENCES users (id),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
-- SQLite 的 lower() 只轉 ASCII；中文名稱不受影響
CREATE UNIQUE INDEX classrooms_normalized_name_unique
    ON classrooms (lower(trim(name, ' ' || char(9) || char(10) || char(11) || char(12) || char(13))));
CREATE INDEX classrooms_created_by_idx ON classrooms (created_by);

CREATE TABLE enrollments (
    classroom_id TEXT NOT NULL REFERENCES classrooms (id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL,
    student_id   TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (classroom_id, user_id),
    FOREIGN KEY (user_id, student_id) REFERENCES users (id, student_id)
        ON UPDATE CASCADE ON DELETE CASCADE
);
CREATE INDEX enrollments_user_id_idx ON enrollments (user_id);

CREATE TABLE teachers (
    email      TEXT PRIMARY KEY CHECK (email = lower(email) AND email <> ''),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE topics (
    id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    category    TEXT,
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE activities (
    id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    classroom_id TEXT NOT NULL REFERENCES classrooms (id) ON DELETE CASCADE,
    is_archived INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0, 1)),
    status      TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'closed')),
    opens_at    TEXT,
    closes_at   TEXT,
    created_by  TEXT REFERENCES users (id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    CONSTRAINT activities_schedule_check CHECK (closes_at > opens_at)
);
CREATE INDEX activities_status_idx ON activities (status);
CREATE INDEX activities_classroom_idx ON activities (classroom_id);
CREATE INDEX activities_created_idx ON activities (created_at DESC, id);

CREATE TABLE activity_topics (
    activity_id TEXT NOT NULL REFERENCES activities (id) ON DELETE CASCADE,
    topic_id    TEXT NOT NULL REFERENCES topics (id),
    position    INTEGER NOT NULL CHECK (position >= 0 AND position < 5),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (activity_id, topic_id),
    CONSTRAINT activity_topics_position_unique UNIQUE (activity_id, position)
);
CREATE INDEX activity_topics_topic_idx ON activity_topics (topic_id);

CREATE TABLE conversations (
    id           TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    activity_id  TEXT REFERENCES activities (id) ON DELETE CASCADE,
    topic_id     TEXT REFERENCES topics (id) ON DELETE SET NULL,
    classroom_id TEXT REFERENCES classrooms (id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    language     TEXT NOT NULL DEFAULT 'zh-TW' CHECK (language IN ('zh-TW', 'en', 'es')),
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'ended')),
    stage        INTEGER NOT NULL DEFAULT 1 CHECK (stage BETWEEN 1 AND 3),
    turn_count   INTEGER NOT NULL DEFAULT 0,
    converge_ready INTEGER NOT NULL DEFAULT 0 CHECK (converge_ready IN (0, 1)),
    generating_since TEXT,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    ended_at     TEXT,
    archived_at  TEXT
);
CREATE INDEX conversations_user_idx ON conversations (user_id, created_at DESC, id);
CREATE INDEX conversations_ended_idx ON conversations (ended_at DESC, id) WHERE ended_at IS NOT NULL;
CREATE INDEX conversations_classroom_idx ON conversations (classroom_id) WHERE classroom_id IS NOT NULL;
CREATE INDEX conversations_activity_idx ON conversations (activity_id) WHERE activity_id IS NOT NULL;
CREATE UNIQUE INDEX conversations_user_activity_uniq
    ON conversations (user_id, activity_id, topic_id) WHERE activity_id IS NOT NULL;

-- PostgreSQL 版的 seq（identity 欄位）只用來排序；SQLite 用隱含的 rowid（插入順序），程式以 ORDER BY rowid 排序。
CREATE TABLE messages (
    id              TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('student', 'ai')),
    content         TEXT NOT NULL,
    source          TEXT NOT NULL DEFAULT 'text' CHECK (source IN ('text', 'speech-browser', 'speech-openai', 'speech-cascade')),
    rules_version   TEXT,
    question_type   TEXT,
    advance_reason  TEXT,
    heard_chars     INTEGER,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX messages_conversation_idx ON messages (conversation_id);

CREATE TABLE claim_groups (
    id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    activity_id TEXT NOT NULL REFERENCES activities (id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (trim(name) <> ''),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE (activity_id, name)
);

CREATE TABLE summaries (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations (id) ON DELETE CASCADE,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'failed')),
    attempt         INTEGER NOT NULL DEFAULT 1,
    stance          TEXT,
    reasons         TEXT,
    claim           TEXT,
    claim_group_id  TEXT REFERENCES claim_groups (id) ON DELETE SET NULL,
    rules_version   TEXT,
    started_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    completed_at    TEXT,
    CHECK (status <> 'ready' OR (stance IS NOT NULL AND reasons IS NOT NULL))
);
CREATE INDEX summaries_ready_idx ON summaries (completed_at DESC) WHERE status = 'ready';

CREATE TABLE audit_logs (
    id           TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    actor_id     TEXT REFERENCES users (id) ON DELETE SET NULL,
    action       TEXT NOT NULL CHECK (action IN (
        'login', 'account_disabled', 'account_enabled', 'student_deleted',
        'teacher_added', 'teacher_removed', 'roster_added', 'roster_removed', 'password_reset',
        'classroom_archived', 'classroom_restored', 'classroom_deleted',
        'activity_archived', 'activity_restored', 'activity_deleted',
        'achievement_created', 'achievement_updated', 'task_updated', 'part_updated'
    )),
    target_email TEXT NOT NULL,
    reason       TEXT,
    item_count   INTEGER CHECK (item_count >= 0),
    old_value    TEXT CHECK (old_value IS NULL OR json_valid(old_value)),
    new_value    TEXT CHECK (new_value IS NULL OR json_valid(new_value)),
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX audit_logs_created_idx ON audit_logs (created_at DESC);

CREATE TABLE point_ledger (
    id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))),
    user_id     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    delta       INTEGER NOT NULL,
    reason      TEXT NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN
                    ('conversation', 'achievement', 'task', 'purchase', 'admin_adjustment')),
    source_id   TEXT NOT NULL CHECK (source_id <> ''),
    unit_price  INTEGER CHECK (unit_price >= 0),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    CHECK (
        (source_type = 'purchase' AND unit_price IS NOT NULL AND delta = -unit_price)
        OR (source_type <> 'purchase' AND unit_price IS NULL AND delta <> 0)
    ),
    UNIQUE (user_id, source_type, source_id)
);

CREATE TABLE achievements (
    id TEXT NOT NULL CHECK (
        length(id) BETWEEN 1 AND 64 AND id GLOB '[a-z]*' AND id NOT GLOB '*[^a-z0-9_]*'
    ),
    level INTEGER NOT NULL CHECK (level > 0),
    condition_type TEXT NOT NULL CHECK (condition_type IN (
        'completed_conversations', 'consecutive_days', 'dimension_score',
        'first_task_claim', 'first_purchase'
    )),
    threshold INTEGER NOT NULL CHECK (
        CASE condition_type
            WHEN 'dimension_score' THEN threshold BETWEEN 0 AND 5
            WHEN 'first_task_claim' THEN threshold = 1
            WHEN 'first_purchase' THEN threshold = 1
            ELSE threshold > 0
        END
    ),
    points INTEGER NOT NULL DEFAULT 10 CHECK (points >= 0),
    name_zh_tw TEXT NOT NULL CHECK (trim(name_zh_tw) <> ''),
    name_en TEXT NOT NULL CHECK (trim(name_en) <> ''),
    name_es TEXT NOT NULL CHECK (trim(name_es) <> ''),
    description_zh_tw TEXT NOT NULL CHECK (trim(description_zh_tw) <> ''),
    description_en TEXT NOT NULL CHECK (trim(description_en) <> ''),
    description_es TEXT NOT NULL CHECK (trim(description_es) <> ''),
    source_language TEXT CHECK (source_language IN ('zh-TW', 'en', 'es')),
    -- PostgreSQL 版是 text[]；這裡是 JSON 陣列，元素只能是三種語言
    pending_languages TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(pending_languages) AND json_type(pending_languages) = 'array'
    ),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (id, level)
);

-- 同一成就的門檻必須隨等級嚴格遞增（S-07.3 2.3）。PostgreSQL 版是 commit 時才檢查的 constraint trigger；
-- SQLite 沒有延後檢查，逐列立即檢查會擋掉一次調整多個等級的合法更新，所以改由 API 在寫入前檢查
-- （achievement_admin::update），schema 不設觸發器。

CREATE TABLE achievement_completion_events (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL,
    completed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (user_id, conversation_id)
);
CREATE INDEX achievement_completion_events_user_time_idx
    ON achievement_completion_events (user_id, completed_at);

CREATE TABLE achievement_unlocks (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    achievement_id TEXT NOT NULL,
    level INTEGER NOT NULL CHECK (level > 0),
    unlocked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (user_id, achievement_id, level),
    FOREIGN KEY (achievement_id, level) REFERENCES achievements(id, level)
);

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

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    period TEXT NOT NULL UNIQUE CHECK (period IN ('daily', 'weekly')),
    goal INTEGER NOT NULL CHECK (goal > 0),
    reward INTEGER NOT NULL CHECK (reward > 0)
);
INSERT INTO tasks (id, period, goal, reward) VALUES
    ('daily_dialogue', 'daily', 1, 5),
    ('weekly_dialogue', 'weekly', 5, 20);

CREATE TABLE store_parts (
    id TEXT PRIMARY KEY CHECK (id <> ''),
    name_zh_tw TEXT NOT NULL CHECK (trim(name_zh_tw) <> ''),
    name_en TEXT NOT NULL CHECK (trim(name_en) <> ''),
    name_es TEXT NOT NULL CHECK (trim(name_es) <> ''),
    price INTEGER NOT NULL CHECK (price >= 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);
