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
printf 'SECRET=1\nCASCADE_INTEL_TAG=intel-old\n' > "$BASE/.env"
mkdir -p "$BASE/docker/cascade-intel/npu-debs"
touch "$BASE/docker/cascade-intel/npu-debs/driver.deb"

cat > "$BIN/git" <<'STUB'
#!/usr/bin/env bash
echo "git $*" >> "$TEST_LOG"
case "$1" in
  merge-base) [ "${FAKE_ANCESTOR:-1}" = 1 ] ;;
  log) echo "${FAKE_CASCADE_COMMIT-1111111111111111111111111111111111111111}" ;;
  rev-parse)
    case "$2" in
      origin/*) echo "${FAKE_BRANCH_HEAD:-abcdef1234567890abcdef1234567890abcdef12}" ;;
      *) echo "${FAKE_HEAD:-0000000000000000000000000000000000000000}" ;;
    esac ;;
  *) exit 0 ;;
esac
STUB
cat > "$BIN/docker" <<'STUB'
#!/usr/bin/env bash
echo "docker $* | APP_IMAGE_TAG=${APP_IMAGE_TAG:-} CASCADE_INTEL_TAG=${CASCADE_INTEL_TAG:-}" >> "$TEST_LOG"
case "$1" in
  exec)
    if [[ " $* " == *" socrates-ntnu-cascade-1 "* ]]; then
      [ "${FAKE_WS_EXEC_FAIL:-0}" = 1 ] && exit 1
      echo "${FAKE_WS_COUNT:-0}"
      exit 0
    fi
    [ "${FAKE_DUMP_FAIL:-0}" = 1 ] && exit 1
    [ "${FAKE_DUMP_EMPTY:-0}" = 1 ] && exit 0
    echo "-- fake dump"
    ;;
  build)
    [ "${FAKE_BUILD_FAIL:-0}" = 1 ] && [[ " $* " == *" socrates-cascade-"* ]] && exit 1
    ;;
  inspect)
    case "$*" in
      *Config.Image*) [ -n "${FAKE_RUNNING_TAG:-}" ] && echo "socrates-cascade-core:$FAKE_RUNNING_TAG" || exit 1 ;;
      *Health.Status*) echo "${FAKE_CASCADE_HEALTH:-healthy}" ;;
      *State.Running*) echo true ;;
    esac
    ;;
  info) echo "$WORK_DOCKER_ROOT" ;;
  image)
    case "$2" in
      inspect) [[ " ${FAKE_IMAGES_EXIST:-} " == *" $3 "* ]] || exit 1 ;;
      ls)
        case "$*" in
          *label=*) echo "${FAKE_LABELED_ID:-}" ;;
          *reference=socrates-cascade-*:intel-*) printf '%s\n' ${FAKE_CASCADE_TAGS:-} ;;
          *reference=socrates-cascade-*) echo "${FAKE_ANY_ID:-sha256:anyimage}" ;;
          *) for t in aaaa bbbb cccc dddd eeee ffff; do echo "socrates-ntnu-app:prod-$t"; done ;;
        esac
        ;;
    esac
    ;;
esac
exit 0
STUB
cat > "$BIN/df" <<'STUB'
#!/usr/bin/env bash
printf 'Avail\n %sG\n' "${FAKE_FREE_GB:-100}"
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
export TEST_LOG="$LOG" PATH="$BIN:$PATH" PROD_BASE="$BASE" BACKUP_DIR="$BACKUPS" WORK_DOCKER_ROOT="$WORK/docker-root"
CTAG=intel-111111111111   # FAKE_CASCADE_COMMIT 的前 12 碼

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

# 2. 不是分支 head 的 commit 必須被拒（含分支上較舊的），而且不能碰 docker（也不能備份）
: > "$LOG"
check "不是分支 head 被拒" 1 "$(FAKE_BRANCH_HEAD=1111111111111111111111111111111111111111 run "$GOOD_SHA")"
ok_if "被拒時沒有呼叫 docker" no_docker
: > "$LOG"
check "舊的祖先 commit 在沒有 rollback 旗標時被拒" 1 "$(FAKE_BRANCH_HEAD=1111111111111111111111111111111111111111 FAKE_ANCESTOR=1 run "$GOOD_SHA")"
ok_if "舊祖先被拒時沒有呼叫 docker" no_docker
: > "$LOG"
check "root 設 PROD_ALLOW_ROLLBACK=1 可回滾到祖先" 0 "$(PROD_ALLOW_ROLLBACK=1 FAKE_BRANCH_HEAD=1111111111111111111111111111111111111111 FAKE_ANCESTOR=1 FAKE_HEAD=$GOOD_SHA run "$GOOD_SHA")"
: > "$LOG"
check "rollback 旗標仍拒絕分支外的 commit" 1 "$(PROD_ALLOW_ROLLBACK=1 FAKE_ANCESTOR=0 run "$GOOD_SHA")"
ok_if "分支外被拒時沒有呼叫 docker" no_docker

# 3. 正常部署：備份、建置、以專屬 tag 啟動、健康檢查、清理
: > "$LOG"
check "分支 head 可以部署" 0 "$(FAKE_ANCESTOR=1 FAKE_HEAD=$GOOD_SHA run "$GOOD_SHA")"
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


# 7. cascade 映像：沒有變動（目前的 tag 就是需要的 tag）時，不建映像、不清建置快取，tag 原樣傳給 compose
reset_state() { : > "$LOG"; printf 'SECRET=1\nCASCADE_INTEL_TAG=intel-old\n' > "$BASE/.env"; }
ALL_IMAGES="socrates-cascade-core:$CTAG socrates-cascade-tts-xpu:$CTAG socrates-cascade-whisper-npu:$CTAG"
reset_state
check "cascade 沒有變動時可以部署" 0 "$(FAKE_RUNNING_TAG=$CTAG FAKE_IMAGES_EXIST="$ALL_IMAGES" run "$GOOD_SHA")"
ok_if "沒有建 cascade 映像" bash -c "! grep -E 'docker build.*socrates-cascade' '$LOG'"
ok_if "沒有清建置快取" bash -c "! grep 'builder prune' '$LOG'"
ok_if "沒有因此等待場次" bash -c "! grep 'docker exec.*socrates-ntnu-cascade-1' '$LOG'"
ok_if "compose 帶著同一個 cascade tag" grep -q "docker compose.*up -d --no-build | APP_IMAGE_TAG=prod-.* CASCADE_INTEL_TAG=$CTAG" "$LOG"

# 8. cascade 有變動、沒有場次：先建完映像才動容器，tag 用 commit SHA，建完清建置快取，.env 同步
reset_state
check "cascade 有變動且沒有場次時部署成功" 0 "$(FAKE_RUNNING_TAG=intel-old FAKE_WS_COUNT=0 FAKE_FREE_GB=100 FAKE_CASCADE_TAGS='intel-old intel-older intel-oldest' run "$GOOD_SHA")"
ok_if "建了 core 與 tts，tag 是 intel-<commit 前 12 碼>" bash -c "grep -q 'docker build.*-t socrates-cascade-core:$CTAG' '$LOG' && grep -q 'docker build.*-t socrates-cascade-tts-xpu:$CTAG' '$LOG'"
ok_if "映像帶輸入的 commit 標籤" grep -q 'label socrates.inputs=1111111111111111111111111111111111111111' "$LOG"
ok_if "建完才啟動 compose" bash -c "[ \$(grep -n 'docker build.*socrates-cascade' '$LOG' | tail -1 | cut -d: -f1) -lt \$(grep -n 'docker compose.*up -d' '$LOG' | head -1 | cut -d: -f1) ]"
ok_if "建完清建置快取" grep -q 'docker builder prune -a -f' "$LOG"
ok_if "compose 用新的 cascade tag" grep -q "docker compose.*up -d --no-build | APP_IMAGE_TAG=prod-.* CASCADE_INTEL_TAG=$CTAG" "$LOG"
ok_if ".env 的 CASCADE_INTEL_TAG 同步成新 tag，其他行不變" bash -c "grep -qx 'CASCADE_INTEL_TAG=$CTAG' '$BASE/.env' && grep -qx 'SECRET=1' '$BASE/.env'"
ok_if "舊 tag 只保留最近 2 個，正在使用的與新的不刪" bash -c "grep -q 'docker image rm socrates-cascade-core:intel-oldest' '$LOG' && ! grep -q 'docker image rm socrates-cascade-core:intel-old\$' '$LOG' && ! grep -q 'docker image rm socrates-cascade-core:$CTAG' '$LOG'"

# 9. 已有相同輸入的映像（標籤相符）：只加 tag，不重建
reset_state
check "輸入沒變的映像只加 tag" 0 "$(FAKE_RUNNING_TAG=intel-old FAKE_LABELED_ID=sha256:labeled run "$GOOD_SHA")"
ok_if "用 docker tag 取代 docker build" bash -c "grep -q 'docker tag sha256:labeled socrates-cascade-core:$CTAG' '$LOG' && ! grep -E 'docker build.*socrates-cascade' '$LOG'"

# 10. 建映像失敗：不能動任何容器，.env 不變
reset_state
check "建映像失敗時中止" 1 "$(FAKE_RUNNING_TAG=intel-old FAKE_BUILD_FAIL=1 run "$GOOD_SHA")"
ok_if "建映像失敗時沒有 compose" bash -c "! grep 'docker compose' '$LOG'"
ok_if "建映像失敗時 .env 不變" grep -qx 'CASCADE_INTEL_TAG=intel-old' "$BASE/.env"

# 11. 磁碟不夠：不建映像、不動容器
reset_state
check "磁碟空間不足時中止" 1 "$(FAKE_RUNNING_TAG=intel-old FAKE_FREE_GB=10 run "$GOOD_SHA")"
ok_if "磁碟不足時沒有建 cascade 映像也沒有 compose" bash -c "! grep -E 'docker build.*socrates-cascade' '$LOG' && ! grep 'docker compose' '$LOG'"

# 12. 一直有人在用語音：延後換 cascade，其餘照常部署，cascade 維持原 tag
reset_state
check "有場次時部署仍成功（延後 cascade）" 0 "$(FAKE_RUNNING_TAG=intel-old FAKE_WS_COUNT=2 run "$GOOD_SHA")"
ok_if "延後時 compose 用目前執行中的 cascade tag" grep -q "docker compose.*up -d --no-build | APP_IMAGE_TAG=prod-.* CASCADE_INTEL_TAG=intel-old" "$LOG"
ok_if "延後時 .env 維持原 tag" grep -qx 'CASCADE_INTEL_TAG=intel-old' "$BASE/.env"
ok_if "延後時有說明" grep -q '延後' "$WORK/out"
# 讀不到場次（docker exec 失敗）也要保守地延後
reset_state
check "讀不到場次時保守延後" 0 "$(FAKE_RUNNING_TAG=intel-old FAKE_WS_EXEC_FAIL=1 run "$GOOD_SHA")"
ok_if "讀不到場次時沒有換 tag" grep -q "docker compose.*up -d --no-build | APP_IMAGE_TAG=prod-.* CASCADE_INTEL_TAG=intel-old" "$LOG"

# 13. 換 cascade 後它沒有變 healthy：部署要算失敗
reset_state
check "cascade 沒變 healthy 時失敗" 1 "$(FAKE_RUNNING_TAG=intel-old FAKE_CASCADE_HEALTH=unhealthy run "$GOOD_SHA")"

# 14. 主機沒有 NPU 驅動 .deb：whisper 映像不重建，沿用現有映像並加 tag，部署不被擋
rm -f "$BASE/docker/cascade-intel/npu-debs/driver.deb"
reset_state
check "缺 .deb 時仍可部署" 0 "$(FAKE_RUNNING_TAG=intel-old run "$GOOD_SHA")"
ok_if "沒有重建 whisper，改沿用現有映像" bash -c "! grep -E 'docker build.*socrates-cascade-whisper-npu' '$LOG' && grep -q 'docker tag sha256:anyimage socrates-cascade-whisper-npu:$CTAG' '$LOG'"
ok_if "缺 .deb 時有警告" grep -q 'npu-debs' "$WORK/out"
touch "$BASE/docker/cascade-intel/npu-debs/driver.deb"

# 15. 找不到任何 cascade 輸入的 commit：中止，不動容器
reset_state
check "算不出 cascade tag 時中止" 1 "$(FAKE_CASCADE_COMMIT= run "$GOOD_SHA")"
ok_if "算不出 tag 時沒有 compose" bash -c "! grep 'docker compose' '$LOG'"

# 16. 輸入清單要涵蓋各 Dockerfile 的 COPY 來源，之後新增 COPY 卻忘了改清單會在這裡失敗
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
inputs_in_wrapper="$(grep -E '^  \[socrates-cascade-[a-z-]+\]="' "$WRAPPER" | grep -v dockerfile || true)"
cover_missing=""
for df in core tts whisper; do
  f="$REPO_ROOT/docker/cascade-intel/$df.Dockerfile"
  while read -r src; do
    case "$src" in
      docker/cascade-intel/npu-debs/*) src=docker/cascade-intel/npu-debs ;;
    esac
    grep -q "$src" <<<"$(cat "$WRAPPER")" || cover_missing="$cover_missing $df:$src"
  done < <(awk '/^COPY / { for (i = 2; i < NF; i++) if ($i !~ /^--/) print $i }' "$f")
done
ok_if "輸入清單涵蓋 Dockerfile 的 COPY 來源（缺：${cover_missing:- 無}）" test -z "$cover_missing"

# 17. 場次偵測讀 cascade 的 /health（不是 TCP 連線數：app 的探測連線常駐，混在一起分不出來）
ok_if "場次偵測讀 /health 的 request_states" bash -c "grep -q 'request_states' '$WRAPPER' && grep -q '127.0.0.1:8110/health' '$WRAPPER'"
ok_if "沒有用 TCP 連線數判斷場次" bash -c "! grep -q '/proc/net/tcp' '$WRAPPER' || ! grep -q '\"01\"' '$WRAPPER'"

echo
echo "結果：$pass 通過，$fail 失敗"
[ "$fail" -eq 0 ]
