#!/usr/bin/env bash
# 一鍵安裝：檢查 Docker、產生 .env、建置並啟動整個系統（資料庫＋後端＋前端）。
# 只需要 Docker（含 Compose v2）與 openssl 或 /dev/urandom。
#
# 用法：
#   ./scripts/install.sh
# 非互動：先以環境變數提供下列必填值即可略過提問。
#   GOOGLE_CLIENT_ID  GOOGLE_CLIENT_SECRET  ADMIN_EMAILS  OPENAI_API_KEY  OPENAI_MODEL
# 可選：APP_BASE_URL（預設 http://localhost:3000）、ADMIN_INITIAL_PASSWORD（預設隨機產生）
#
# 已有 .env 時不會覆寫，直接使用它。
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "錯誤：$*" >&2; exit 1; }

command -v docker >/dev/null 2>&1 || die "找不到 docker，請先安裝 Docker"
docker compose version >/dev/null 2>&1 || die "需要 Docker Compose v2（docker compose）"
docker info >/dev/null 2>&1 || die "無法連線到 Docker daemon，請確認 Docker 已啟動"

ask() {
  # ask VAR "提示" [secret]
  local var="$1" prompt="$2" secret="${3:-}" value="${!1:-}"
  if [ -z "$value" ]; then
    [ -t 0 ] || die "非互動環境請先設定環境變數 $var"
    while [ -z "$value" ]; do
      if [ -n "$secret" ]; then
        read -r -s -p "$prompt: " value
        echo
      else
        read -r -p "$prompt: " value
      fi
    done
  fi
  printf -v "$var" '%s' "$value"
}

esc() { printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'; }

random_password() {
  LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 20
}

if [ -f .env ]; then
  echo "使用既有的 .env（不覆寫）"
  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
else
  ask GOOGLE_CLIENT_ID "Google OAuth client ID"
  ask GOOGLE_CLIENT_SECRET "Google OAuth client secret" secret
  ask ADMIN_EMAILS "管理者信箱（多個以逗號分隔）"
  ask OPENAI_API_KEY "OpenAI API key" secret
  ask OPENAI_MODEL "OpenAI 模型名稱"
  APP_BASE_URL="${APP_BASE_URL:-http://localhost:3000}"
  ADMIN_INITIAL_PASSWORD="${ADMIN_INITIAL_PASSWORD:-$(random_password)}"
  GENERATED_PASSWORD="$ADMIN_INITIAL_PASSWORD"

  umask 077
  sed \
    -e "s|^APP_BASE_URL=.*|APP_BASE_URL=$(esc "${APP_BASE_URL}")|" \
    -e "s|^GOOGLE_CLIENT_ID=.*|GOOGLE_CLIENT_ID=$(esc "${GOOGLE_CLIENT_ID}")|" \
    -e "s|^GOOGLE_CLIENT_SECRET=.*|GOOGLE_CLIENT_SECRET=$(esc "${GOOGLE_CLIENT_SECRET}")|" \
    -e "s|^ADMIN_EMAILS=.*|ADMIN_EMAILS=$(esc "${ADMIN_EMAILS}")|" \
    -e "s|^ADMIN_INITIAL_PASSWORD=.*|ADMIN_INITIAL_PASSWORD=$(esc "${ADMIN_INITIAL_PASSWORD}")|" \
    -e "s|^OPENAI_API_KEY=.*|OPENAI_API_KEY=$(esc "${OPENAI_API_KEY}")|" \
    -e "s|^OPENAI_MODEL=.*|OPENAI_MODEL=$(esc "${OPENAI_MODEL}")|" \
    .env.example >.env
  echo "已建立 .env"
fi

echo "建置並啟動服務（首次建置需要數分鐘）..."
docker compose --profile app up --build -d

URL="${APP_BASE_URL:-http://localhost:3000}"
echo "等待服務就緒..."
ready=""
for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:3000/health" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
[ -n "$ready" ] || die "服務未在時限內就緒，請用 docker compose logs app 查看原因"

echo
echo "安裝完成：$URL"
if [ -n "${GENERATED_PASSWORD:-}" ]; then
  echo "管理者帳號：${ADMIN_EMAILS%%,*}"
  echo "初始密碼：${GENERATED_PASSWORD}（首次登入須更改；亦寫在 .env）"
fi
echo "停止：docker compose --profile app down"
