-- F-19.1：點數帳本與防重複入帳（S-07.2）
-- 只新增、不修改、不刪除；更正以新增一筆反向紀錄處理。帳號刪除時由外鍵連動刪除（S-07.2 2.5）。
-- 餘額＝該使用者 delta 的加總。只存 ID 與數值，不含對話內容或任何敏感欄位（S-02.2）。

CREATE TABLE point_ledger (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 正為入帳、負為扣除
    delta       integer NOT NULL,
    reason      text NOT NULL,
    source_type text NOT NULL CHECK (source_type IN
                    ('conversation', 'achievement', 'task', 'purchase', 'admin_adjustment')),
    -- 對話 ID、成就 ID、`<task_id>:<period_start>`、部件 ID 或該次調整的唯一 ID。
    -- 不做成外鍵：來源（例如對話）被刪除後紀錄仍保留
    source_id   text NOT NULL CHECK (source_id <> ''),
    -- 扣除時記錄當下的實際價格；0 表示免費
    unit_price  integer CHECK (unit_price >= 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    -- 欄位之間的一致性（S-07.2 2.1）：購買一定記錄價格且扣的點數等於價格（免費部件皆為 0）；
    -- 其他來源不記錄價格且一定有變動。管理者調整可正可負（2.4）
    CHECK (
        (source_type = 'purchase' AND unit_price IS NOT NULL AND delta = -unit_price)
        OR (source_type <> 'purchase' AND unit_price IS NULL AND delta <> 0)
    ),
    -- 防重複由資料庫保證：同一事件只入帳一次。也作為依使用者查餘額與帳本的索引
    UNIQUE (user_id, source_type, source_id)
);
