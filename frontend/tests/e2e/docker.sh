#!/bin/sh
# 在官方 Playwright Docker image（Linux）裡跑 e2e，讓截圖與 CI 完全一致。
# 用法（在 frontend/）：npm run test:e2e:linux [-- <playwright test 參數>]
#   更新基準截圖：npm run test:e2e:linux -- --update-snapshots
# image 版本跟著 node_modules 裡的 @playwright/test，升級套件時不用改這裡。
# 固定 linux/amd64（與 GitHub 的 ubuntu runner 相同）：Apple Silicon 上的 arm64 Chromium 字型點陣化可能略有差異。
set -eu
cd "$(dirname "$0")/../.."

version=$(node -p "require('./node_modules/@playwright/test/package.json').version")
image="mcr.microsoft.com/playwright:v${version}-noble"

if [ -n "${CI:-}" ]; then
  # CI 的主機本來就是 Linux x64，直接用主機 npm ci 裝好的 node_modules
  modules=""
  install=""
else
  # macOS／Windows 的 node_modules 帶的是主機平台的原生套件，容器裡另外裝一份（存在具名 volume，下次重用）
  modules="-v socrates-chat-e2e-node-modules:/work/node_modules"
  install="npm ci --no-audit --no-fund --loglevel=error &&"
fi

# shellcheck disable=SC2086
exec docker run --rm --init --ipc=host --platform linux/amd64 \
  -e CI -e GITHUB_ACTIONS \
  -v "$PWD":/work $modules -w /work \
  "$image" \
  sh -c "$install npx playwright test \"\$@\"" playwright-e2e "$@"
