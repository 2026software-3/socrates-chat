#!/usr/bin/env bash
# 正式環境自動部署的主機端一次性設定（msi_cubi，需要 root）。互動式：每一步先說明、等你按 Enter 才做。
#
#   用法（在 msi_cubi 上，PR 已合進 deploy/msi-cubi-intel 之後）：
#     sudo bash setup-prod-autodeploy.sh
#
# 做的事（都可重跑，已完成的步驟會略過）：
#   1. 建立 /var/lib/socrates-prod（deploy/msi-cubi-intel 的 root 擁有的 clone）與 /var/backups/socrates-prod
#   2. 把 ~/socrates-intel/.env 複製成 /var/lib/socrates-prod/.env（root 0600）；缺 TEACHER_KEY_ENCRYPTION_KEYS 就產生一把
#   3. 安裝部署包裝腳本與 sudoers
#   4. 建立專用使用者 gha-runner-prod（不在 docker 群組），安裝並啟動 self-hosted runner（label msi-cubi-prod）
#   5. 驗證：runner 使用者不能用 docker、包裝腳本會拒絕不在分支上的 SHA
# 不會做：不動正在跑的容器、不動資料庫 volume、不刪 ~/socrates-intel 與 .apikeylist。
set -euo pipefail

REPO_URL="https://github.com/2026software-3/socrates-chat"
BRANCH="deploy/msi-cubi-intel"
BASE="/var/lib/socrates-prod"
BACKUPS="/var/backups/socrates-prod"
OLD_DIR="${OLD_DIR:-/home/ray520/socrates-intel}"
RUNNER_USER="gha-runner-prod"
RUNNER_HOME="/home/$RUNNER_USER"
RUNNER_DIR="$RUNNER_HOME/actions-runner"

if [ "$(id -u)" -ne 0 ]; then
  echo "請用 sudo 執行：sudo bash $0" >&2
  exit 1
fi

step() {
  echo
  echo "=== $1 ==="
  echo "$2"
  read -r -p "按 Enter 執行，輸入 s 略過，Ctrl-C 中止：" ans
  [ "$ans" != "s" ]
}

if step "1/5 建立部署目錄與備份目錄" "git clone -b $BRANCH $REPO_URL $BASE；mkdir $BACKUPS（root 0700）"; then
  if [ -d "$BASE/.git" ]; then
    echo "已存在 $BASE，改為 fetch 並對齊 origin/$BRANCH"
    git -C "$BASE" fetch --quiet origin "$BRANCH"
    git -C "$BASE" reset --hard --quiet "origin/$BRANCH"
  else
    git clone --quiet -b "$BRANCH" "$REPO_URL" "$BASE"
  fi
  chown -R root:root "$BASE"
  install -d -o root -g root -m 0700 "$BACKUPS"
  git -C "$BASE" log --oneline -1
fi

if step "2/5 放入 .env" "從 $OLD_DIR/.env 複製到 $BASE/.env（root 0600）。缺 TEACHER_KEY_ENCRYPTION_KEYS 時自動產生一把並加進去。
    這把金鑰加密資料庫裡所有 AI key（系統與教師）；遺失就解不開，請務必另存到密碼管理員。"; then
  [ -f "$OLD_DIR/.env" ] || { echo "找不到 $OLD_DIR/.env" >&2; exit 1; }
  if [ -f "$BASE/.env" ]; then
    echo "$BASE/.env 已存在，不覆蓋（只檢查 TEACHER_KEY_ENCRYPTION_KEYS）"
  else
    install -o root -g root -m 0600 "$OLD_DIR/.env" "$BASE/.env"
  fi
  if grep -q '^TEACHER_KEY_ENCRYPTION_KEYS=.\+' "$BASE/.env"; then
    echo "TEACHER_KEY_ENCRYPTION_KEYS 已設定"
  else
    key="k1:$(openssl rand -base64 32)"
    printf '\nTEACHER_KEY_ENCRYPTION_KEYS=%s\n' "$key" >> "$BASE/.env"
    echo
    echo "已產生 TEACHER_KEY_ENCRYPTION_KEYS，請現在複製存好（只會顯示這一次）："
    echo "  $key"
    read -r -p "存好了按 Enter：" _
  fi
  echo "提醒：.env 裡舊的 OPENAI_API_KEY／OPENAI_API_KEYS／OPENAI_API_KEYS_FILE 不再使用，可以刪掉。"
