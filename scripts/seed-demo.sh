#!/usr/bin/env bash
# 匯入示範資料（只給本機開發與試用，全部是合成資料）：
#   - 1 位教師 teacher@example.com、8 位學生 student1～8@example.com（固定的示範密碼）
#   - 2 間教室（週一班 6 人、週三班 4 人，student3、student4 兩班都在），另有 1 間已封存的舊教室
#   - 4 個題目（內建「電車難題」加 3 題）、7 個活動（含草稿、已發布、已停止開放）
#   - 約 25 場對話：已完成（含逐則訊息、AI 總結、最終主張與主張分組）、進行中、總結產生中／失敗
#     全部直接寫入資料庫，不呼叫 AI；讓班上論點分布、個人儀表板與對話頁都有東西可看
# 可重複執行：帳號會重設成示範密碼，示範教室、活動與對話會先刪除再重建。
#
# 前置：服務已啟動（後端啟動時會補上內建題目），且管理者已登入過一次並更改了初始密碼。
# 用法：
#   ADMIN_PASSWORD='<管理者目前的密碼>' ./scripts/seed-demo.sh
# 可用環境變數：
#   BASE_URL         服務網址，預設取 .env 的 APP_BASE_URL，沒有就是 http://localhost:3000
#   ADMIN_EMAIL      管理者信箱，預設取 .env 的 ADMIN_EMAILS 第一個
#   ADMIN_PASSWORD   管理者目前的密碼（必填）
#   DEMO_PSQL        執行 SQL 的指令，預設 docker compose exec -T db psql -U socrates -d socrates
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -f .env ]; then
  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
fi

BASE_URL="${BASE_URL:-${APP_BASE_URL:-http://localhost:3000}}"
ADMIN_EMAIL="${ADMIN_EMAIL:-${ADMIN_EMAILS%%,*}}"
: "${ADMIN_EMAIL:?請設定 ADMIN_EMAIL（或在 .env 設定 ADMIN_EMAILS）}"
: "${ADMIN_PASSWORD:?請設定 ADMIN_PASSWORD（管理者目前的密碼）}"
DEMO_PSQL="${DEMO_PSQL:-docker compose exec -T db psql -U socrates -d socrates}"

TEACHER_PASSWORD='Teacher-Demo-2026'
STUDENT_PASSWORD='Student-Demo-2026'
STUDENTS=(student1@example.com student2@example.com student3@example.com student4@example.com
          student5@example.com student6@example.com student7@example.com student8@example.com)

json_get() { python3 -c "import sys,json; print(json.load(sys.stdin)$1)"; }
api() { # api <cookie-jar> <METHOD> <path> [json-body]
  local jar="$1" method="$2" path="$3" body="${4:-}"
  curl -sS -f -b "$jar" -c "$jar" -X "$method" "$BASE_URL$path" \
    -H "Origin: $BASE_URL" -H 'Content-Type: application/json' ${body:+-d "$body"}
}

echo "→ 管理者登入 $ADMIN_EMAIL"
ADMIN_JAR=$(mktemp)
login=$(api "$ADMIN_JAR" POST /api/auth/login "{\"email\":\"$ADMIN_EMAIL\",\"password\":\"$ADMIN_PASSWORD\"}") \
  || { echo "登入失敗：請確認服務已啟動、帳號密碼正確。" >&2; exit 1; }
if [ "$(echo "$login" | json_get "['must_change_password']")" = "True" ]; then
  echo "這是臨時密碼：請先用瀏覽器登入並更改密碼，再用新密碼重新執行。" >&2
  exit 1
fi

# 重設成示範密碼：臨時密碼登入後立刻改成固定的示範密碼
set_password() { # set_password <email> <final-password>
  local temp jar
  temp=$(api "$ADMIN_JAR" POST /api/admin/users/reset-password "{\"email\":\"$1\"}" | json_get "['temporary_password']")
  jar=$(mktemp)
  api "$jar" POST /api/auth/login "{\"email\":\"$1\",\"password\":\"$temp\"}" >/dev/null
  api "$jar" POST /api/auth/change-password "{\"current_password\":\"$temp\",\"new_password\":\"$2\"}" >/dev/null
  rm -f "$jar"
}

