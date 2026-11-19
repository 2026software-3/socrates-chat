#!/usr/bin/env bash
# 驗證反向代理（S-16）：用假後端起一個獨立的 compose 專案，檢查
#   後端在線：一般轉發、X-Forwarded-For／Host／協定、後端自己的 503 原樣轉發、SSE 不緩衝、WebSocket 升級、大請求
#   後端停掉：503 靜態停機頁（三語、不快取），之後後端回來能自動恢復
# 代理是 Pingap（見 docs/specs/decisions.md S-16）。需要 docker compose 與 python3。專案名稱固定為 socrates-proxy-verify，不碰任何其他 compose 專案（含 socrates-ntnu）。
set -euo pipefail
cd "$(dirname "$0")/.."

project=socrates-proxy-verify
port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
export PROXY_PORT="$port"
base="http://127.0.0.1:$port"
compose=(docker compose -p "$project" -f docker-compose.yml -f docker/proxy/verify.compose.yml --profile proxy)

cleanup() { "${compose[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

# 等 $base$1 回 $2
wait_for() {
  local path="$1" want="$2" i code=""
  for i in $(seq 1 60); do
    code="$(python3 - "$base$path" <<'PY' 2>/dev/null || true
import sys, urllib.request, urllib.error
try:
    print(urllib.request.urlopen(sys.argv[1], timeout=3).status)
except urllib.error.HTTPError as e:
    print(e.code)
PY
)"
    [ "$code" = "$want" ] && return 0
    sleep 1
  done
  echo "等不到 $base$path 回 $want（最後是 ${code:-無回應}）" >&2
  "${compose[@]}" logs --no-color >&2 || true
  return 1
}

echo "==> 啟動代理與假後端（專案 $project，埠 $port）"
"${compose[@]}" up -d proxy fake-app
wait_for /proxy-health 200
wait_for /health 200

# 設定檔要通過 Pingap 自己的檢查（pingap -t），並確認逾時與本文大小的設定確實在檔案裡。
# Pingap 轉發本來就是串流、沒有 nginx 的 proxy_buffering 開關，SSE 逐段送達由下面的行為檢查把關。
echo "==> 設定檔檢查"
"${compose[@]}" exec -T proxy pingap -t -c /opt/pingap/pingap.toml >/dev/null 2>&1 || {
  echo "  FAIL pingap -t 沒通過" >&2
  "${compose[@]}" exec -T proxy pingap -t -c /opt/pingap/pingap.toml >&2 || true
  exit 1
}
echo "  ok   pingap -t"
for directive in 'read_timeout = "600s"' 'read_timeout = "3600s"' 'connection_timeout = "5s"' 'client_max_body_size = "11mb"' '"5xx:/opt/pingap/down.html"'; do
  if grep -qF "$directive" docker/proxy/pingap.toml; then
    echo "  ok   $directive"
  else
    echo "  FAIL docker/proxy/pingap.toml 裡沒有 $directive" >&2
    exit 1
  fi
done

echo "==> 後端在線"
python3 scripts/proxy-check.py check up "$base"

echo "==> 停掉後端"
"${compose[@]}" stop fake-app
wait_for /health 503
python3 scripts/proxy-check.py check down "$base"

echo "==> 後端回來，代理自動恢復"
"${compose[@]}" start fake-app
wait_for /health 200
python3 scripts/proxy-check.py check up "$base"

echo "==> 全部通過"
