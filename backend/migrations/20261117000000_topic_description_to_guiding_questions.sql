-- F-32.1：引導問題取代題目內容（S-04.8 2026-11 修訂）。
-- 只處理引導問題為空且 description 非空的題目：依換行分段；超過 200 字的段落再依句末標點切開，
-- 仍超過 200 字者截斷；最多取前 5 段。description 欄位保留不動，之後另以 migration 移除。
WITH lines AS (
    SELECT t.id, l.ord AS line_ord, btrim(l.line, E' \t　') AS line
    FROM topics t
    CROSS JOIN LATERAL regexp_split_to_table(replace(t.description, E'\r', ''), E'\n')
        WITH ORDINALITY AS l(line, ord)
    WHERE cardinality(t.guiding_questions) = 0
      AND btrim(t.description, E' \t\r\n　') <> ''
),
pieces AS (
    SELECT id, line_ord, 1::bigint AS piece_ord, line AS piece
    FROM lines
    WHERE char_length(line) <= 200
    UNION ALL
    SELECT l.id, l.line_ord, m.ord, btrim(m.hit[1], E' \t　')
    FROM lines l
    CROSS JOIN LATERAL regexp_matches(l.line, '[^。！？.!?]+[。！？.!?]*', 'g')
        WITH ORDINALITY AS m(hit, ord)
    WHERE char_length(l.line) > 200
),
ranked AS (
    SELECT id,
           left(piece, 200) AS question,
           row_number() OVER (PARTITION BY id ORDER BY line_ord, piece_ord) AS rn
    FROM pieces
    WHERE piece <> ''
)
UPDATE topics t
SET guiding_questions = s.questions
FROM (
    SELECT id, array_agg(question ORDER BY rn) AS questions
    FROM ranked
    WHERE rn <= 5
    GROUP BY id
) s
WHERE t.id = s.id;