echo "→ 建立教師帳號"
api "$ADMIN_JAR" POST /api/admin/teachers '{"email":"teacher@example.com"}' >/dev/null
set_password teacher@example.com "$TEACHER_PASSWORD"

echo "→ 建立教室、修課名單與題目"
# 先清掉上次的示範資料：教室以 [demo] 標記在說明；活動、對話、總結、主張分組隨之刪除
$DEMO_PSQL -v ON_ERROR_STOP=1 -q <<'SQL'
BEGIN;
DELETE FROM conversations WHERE classroom_id IN (SELECT id FROM classrooms WHERE description LIKE '[demo]%');
DELETE FROM activities WHERE classroom_id IN (SELECT id FROM classrooms WHERE description LIKE '[demo]%');
DELETE FROM enrollments WHERE classroom_id IN (SELECT id FROM classrooms WHERE description LIKE '[demo]%');
DELETE FROM classrooms WHERE description LIKE '[demo]%';
DELETE FROM topics WHERE id::text LIKE '00000000-0000-4000-8000-0000000001%';

INSERT INTO classrooms (name, year_label, description, created_by)
SELECT n, y, d, (SELECT id FROM users WHERE email = 'teacher@example.com')
FROM (VALUES
  ('哲學導論 週一班', '115 學年度', '[demo] 週一上午班，6 位學生'),
  ('哲學導論 週三班', '115 學年度', '[demo] 週三下午班，4 位學生'),
  ('哲學導論 舊班（已封存）', '114 學年度', '[demo] 上學年的班級，保留供查閱')
) AS t(n, y, d);

INSERT INTO enrollments (classroom_id, email)
SELECT c.id, e FROM classrooms c, unnest(CASE c.name
  WHEN '哲學導論 週一班' THEN ARRAY['student1','student2','student3','student4','student5','student6']
  WHEN '哲學導論 週三班' THEN ARRAY['student3','student4','student7','student8']
  ELSE ARRAY['student1','student2'] END) AS e0(e0), LATERAL (SELECT e0.e0 || '@example.com' AS e) x(e)
WHERE c.description LIKE '[demo]%';

INSERT INTO topics (id, title, description, category) VALUES
  ('00000000-0000-4000-8000-000000000101', '說謊是否永遠不對？',
   '康德主張說謊永遠是錯的，即使是對想傷害朋友的人說謊。你同意嗎？請說明你的理由，並想想有沒有例外。', '倫理學'),
  ('00000000-0000-4000-8000-000000000102', '我們能確定自己不是在做夢嗎？',
   '笛卡兒懷疑我們的感官經驗可能全是夢境。你有什麼理由相信眼前的世界是真實的？', '知識論'),
  ('00000000-0000-4000-8000-000000000103', '電腦程式能擁有心靈嗎？',
   '如果一個 AI 能完美通過對話測試，我們是否該說它「理解」語言、甚至有意識？', '心靈哲學');
COMMIT;
SQL

echo "→ 建立學生帳號"
for s in "${STUDENTS[@]}"; do set_password "$s" "$STUDENT_PASSWORD"; done
rm -f "$ADMIN_JAR"

echo "→ 匯入示範活動、對話與總結"
$DEMO_PSQL -v ON_ERROR_STOP=1 -q <<'SQL'
BEGIN;
-- 活動（description 以 [demo] 開頭）。每位學生在每個活動至多一場對話（S-04.7）
INSERT INTO activities (title, description, topic_id, classroom_id, status, created_by)
SELECT t.title, '[demo] ' || t.descr, t.topic::uuid, c.id, t.status, c.created_by
FROM (VALUES
  ('哲學導論 週一班', '電車難題：第一週', '先想想你的直覺，再說明理由', '00000000-0000-4000-8000-000000000001', 'published'),
  ('哲學導論 週一班', '說謊的界線',       '找出一個你認為可以說謊的情境',     '00000000-0000-4000-8000-000000000101', 'published'),
  ('哲學導論 週一班', '夢境與真實',       '討論期中前請先讀完笛卡兒第一沉思', '00000000-0000-4000-8000-000000000102', 'closed'),
  ('哲學導論 週一班', 'AI 與心靈（草稿）', '尚未發布，預計下週開放',           '00000000-0000-4000-8000-000000000103', 'draft'),
  ('哲學導論 週三班', '電車難題：第一週', '先想想你的直覺，再說明理由',       '00000000-0000-4000-8000-000000000001', 'published'),
  ('哲學導論 週三班', 'AI 與心靈',       'AI 能擁有心靈嗎？',                 '00000000-0000-4000-8000-000000000103', 'published'),
  ('哲學導論 舊班（已封存）', '電車難題：期末討論', '上學年的活動', '00000000-0000-4000-8000-000000000001', 'closed')
) AS t(classroom, title, descr, topic, status)
JOIN classrooms c ON c.name = t.classroom AND c.description LIKE '[demo]%';

