#!/usr/bin/env bash
# socrates-staging-deploy 的回歸測試：用假的 git／docker／curl／sleep 驗證控制流程，不碰真實主機。
#   用法：bash scripts/host/test-socrates-staging-deploy.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WRAPPER="$HERE/socrates-staging-deploy"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

BIN="$WORK/bin"
BASE="$WORK/base"
LOG="$WORK/calls.log"
mkdir -p "$BIN" "$BASE"
: > "$LOG"
echo secret > "$BASE/.env.staging"

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
echo "docker $* | STAGING_APP_IMAGE_TAG=${STAGING_APP_IMAGE_TAG:-}" >> "$TEST_LOG"
if [ "$1" = image ]; then
  for t in aaaa bbbb cccc dddd eeee ffff; do echo "socrates-ntnu-app:staging-$t"; done
fi
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
export TEST_LOG="$LOG" PATH="$BIN:$PATH" STAGING_BASE="$BASE"

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
run() {
  # 執行包裝腳本，回傳 exit code（不讓 set -e 中止測試）
  local rc=0
  bash "$WRAPPER" "$@" >"$WORK/out" 2>&1 || rc=$?
  echo "$rc"
}

# 1. 參數數量與格式
check "無參數被拒" 2 "$(run)"
check "兩個參數被拒" 2 "$(FAKE_ANCESTOR=1 run "$GOOD_SHA" extra)"
check "短 SHA 被拒" 2 "$(run abc123)"
check "大寫 SHA 被拒" 2 "$(run ABCDEF1234567890ABCDEF1234567890ABCDEF12)"
check "含特殊字元的參數被拒" 2 "$(run "$GOOD_SHA;rm")"

# 2. 不在分支上的 commit 必須被拒，而且不能碰 docker
: > "$LOG"
check "非分支祖先被拒" 1 "$(FAKE_ANCESTOR=0 run "$GOOD_SHA")"
if grep -q '^docker' "$LOG"; then
  fail=$((fail + 1)); echo "FAIL - 被拒時仍呼叫了 docker"
else
  pass=$((pass + 1)); echo "ok   - 被拒時沒有呼叫 docker"
fi

# 3. 正常部署：建置、以 staging 專屬 tag 啟動、健康檢查、清理
: > "$LOG"
check "分支祖先可以部署" 0 "$(FAKE_ANCESTOR=1 FAKE_HEAD=$GOOD_SHA run "$GOOD_SHA")"
if grep -q "STAGING_APP_IMAGE_TAG=staging-abcdef123456" "$LOG" && grep -q 'docker build' "$LOG"; then
  pass=$((pass + 1)); echo "ok   - 建置並以 staging 專屬 tag 啟動"
else
  fail=$((fail + 1)); echo "FAIL - 建置或 tag 不正確"; cat "$LOG"
fi
if grep -q 'docker image rm socrates-ntnu-app:staging-ffff' "$LOG"; then
  pass=$((pass + 1)); echo "ok   - 只保留最近 5 個 staging 映像"
else
  fail=$((fail + 1)); echo "FAIL - 映像清理不正確"
fi

# 4. 健康檢查失敗必須回非零
: > "$LOG"
check "健康檢查失敗回非零" 1 "$(FAKE_CURL_FAIL=1 FAKE_ANCESTOR=1 run "$GOOD_SHA")"

# 5. 缺少密鑰檔要失敗，而且不能碰 docker
mv "$BASE/.env.staging" "$WORK/env.bak"
: > "$LOG"
check "缺少 .env.staging 失敗" 1 "$(FAKE_ANCESTOR=1 run "$GOOD_SHA")"
mv "$WORK/env.bak" "$BASE/.env.staging"
if grep -q '^docker' "$LOG"; then
  fail=$((fail + 1)); echo "FAIL - 缺少密鑰時仍呼叫了 docker"
else
  pass=$((pass + 1)); echo "ok   - 缺少密鑰時沒有呼叫 docker"
fi

echo
echo "結果：$pass 通過，$fail 失敗"
[ "$fail" -eq 0 ]
