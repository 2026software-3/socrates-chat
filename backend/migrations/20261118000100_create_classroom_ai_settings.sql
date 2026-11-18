-- F-40.4-DB (#596): 教室的 AI 對話模型、備援模型、供應商確認，以及「過渡中」旗標（S-03.1，#453）。
-- 只往前：新增資料表與一個欄位，不改動既有資料。

-- NTNU 上線前已存在的教室沿用系統 key，直到教師設定第一把 key 與教室模型（F-40.5）。
-- 先以 DEFAULT true 讓此刻已存在的列都成為過渡中，再把預設改回 false，之後新建的教室不是過渡中。
ALTER TABLE classrooms ADD COLUMN ai_transitional boolean NOT NULL DEFAULT true;
ALTER TABLE classrooms ALTER COLUMN ai_transitional SET DEFAULT false;

-- 供應商是文字、由應用層名單驗證，模型由教師自由輸入。
CREATE TABLE classroom_ai_models (
    classroom_id uuid PRIMARY KEY REFERENCES classrooms (id) ON DELETE CASCADE,
    provider     text NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    model        text NOT NULL CHECK (btrim(model) <> '' AND char_length(model) <= 200),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- 備援最多三個、有順序（1 先試）。
CREATE TABLE classroom_ai_fallbacks (
    classroom_id uuid NOT NULL REFERENCES classrooms (id) ON DELETE CASCADE,
    position     smallint NOT NULL CHECK (position BETWEEN 1 AND 3),
    provider     text NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    model        text NOT NULL CHECK (btrim(model) <> '' AND char_length(model) <= 200),
    PRIMARY KEY (classroom_id, position)
);

-- 教師第一次為教室選定某家供應商時的確認紀錄（以教室＋供應商為單位；此解讀待確認）。
CREATE TABLE classroom_ai_provider_confirmations (
    classroom_id uuid NOT NULL REFERENCES classrooms (id) ON DELETE CASCADE,
    provider     text NOT NULL CHECK (provider ~ '^[a-z0-9_-]{1,64}$'),
    confirmed_by uuid NOT NULL REFERENCES users (id),
    confirmed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (classroom_id, provider)
);