CREATE TEMP TABLE demo_conv (
  student text, classroom text, act text, state text, claim text, grp text, reasons text,
  opening text, followup text, reply text
) ON COMMIT DROP;
INSERT INTO demo_conv VALUES
 -- 週一班 電車難題（已完成，三種立場）
 ('student1','哲學導論 週一班','電車難題：第一週','ended','該拉桿，救五人優先','拉桿','結果上救五個人比救一個人重要','我會拉桿，五條命比一條命多。','你說「比較多」，那如果那一個人是你的家人呢？你的標準還成立嗎？','還是成立，但我會很難受。標準不該因為關係而改變，只是我得承認自己可能做不到。'),
 ('student2','哲學導論 週一班','電車難題：第一週','ended','不拉桿，不能主動殺人','不拉桿','不能把人當成達成目的的工具','我不會拉，因為拉了我就是殺人的人。','不拉桿讓五個人死，你認為這和拉桿殺一個人有什麼不同？','差別在於我有沒有主動介入。我不是不在乎五個人，但我不能拿一個人當工具。'),
 ('student3','哲學導論 週一班','電車難題：第一週','ended','拉桿，但要有人負責','拉桿','救人是對的，但要為行動承擔責任','我會拉，但我覺得之後要有人來檢討這個決定。','為什麼你覺得拉桿的人需要被檢討？這是否代表你認為這個行為本身有問題？','不是有問題，而是它造成了傷害。即使選擇是對的，也不代表可以不負責。'),
 ('student4','哲學導論 週一班','電車難題：第一週','ended','視情況而定','視情況而定','要看被犧牲的人是誰、與我有什麼關係','我覺得不能一概而論，要看情境。','你說要看情境，那是哪些條件會讓你改變答案？','例如那個人是不是自願、或是我有沒有別的辦法。條件變了，結論也會變。'),
 ('student5','哲學導論 週一班','電車難題：第一週','ended','拉桿','拉桿','造成的傷害越少越好','我拉桿，因為傷害越少越好。','如果要推一個胖子下去才能擋住電車，你的理由還適用嗎？','那我就不確定了。結果一樣，但推人感覺不同，我需要想想為什麼。'),
 ('student6','哲學導論 週一班','電車難題：第一週','active',NULL,NULL,NULL,'我覺得這題很難，兩邊都有道理。','你說兩邊都有道理，能先說說你覺得拉桿那邊最有力的理由是什麼嗎？',NULL),
 -- 週一班 說謊的界線
 ('student1','哲學導論 週一班','說謊的界線','ended','原則上不說謊，但可例外','視情況而定','誠實是基本原則，但保護無辜時可以讓步','我覺得大部分時候不該說謊，但保護朋友時可以。','你把「保護朋友」當成例外，那誰來決定什麼算正當的例外？','最後還是要自己判斷，所以我需要能說出理由，而不是只憑感覺。'),
 ('student2','哲學導論 週一班','說謊的界線','ended','說謊永遠不對','不可說謊','說謊破壞信任，若人人都說謊，語言就失去意義','我同意康德，說謊就是不對。','如果凶手問你朋友藏在哪裡，你也不說謊？','我可以保持沉默或拒絕回答，這樣既不說謊也不洩漏。'),
 ('student3','哲學導論 週一班','說謊的界線','ended','後果重要，可以說謊','可以說謊','善意的謊言能減少傷害','善意的謊言有時很必要，例如安慰病人。','那病人之後發現你騙了他，你認為他會怎麼看待你之前說的話？','可能會失去信任，所以謊言也有代價，要衡量。'),
 ('student4','哲學導論 週一班','說謊的界線','summary_pending',NULL,NULL,NULL,'我覺得要看說謊的目的。','你說的「目的」，是指說謊者的動機，還是結果？','我想主要是動機，但結果也會影響我怎麼評價。'),
 ('student5','哲學導論 週一班','說謊的界線','active',NULL,NULL,NULL,'我還沒想好。',NULL,NULL),
 -- 週一班 夢境與真實（活動已停止開放）
 ('student1','哲學導論 週一班','夢境與真實','ended','無法絕對確定，但有實用的理由相信','懷疑但務實','日常經驗前後一致，足以當作行動的依據','我沒辦法證明自己不在做夢，但生活是連貫的。','「連貫」本身能保證真實嗎？夢有時也可以很連貫。','不能保證，所以我只說「有好的理由相信」，不說「確定」。'),
 ('student2','哲學導論 週一班','夢境與真實','ended','相信外在世界存在','相信真實','懷疑到底會讓人無法行動，所以預設世界存在較合理','我相信世界是真的，不然懷疑沒有止境。','你的理由是實用上的，這算不算承認無法證明？','算，但我覺得「無法證明」和「不合理相信」是兩回事。'),
 ('student3','哲學導論 週一班','夢境與真實','summary_failed',NULL,NULL,NULL,'我認為我們永遠沒辦法確定。','如果永遠不能確定，那「知道」這個詞還有意義嗎？','有，只是標準不是百分之百確定。'),
 -- 週三班 電車難題（與週一班分開統計）
 ('student3','哲學導論 週三班','電車難題：第一週','ended','拉桿，但要有人負責','拉桿','救人是對的，但要為行動承擔責任','在這個班我也會拉桿，原因跟上次一樣。','你在兩個班的答案相同，這次有沒有想到新的反例？','有，如果五個人是自己闖入軌道的，我會比較猶豫。'),
 ('student4','哲學導論 週三班','電車難題：第一週','ended','不拉桿，這不該由我決定','不拉桿','個人沒有資格決定誰該活下來','我不拉，因為我沒有資格決定誰活。','不作為難道就不是一種決定嗎？','是，但我認為主動與被動在道德上有差別。'),
 ('student7','哲學導論 週三班','電車難題：第一週','ended','該拉桿','拉桿','五條命比一條重要','拉桿，數字很清楚。','如果犧牲的是一位正要治癒疾病的醫生呢？','那我會需要重新衡量，單純數字不夠。'),
 ('student8','哲學導論 週三班','電車難題：第一週','ended','視情況而定','視情況而定','需要更多資訊才能判斷','我需要知道更多：那些人是誰？','你想要的資訊裡，哪一項最可能改變你的答案？','那一個人是不是自願在軌道上。'),
 -- 週三班 AI 與心靈
 ('student3','哲學導論 週三班','AI 與心靈','ended','AI 目前沒有心靈','沒有心靈','通過對話測試只證明行為像，不證明有內在經驗','能對話不代表真的理解，只是模式很像。','那你怎麼確定其他人類不是也「只是模式很像」？','我確實無法直接確認，但同為人類，我有理由類推。'),
 ('student4','哲學導論 週三班','AI 與心靈','ended','可能有，要看功能','功能論','心靈是功能組織，不一定取決於材質','如果功能一樣，我看不出為什麼材質有差別。','這是否代表一台複雜的恆溫器也有一點心靈？','不一定，功能要夠複雜；但界線在哪確實是難題。'),
 ('student7','哲學導論 週三班','AI 與心靈','active',NULL,NULL,NULL,'我覺得 AI 只是工具。','你說它只是工具，那工具和有心靈的東西差在哪裡？',NULL),
 ('student8','哲學導論 週三班','AI 與心靈','active',NULL,NULL,NULL,'我不知道，我沒想過這件事。',NULL,NULL),
 -- 舊班
 ('student1','哲學導論 舊班（已封存）','電車難題：期末討論','ended','拉桿','拉桿','傷害越少越好','期末我仍然傾向拉桿。','經過一學期，你的理由有改變嗎？','更細緻了，我現在會先問誰有權做這個決定。'),
 ('student2','哲學導論 舊班（已封存）','電車難題：期末討論','ended','不拉桿','不拉桿','人不能被當成工具','我維持不拉桿。','一學期後你有遇到最有力的反對意見嗎？','有，就是不作為也是選擇，我至今還沒完全回應它。');

