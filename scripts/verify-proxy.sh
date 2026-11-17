#!/usr/bin/env bash
# 驗證反向代理（S-16）：用假後端起一個獨立的 compose 專案，檢查
#   後端在線：一般轉發、X-Forwarded-For／Host／協定、後端自己的 503 原樣轉發、SSE 不緩衝、WebSocket 升級、大請求
#   後端停掉：503 靜態停機頁（三語、不快取），之後後端回來能自動恢復
# 需要 docker compose 與 python3。專案名稱固定為 socrates-proxy-verify，不碰任何其他 compose 專案（含 socrates-ntnu）。
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

# SSE 不緩衝：小量事件在 nginx 緩衝開著時也常常即時送出，行為檢查分辨不出來，
# 所以另外確認「生效中的設定」確實關掉緩衝，並把逾時設得比 AI 回覆的產生時間長
echo "==> 生效中的 nginx 設定"
effective="$("${compose[@]}" exec -T proxy nginx -T 2>&1)"
for directive in 'proxy_buffering off;' 'proxy_request_buffering off;' 'proxy_http_version 1.1;' 'proxy_read_timeout 600s;' 'proxy_read_timeout 3600s;'; do
  if grep -qF "$directive" <<<"$effective"; then
    echo "  ok   $directive"
  else
    echo "  FAIL 生效設定裡沒有 $directive" >&2
    exit 1
  fi
done
if grep -qE '^\s*proxy_buffering on;' <<<"$effective"; then
  echo "  FAIL 生效設定裡有 proxy_buffering on;" >&2
  exit 1
fi

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