fi

if step "3/5 安裝包裝腳本與 sudoers" "install scripts/host/socrates-prod-deploy → /usr/local/sbin（root 0755）；
    visudo -cf 通過後 install gha-runner-prod.sudoers → /etc/sudoers.d/gha-runner-prod（root 0440）"; then
  install -o root -g root -m 0755 "$BASE/scripts/host/socrates-prod-deploy" /usr/local/sbin/socrates-prod-deploy
  visudo -cf "$BASE/scripts/host/gha-runner-prod.sudoers"
  install -o root -g root -m 0440 "$BASE/scripts/host/gha-runner-prod.sudoers" /etc/sudoers.d/gha-runner-prod
fi

if step "4/5 建立 runner 使用者並註冊 self-hosted runner" "使用者 $RUNNER_USER（不加入 docker 群組）、目錄 $RUNNER_DIR、label msi-cubi-prod。
    需要註冊 token：到 https://github.com/2026software-3/socrates-chat/settings/actions/runners/new 複製 --token 後面那串（約 1 小時失效）。"; then
  id "$RUNNER_USER" >/dev/null 2>&1 || useradd -m -s /bin/bash "$RUNNER_USER"
  if id -nG "$RUNNER_USER" | grep -qw docker; then gpasswd -d "$RUNNER_USER" docker; fi
  if [ -x "$RUNNER_DIR/svc.sh" ] && [ -f "$RUNNER_DIR/.runner" ]; then
    echo "runner 已註冊，略過（要重新註冊請先刪除 $RUNNER_DIR/.runner）"
  else
    read -r -p "貼上註冊 token：" TOKEN
    case "$(uname -m)" in x86_64) arch=x64 ;; aarch64) arch=arm64 ;; *) echo "不支援的架構" >&2; exit 1 ;; esac
    ver="$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -1)"
    [ -n "$ver" ] || { echo "查不到 runner 版本" >&2; exit 1; }
    install -d -o "$RUNNER_USER" -g "$RUNNER_USER" -m 0755 "$RUNNER_DIR"
    tmp="$(mktemp -d)"
    curl -fsSL -o "$tmp/runner.tgz" "https://github.com/actions/runner/releases/download/v$ver/actions-runner-linux-$arch-$ver.tar.gz"
    tar -xzf "$tmp/runner.tgz" -C "$RUNNER_DIR"
    rm -rf "$tmp"
    chown -R "$RUNNER_USER:$RUNNER_USER" "$RUNNER_DIR"
    runuser -u "$RUNNER_USER" -- bash -c "cd '$RUNNER_DIR' && ./config.sh --url '$REPO_URL' --token '$TOKEN' --labels msi-cubi-prod --name msi-cubi-prod --unattended --replace"
    (cd "$RUNNER_DIR" && ./svc.sh install "$RUNNER_USER" && ./svc.sh start)
  fi
  (cd "$RUNNER_DIR" && ./svc.sh status | head -5) || true
fi

if step "5/5 驗證權限" "runner 使用者不能用 docker；包裝腳本要拒絕不在分支上的 SHA"; then
  if runuser -u "$RUNNER_USER" -- docker ps >/dev/null 2>&1; then
    echo "失敗：$RUNNER_USER 可以使用 docker，請檢查群組" >&2
    exit 1
  fi
  echo "ok：$RUNNER_USER 不能使用 docker"
  if runuser -u "$RUNNER_USER" -- sudo -n /usr/local/sbin/socrates-prod-deploy 0000000000000000000000000000000000000000 >/dev/null 2>&1; then
    echo "失敗：包裝腳本接受了不在分支上的 SHA" >&2
    exit 1
  fi
  echo "ok：包裝腳本拒絕不在分支上的 SHA"
fi

echo
echo "完成。到 GitHub Actions 核准「Production deploy (msi_cubi)」就會部署（部署前會自動備份資料庫）。"
echo "部署後第一件事：以管理者登入，到「系統 AI」頁設定模型與 key（舊的 .apikeylist 不再使用）。"