DO $$
DECLARE
  r record; cid uuid; gid uuid; n int := 0;
  stage_v int; turns int; st text;
BEGIN
  FOR r IN SELECT * FROM demo_conv LOOP
    n := n + 1;
    st := CASE WHEN r.state = 'active' THEN 'active' ELSE 'ended' END;
    turns := CASE WHEN r.state = 'active' THEN (CASE WHEN r.followup IS NULL THEN 1 ELSE 2 END) ELSE 6 + n % 3 END;
    stage_v := CASE WHEN r.state = 'active' THEN (CASE WHEN r.followup IS NULL THEN 1 ELSE 2 END) ELSE 3 END;
    INSERT INTO conversations (user_id, activity_id, topic_id, classroom_id, title, description,
                               status, stage, turn_count, converge_ready, created_at, ended_at)
    SELECT u.id, a.id, a.topic_id, a.classroom_id, a.title, a.description, st, stage_v, turns,
           r.state <> 'active', now() - (n * interval '3 hours') - interval '30 minutes',
           CASE WHEN st = 'ended' THEN now() - n * interval '3 hours' END
    FROM users u
    JOIN activities a ON a.title = r.act AND a.description LIKE '[demo]%'
    JOIN classrooms c ON c.id = a.classroom_id AND c.name = r.classroom
    WHERE u.email = r.student || '@example.com'
    RETURNING id INTO cid;

    INSERT INTO messages (conversation_id, role, content, rules_version, question_type) VALUES
      (cid, 'student', r.opening, NULL, NULL);
    IF r.followup IS NOT NULL THEN
      INSERT INTO messages (conversation_id, role, content, rules_version, question_type)
        VALUES (cid, 'ai', r.followup, 'v1/zh-TW', 'clarify');
    END IF;
    IF r.reply IS NOT NULL THEN
      INSERT INTO messages (conversation_id, role, content, rules_version, question_type) VALUES
        (cid, 'student', r.reply, NULL, NULL),
        (cid, 'ai', '謝謝你的想法。' || COALESCE('聽起來你的立場有一個核心理由：' || r.reasons || '。', '') || '最後一個問題：有什麼情境會讓你放棄這個立場？', 'v1/zh-TW', 'challenge');
    END IF;

    IF r.state = 'ended' THEN
      gid := NULL;
      INSERT INTO claim_groups (activity_id, name)
      SELECT a.id, r.grp FROM activities a JOIN classrooms c ON c.id = a.classroom_id
      WHERE a.title = r.act AND c.name = r.classroom AND a.description LIKE '[demo]%'
      ON CONFLICT DO NOTHING;
      SELECT g.id INTO gid FROM claim_groups g JOIN activities a ON a.id = g.activity_id
      JOIN classrooms c ON c.id = a.classroom_id
      WHERE a.title = r.act AND c.name = r.classroom AND a.description LIKE '[demo]%' AND g.name = r.grp;
      INSERT INTO summaries (conversation_id, status, stance, reasons, claim, claim_group_id, rules_version, completed_at)
      VALUES (cid, 'ready', '傾向：' || r.claim, r.reasons, r.claim, gid, 'demo', now());
    ELSIF r.state = 'summary_pending' THEN
      UPDATE conversations SET ended_at = now() - interval '1 minute' WHERE id = cid;
      INSERT INTO summaries (conversation_id, status) VALUES (cid, 'pending');
    ELSIF r.state = 'summary_failed' THEN
      INSERT INTO summaries (conversation_id, status) VALUES (cid, 'failed');
    END IF;
  END LOOP;
END $$;
COMMIT;
SQL

cat <<DONE

完成。示範帳號（只給本機使用）：
  教師  teacher@example.com           密碼 $TEACHER_PASSWORD
  學生  student1~8@example.com        密碼 $STUDENT_PASSWORD
資料內容：
  教室  週一班（student1~6）、週三班（student3、4、7、8）、舊班（已封存）
  題目  電車難題＋說謊、夢境、AI 心靈 共 4 題
  活動  7 個（草稿、已發布、已停止開放各有）
  對話  已完成、進行中、總結產生中、總結失敗皆有
用教師登入看各教室「班上分布」；用 student3 登入可看到跨兩個教室的個人儀表板。
DONE
