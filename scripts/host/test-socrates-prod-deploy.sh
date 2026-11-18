#!/usr/bin/env bash
# socrates-prod-deploy 的回歸測試：用假的 git／docker／curl／sleep 驗證控制流程，不碰真實主機。
#   用法：bash scripts/host/test-socrates-prod-deploy.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WRAPPER="$HERE/socrates-prod-deploy"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

BIN="$WORK/bin"
BASE="$WORK/base"
BACKUPS="$WORK/backups"
LOG="$WORK/calls.log"
mkdir -p "$BIN" "$BASE"
: > "$LOG"
echo secret > "$BASE/.env"

cat > "$BIN/git" <<'STUB'
#!/usr/bin/env bash
echo "git $*" >> "$TEST_LOG"
case "$1" in
  merge-base) [ "${FAKE_ANCESTOR:-1}" = 1 ] ;;
  rev-parse) echo "${FAKE_HEAD:-0000000000000000000000000000000000000000}" ;;
  *) exit 0 ;;
esac
STUB
cat > "$BIN/docker" <<'STUB'
#!/usr/bin/env bash
echo "docker $* | APP_IMAGE_TAG=${APP_IMAGE_TAG:-}" >> "$TEST_LOG"
case "$1" in
  exec)
    [ "${FAKE_DUMP_FAIL:-0}" = 1 ] && exit 1
    [ "${FAKE_DUMP_EMPTY:-0}" = 1 ] && exit 0
    echo "-- fake dump"
    ;;
  image)
    for t in aaaa bbbb cccc dddd eeee ffff; do echo "socrates-ntnu-app:prod-$t"; done
    ;;
esac
exit 0
STUB
cat > "$BIN/curl" <<'STUB'
#!/usr/bin/env bash
echo "curl $*" >> "$TEST_LOG"
[ "${FAKE_CURL_FAIL:-0}" = 1 ] && exit 22
exit 0
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$BIN/sleep"
chmod +x "$BIN"/*

GOOD_SHA=abcdef1234567890abcdef1234567890abcdef12
export TEST_LOG="$LOG" PATH="$BIN:$PATH" PROD_BASE="$BASE" BACKUP_DIR="$BACKUPS"

pass=0
fail=0
check() {
  local name="$1" want="$2" got="$3"
  if [ "$want" = "$got" ]; then
    pass=$((pass + 1))
    echo "ok   - $name"
  else
    fail=$((fail + 1))
    echo "FAIL - $name（期望 exit=$want，實際 exit=$got）"
  fi
}
ok_if() {
  local name="$1"
  shift
  if "$@"; then
    pass=$((pass + 1))
    echo "ok   - $name"
  else
    fail=$((fail + 1))
    echo "FAIL - $name"
  fi
}
run() {
  # 執行包裝腳本，回傳 exit code（不讓 set -e 中止測試）
  local rc=0
  bash "$WRAPPER" "$@" >"$WORK/out" 2>&1 || rc=$?
  echo "$rc"
}
no_docker() { ! grep -q '^docker' "$LOG"; }

# 1. 參數數量與格式
check "無參數被拒" 2 "$(run)"
check "兩個參數被拒" 2 "$(FAKE_ANCESTOR=1 run "$GOOD_SHA" extra)"
check "短 SHA 被拒" 2 "$(run abc123)"
check "大寫 SHA 被拒" 2 "$(run ABCDEF1234567890ABCDEF1234567890ABCDEF12)"
check "含特殊字元的參數被拒" 2 "$(run "$GOOD_SHA;rm")"

# 2. 不在分支上的 commit 必須被拒，而且不能碰 docker（也不能備份）
: > "$LOG"
check "非分支祖先被拒" 1 "$(FAKE_ANCESTOR=0 run "$GOOD_SHA")"
ok_if "被拒時沒有呼叫 docker" no_docker

# 3. 正常部署：備份、建置、以專屬 tag 啟動、健康檢查、清理
: > "$LOG"
check "分支祖先可以部署" 0 "$(FAKE_ANCESTOR=1 FAKE_HEAD=$GOOD_SHA run "$GOOD_SHA")"
ok_if "備份檔已建立且不是空的" test -n "$(find "$BACKUPS" -name 'socrates-predeploy-*.sql.gz' -size +0 | head -1)"
ok_if "建置並以 prod 專屬 tag 啟動" bash -c "grep -q 'APP_IMAGE_TAG=prod-abcdef123456' '$LOG' && grep -q 'docker build' '$LOG'"
ok_if "只保留最近 5 個 prod 映像" grep -q 'docker image rm socrates-ntnu-app:prod-ffff' "$LOG"
ok_if "沒有使用 down 或 -v" bash -c "! grep -E 'docker compose.* down|docker compose.* -v( |$)' '$LOG'"
ok_if "備份在 pg_dump 之後才建置（備份先於任何變更）" bash -c "[ \$(grep -n 'docker exec' '$LOG' | head -1 | cut -d: -f1) -lt \$(grep -n 'docker build' '$LOG' | head -1 | cut -d: -f1) ]"

# 4. 備份失敗或備份是空的：不能建置、不能動容器
for mode in FAKE_DUMP_FAIL FAKE_DUMP_EMPTY; do
  : > "$LOG"
  rm -rf "$BACKUPS"
  check "$mode 時中止" 1 "$(env "$mode=1" FAKE_ANCESTOR=1 bash -c "bash '$WRAPPER' '$GOOD_SHA' >'$WORK/out' 2>&1; echo \$?")"
  ok_if "$mode 時沒有 build 與 compose" bash -c "! grep -E 'docker (build|compose)' '$LOG'"
  ok_if "$mode 時沒有留下空備份" bash -c "[ -z \"\$(find '$BACKUPS' -name '*.sql.gz' 2>/dev/null)\" ]"
done

# 5. 健康檢查失敗必須回非零
: > "$LOG"
check "健康檢查失敗回非零" 1 "$(FAKE_CURL_FAIL=1 FAKE_ANCESTOR=1 run "$GOOD_SHA")"

# 6. 缺少 .env 要失敗，而且不能碰 docker
mv "$BASE/.env" "$WORK/env.bak"
: > "$LOG"
check "缺少 .env 失敗" 1 "$(FAKE_ANCESTOR=1 run "$GOOD_SHA")"
mv "$WORK/env.bak" "$BASE/.env"
ok_if "缺少 .env 時沒有呼叫 docker" no_docker

echo
echo "結果：$pass 通過，$fail 失敗"
[ "$fail" -eq 0 ]
