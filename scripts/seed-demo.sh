#!/usr/bin/env bash
# 匯入示範資料（只給本機開發與試用，全部是合成資料）：
#   - 1 位教師 teacher@example.com、3 位學生 student1～3@example.com（固定的示範密碼）
#   - 內建題目「電車難題」下 6 場已完成的對話：含 AI 總結、最終主張、主張分組與 6 維分數
#     （直接寫入資料庫，不呼叫 AI），讓班上論點分布與個人儀表板有東西可看
# 可重複執行：帳號會重設成示範密碼，示範對話會先刪除再重建。
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
STUDENTS=(student1@example.com student2@example.com student3@example.com)

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

echo "→ 建立教師與學生帳號"
api "$ADMIN_JAR" POST /api/admin/teachers '{"email":"teacher@example.com"}' >/dev/null
set_password teacher@example.com "$TEACHER_PASSWORD"
api "$ADMIN_JAR" POST /api/roster/import "{\"text\":\"$(printf '%s\\n' "${STUDENTS[@]}")\"}" >/dev/null
for s in "${STUDENTS[@]}"; do set_password "$s" "$STUDENT_PASSWORD"; done
rm -f "$ADMIN_JAR"

echo "→ 匯入示範對話與總結"
$DEMO_PSQL -v ON_ERROR_STOP=1 -q <<'SQL'
BEGIN;
-- 重跑時先清掉上次的示範對話（description 為 [demo]；總結隨之刪除）
DELETE FROM conversations WHERE description = '[demo]';
INSERT INTO claim_groups (topic_title, name)
VALUES ('電車難題', '拉桿'), ('電車難題', '不拉桿'), ('電車難題', '視情況而定')
ON CONFLICT DO NOTHING;

DO $$
DECLARE
  r record;
  cid uuid;
  n int := 0;
BEGIN
  FOR r IN
    SELECT * FROM (VALUES
      -- 學生, 最終主張, 主張群組, 核心理由, 效益主義, 義務論, 德行倫理, 契約論, 存在主義（關懷倫理示範都給 0）
      ('student1@example.com', '該拉桿，救五人優先', '拉桿', '結果上救五個人比救一個人重要', 5, 1, 1, 0, 0),
      ('student1@example.com', '應該拉桿', '拉桿', '造成的傷害越少越好', 4, 2, 0, 1, 0),
      ('student2@example.com', '不拉桿，不能主動殺人', '不拉桿', '不能把人當成達成目的的工具', 1, 5, 2, 0, 0),
      ('student2@example.com', '視情況而定', '視情況而定', '要看被犧牲的人是誰、與我有什麼關係', 1, 1, 2, 4, 1),
      ('student3@example.com', '拉桿，但要有人負責', '拉桿', '救人是對的，但要為行動承擔責任', 3, 2, 3, 1, 3),
      ('student3@example.com', '不拉桿，這不該由我決定', '不拉桿', '個人沒有資格決定誰該活下來', 0, 4, 1, 3, 4)
    ) AS t(email, claim, grp, reasons, u, d, v, c, e)
  LOOP
    n := n + 1;
    INSERT INTO conversations (user_id, topic_id, title, description, status, stage, turn_count, ended_at)
    SELECT id, '00000000-0000-4000-8000-000000000001', '電車難題', '[demo]', 'ended', 3, 8,
           now() - n * interval '1 hour'
    FROM users WHERE email = r.email
    RETURNING id INTO cid;
    INSERT INTO summaries (conversation_id, status, stance, reasons, claim, claim_group_id, framework,
                           framework_scores, rules_version, completed_at)
    SELECT cid, 'ready', '傾向：' || r.claim, r.reasons, r.claim, g.id,
           CASE GREATEST(r.u, r.d, r.v, r.c, r.e)
             WHEN r.u THEN 'utilitarianism' WHEN r.d THEN 'deontology' WHEN r.v THEN 'virtue'
             WHEN r.c THEN 'contractarianism' ELSE 'existentialism' END,
           jsonb_build_object('utilitarianism', r.u, 'deontology', r.d, 'virtue', r.v,
                              'contractarianism', r.c, 'care', 0, 'existentialism', r.e),
           'demo', now()
    FROM claim_groups g WHERE g.topic_title = '電車難題' AND g.name = r.grp;
  END LOOP;
END $$;
COMMIT;
SQL

cat <<DONE

完成。示範帳號（只給本機使用）：
  教師  teacher@example.com        密碼 $TEACHER_PASSWORD
  學生  student1~3@example.com     密碼 $STUDENT_PASSWORD
用教師登入看「班上分布」，用學生登入看「我的儀表板」。
DONE
