#!/usr/bin/env bash
# 把 msi_cubi 上的 staging（compose 專案 socrates-staging）更新到指定的 deploy/msi-cubi-staging commit。
# 由 .github/workflows/staging-deploy.yml 在主機的 self-hosted runner 上呼叫；也可以在主機上手動執行。
#
#   用法：scripts/deploy-staging.sh <commit-sha>
#   環境變數：
#     STAGING_DIR          staging 的部署目錄，預設 $HOME/socrates-staging。必須是 deploy/msi-cubi-staging 的 git clone，
#                          並含不進版控的 .env.staging、.apikeylist（見 docs/ntnu-ops.md）。
#     STAGING_PROXY_PORT   健康檢查用，預設 3081（與 docker-compose.staging.yml 相同）。
#
# 只動 staging：不碰正式環境（socrates-ntnu）的容器、volume 與 .env；不使用 down、不使用 -v。
# 沒有 STAGING_DEMO_MODE，所以自動部署不會把示範帳號打開。
set -euo pipefail

SHA="${1:?用法：deploy-staging.sh <commit-sha>}"
STAGING_DIR="${STAGING_DIR:-$HOME/socrates-staging}"
BRANCH="deploy/msi-cubi-staging"
PROXY_PORT="${STAGING_PROXY_PORT:-3081}"
IMAGE_REPO="socrates-ntnu-app"
KEEP_IMAGES=5

cd "$STAGING_DIR"
for f in .env.staging .apikeylist; do
  if [ ! -f "$f" ]; then
    echo "缺少 $STAGING_DIR/$f（不進版控，需在主機上先放好）" >&2
    exit 1
  fi
done

echo "→ 取得 origin/$BRANCH"
git fetch --quiet origin "$BRANCH"

# 這次 push 之後分支若又被推進，最新的那次執行會負責部署；這次就略過，避免倒退到舊 commit。
HEAD_SHA="$(git rev-parse "origin/$BRANCH")"
if [ "$SHA" != "$HEAD_SHA" ]; then
  echo "分支已前進到 $HEAD_SHA，略過 $SHA（由較新的執行部署）"
  exit 0
fi

echo "→ 切換到 $SHA"
# --hard 只改追蹤中的檔案；.gitignore 內的 .env.staging、.apikeylist、.staging-credentials 不會被動到。
git reset --hard --quiet "$SHA"

TAG="staging-$(git rev-parse --short=12 HEAD)"
echo "→ 建置映像 $IMAGE_REPO:$TAG"
docker build --quiet -t "$IMAGE_REPO:$TAG" .

echo "→ 更新 staging 服務（STAGING_APP_IMAGE_TAG=$TAG）"
STAGING_APP_IMAGE_TAG="$TAG" docker compose -f docker-compose.staging.yml up -d --no-build

echo "→ 健康檢查（經 staging-proxy 的 /ready）"
ok=0
for _ in $(seq 1 30); do
  if curl -fsS -o /dev/null "http://127.0.0.1:$PROXY_PORT/ready"; then
    ok=1
    break
  fi
  sleep 2
done
if [ "$ok" -ne 1 ]; then
  echo "健康檢查失敗：http://127.0.0.1:$PROXY_PORT/ready 60 秒內沒有回 200" >&2
  docker compose -f docker-compose.staging.yml ps >&2
  exit 1
fi

echo "→ 清理舊的 staging 映像（保留最近 $KEEP_IMAGES 個；正在使用的會留下）"
docker image ls --filter "reference=$IMAGE_REPO:staging-*" --format '{{.Repository}}:{{.Tag}}' \
  | tail -n +"$((KEEP_IMAGES + 1))" \
  | xargs -r docker image rm >/dev/null 2>&1 || true

docker compose -f docker-compose.staging.yml ps
echo "完成：staging 已部署 $SHA（$TAG）"
