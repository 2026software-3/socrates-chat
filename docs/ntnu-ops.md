# NTNU 維運說明（正式環境與 staging）

本文件描述目前的部署：主機 `msi_cubi`（Ubuntu、Core Ultra 5 322、14 GB；Intel iGPU＋NPU，沒有 NVIDIA）上的兩套獨立環境，**正式環境**（NTNU 使用）與 **staging**（預覽與測試）。

- 部署相關的版控檔案：`docker-compose.yml`、`docker-compose.intel.yml`、`docker-compose.msi-prod.yml`、`docker-compose.tunnel.yml`（正式環境）、`docker-compose.staging.yml`（staging）、`docker/proxy/`、`scripts/host/`、`.github/workflows/prod-deploy.yml`、`staging-deploy.yml`。
- 秘密只存在主機上，不進版控：正式的 `.env`、staging 的 `.env.staging` 與 `.staging-credentials`。
- 規格上正式環境的部署方式仍未定案（S-09）；本文件是實際做法，不代表規格決策。

## 1. 環境總覽

| 項目 | 正式環境 | staging |
| --- | --- | --- |
| 用途 | NTNU 學生與教師使用，資料要保留 | 預覽與測試；空資料庫加合成資料，不接語音 |
| 公開網址 | `https://se3socrateschat.ntnu.cc` | `https://staging.se3socrateschat.ntnu.cc` |
| compose 專案名稱 | `socrates-ntnu` | `socrates-staging` |
| compose 檔 | `docker-compose.yml` ＋ `docker-compose.intel.yml` ＋ `docker-compose.msi-prod.yml` ＋ `docker-compose.tunnel.yml`，profiles `app cascade proxy tunnel` | `docker-compose.staging.yml`（單獨一檔，不疊在其他檔案上） |
| 服務 | `db`、`app`、`proxy`（Pingap）、`cloudflared`、`cascade`、`cascade-adapter`、`cascade-whisper`、`cascade-tts` | `staging-db`、`staging-app`、`staging-proxy` |
| 資料庫 volume | `socrates-ntnu_pgdata_v2` | `socrates-staging_pgdata_staging` |
| 部署分支 | `deploy/msi-cubi-intel` | `deploy/msi-cubi-staging` |
| 部署目錄（root 擁有） | `/var/lib/socrates-prod` | `/var/lib/socrates-staging` |
| 秘密檔 | `/var/lib/socrates-prod/.env` | `/var/lib/socrates-staging/.env.staging`、`.staging-credentials` |
| GitHub workflow | Production deploy (msi_cubi)，environment `prod-deploy` | Staging deploy (msi_cubi)，environment `staging-deploy` |
| self-hosted runner | 使用者 `gha-runner-prod`，label `msi-cubi-prod` | 使用者 `gha-runner`，label `msi-cubi-staging` |
| 主機埠（只綁本機或 tailnet） | `app` 127.0.0.1:3000；`proxy` 127.0.0.1:3080 與 tailnet 位址（`PROXY_TAILNET_IP`，預設值見 `docker-compose.msi-prod.yml`） | `staging-proxy` 127.0.0.1:3081 |

所有環境共通的規則：

- 部署只由推送到上面兩個 `deploy/*` 分支觸發。其他分支（包括 `deploy/ntnu-integration`）不會部署。
- 不使用 `down -v`，不刪資料庫 volume，不把資料庫改成 bind mount。
- migration 只往前：已合併的 migration 檔不修改、不刪除；改 schema 一律新增 migration。
- 正式環境的資料要保留，不以重建資料庫的方式修正問題。

## 2. 架構與網路

對外路徑（正式環境）：

```
學生瀏覽器 → Cloudflare（自訂主機名稱）→ Cloudflare Tunnel → cloudflared → proxy:80（Pingap）→ app:3000 → db、cascade
                                                                 └─ 後端連不上時回靜態停機說明頁
staging：同一條 tunnel → staging-proxy:80 → staging-app:3000 → staging-db
```

網路：

| 網路 | 專案 | 網段 | 內容 |
| --- | --- | --- | --- |
| `socrates-ntnu_default` | 正式 | compose 自動配置（**不宣告**，見 §6） | `db`、cascade 系列、`app`、`cloudflared`、`staging-proxy`（別名 `staging-proxy`） |
| `socrates-ntnu_edge` | 正式 | `172.29.250.0/24`，可配置範圍 `.128/25` | `app` 固定 `172.29.250.10`；`proxy`、`cloudflared` |
| `socrates-staging_default` | staging | `172.29.251.0/24`，可配置範圍 `.128/25` | `staging-db`、`staging-app` |
| `socrates-staging_edge` | staging | `172.29.252.0/24`，可配置範圍 `.128/25` | `staging-app` 固定 `172.29.252.10`；`staging-proxy` |

- `staging-proxy` 同時加入 `socrates-ntnu_default`，讓正式的 `cloudflared` 連得到它。它不叫 `proxy`，避免 Docker DNS 把正式網路裡的 `app` 解析到 staging。
- 各網段不能重疊。
- `app` 與 `staging-app` 的固定位址讓 Pingap 不會因為 DNS 快取而連到舊 IP（見 §6）。

### 公開網址與 DNS

- 正式網址 `se3socrateschat.ntnu.cc` 在學校 DNS 上設定（名稱相對於 `se3socrateschat.ntnu.cc`，`@` 代表它自己）：CNAME 指到 `socrates.ray-dev.stream`（灰雲）；驗證用的 TXT 名稱為 `_cf-custom-hostname`、`_acme-challenge`，不要再加 `.se3socrateschat`。
- staging 網址：CNAME `staging` → `socrates.ray-dev.stream`（灰雲）；TXT `_cf-custom-hostname.staging`；兩筆 TXT `_acme-challenge.staging`。
- `socrates.ray-dev.stream` 同時是 Cloudflare for SaaS 的 fallback origin，也是備用網址。備用網址只能看頁面，寫入請求會被 CSRF 檢查擋掉。
- 學校帳號下的網域不能直接當 tunnel 主機（`cfargotunnel.com` 只代理同一帳號內的 DNS 記錄），所以用 SaaS 自訂主機名稱，tunnel 與 `ray-dev.stream` 在同一個 Cloudflare 帳號。
- Tunnel 的 ingress 是遠端設定（Cloudflare Zero Trust → Networks → Tunnels）：
  - `se3socrateschat.ntnu.cc`、`socrates.ray-dev.stream` → `http://proxy:80`
  - `staging.se3socrateschat.ntnu.cc` → `http://staging-proxy:80`

### 換公開網址時

CSRF 只認 `APP_BASE_URL` 一個網址，換網址要照順序做：

1. 在 Cloudflare 建好新的主機名稱（自訂主機名稱或同帳號的 DNS 記錄）與 tunnel ingress；先確認新網址的首頁回 200。
2. 改正式 `.env` 的 `APP_PUBLIC_URL`（staging 改 `.env` 的 `STAGING_PUBLIC_URL`），重建對應的 app（見 §4「手動操作」）。不改的話登入與所有寫入都會被 CSRF 擋掉。
3. 到 Google Cloud Console 把 OAuth 的重新導向 URI（`<網址>/api/auth/google/callback`）與授權來源換成新網址。正式與 staging 各有一組。
4. 通知使用者。

## 3. 設定與秘密

正式環境 `.env`（只在主機上）：

- `APP_PUBLIC_URL`：學生實際開啟的網址，容器內變成 `APP_BASE_URL`。
- `TRUSTED_PROXIES=172.16.0.0/12`：必填，沒設 `app` 不會啟動。
- `APP_IMAGE_TAG`、`CASCADE_INTEL_TAG`：映像 tag。自動部署時 `APP_IMAGE_TAG` 由部署腳本以命令列變數覆寫。
- `CLOUDFLARE_TUNNEL_TOKEN`（`cloudflared` 用）、`CASCADE_LLM_TOKEN`（cascade 用）。
- `ADMIN_EMAILS`：啟動時建立管理者帳號的信箱（見 §10）。
- `TEACHER_KEY_ENCRYPTION_KEYS`：加密資料庫裡教師 AI key 的金鑰。**遺失就解不開資料庫裡所有 key**，要另外存一份。
- `DEMO_MODE` 不設定（見 §11）。`SMTP_*`、`COMPLAINT_TO` 見 §9。

staging `.env.staging`（只在主機上，與正式的 `.env` 分開）：

- 不含 SMTP、客訴信箱、語音設定（`CASCADE_*`、STT／TTS）、tunnel token；staging 不寄信、不接語音。
- `TEACHER_KEY_ENCRYPTION_KEYS` 要用 staging 自己的值，不與正式共用。
- AI key 在 staging 的介面（「系統 AI」與教師 key）自己設定。
- `STAGING_APP_IMAGE_TAG`（部署腳本設定）、`STAGING_PUBLIC_URL`（預設 `https://staging.se3socrateschat.ntnu.cc`）、`STAGING_DEMO_MODE`（預設 `false`）由命令列或部署腳本提供。

AI 金鑰：後端不讀 `.apikeylist`，也不讀 `OPENAI_API_KEYS`。系統用途的 AI（個人練習、翻譯、主張分組、公告翻譯等）要先在「系統 AI」設定模型與 key，設定前這些功能不可用；教師 key 與教室 AI 不受影響。

改環境變數後要重建容器才會生效（§4「手動操作」）。

## 4. 部署

### 正式環境（自動部署）

流程：

1. 功能先在 `deploy/msi-cubi-staging` 上部署到 staging 驗證。
2. 開 PR 把 `deploy/msi-cubi-staging` 合併到 `deploy/msi-cubi-intel`。合併（push）後 GitHub Actions「Production deploy (msi_cubi)」開始，等待 environment `prod-deploy` 的人工核准。
3. 核准後，主機上的 runner（`gha-runner-prod`）執行 `sudo -n /usr/local/sbin/socrates-prod-deploy <sha>`。包裝腳本（`scripts/host/socrates-prod-deploy`，root 擁有，不取自被推送的 commit）依序做：
   - 只接受 40 位小寫十六進位 SHA，且必須是 `deploy/msi-cubi-intel` 的目前 head（回滾例外，見下）。
   - 部署前 `pg_dump` 到 `/var/backups/socrates-prod`（保留最近 14 份）。**備份失敗就中止，不動任何容器。**
   - 切到該 commit（`/var/lib/socrates-prod`）；`docker build -t socrates-ntnu-app:prod-<sha12>`。
   - **cascade 映像（§13）**：算出 tag `intel-<最後改到 cascade 輸入檔的 commit 前 12 碼>`，缺的映像先建好（失敗或磁碟可用空間不足 50 GB，在動任何容器之前中止）。要換 cascade 時先等進行中的語音場次結束（讀 cascade 的 `/health`，最多約 10 分鐘），等不到就**延後**：其他服務照常部署，cascade 維持原 tag，下次部署再試。
   - `CASCADE_INTEL_TAG=<實際使用的 tag> APP_IMAGE_TAG=prod-<sha12> docker compose`（四個檔、四個 profile）`up -d --no-build`。`db` 不會被停；cascade 只在上面決定要換時才重建，其他服務只有設定有變的會重建。tag 以環境變數傳入，優先於 `.env`。
   - 健康檢查：經 `proxy` 的 `/ready`，90 秒內要回 200，否則標為失敗。換了 cascade 的話再等它變 healthy（最多 5 分鐘），否則標為失敗。成功後把 `.env` 的 `CASCADE_INTEL_TAG` 同步成實際使用的 tag（手動操作才不會把 cascade 換回舊映像）。
   - 只保留最近 5 個 prod 映像；建過 cascade 映像就清建置快取，每種 cascade 映像保留最近 2 個 tag（正在使用的不刪）。
   - 同一時間只跑一個部署（workflow 的 concurrency 與腳本的 `flock`）；部署中途不取消。

### staging（自動部署）

推送到 `deploy/msi-cubi-staging` 後，GitHub Actions「Staging deploy (msi_cubi)」等待 environment `staging-deploy` 核准，然後 runner（`gha-runner`）執行 `sudo -n /usr/local/sbin/socrates-staging-deploy <sha>`：

- 同正式的檢查（SHA 格式、必須是分支目前的 head、`flock`）。
- 切到該 commit（`/var/lib/socrates-staging`）；`docker build -t socrates-ntnu-app:staging-<sha12>`；`STAGING_APP_IMAGE_TAG=… docker compose -f docker-compose.staging.yml up -d --no-build`。
- 健康檢查：`127.0.0.1:3081/ready` 60 秒內要回 200。失敗時新版容器可能已經取代舊的，看 `docker logs socrates-staging-staging-app-1`，修好後推新 commit 或在 Actions 對該次執行按 Re-run。
- 只保留最近 5 個 staging 映像。staging 部署不做資料庫備份（資料是合成資料）。

### 回滾

- 正式：`sudo PROD_ALLOW_ROLLBACK=1 /usr/local/sbin/socrates-prod-deploy <祖先 sha>`。
- staging：`sudo STAGING_ALLOW_ROLLBACK=1 /usr/local/sbin/socrates-staging-deploy <祖先 sha>`。
- sudoers 不保留環境變數，runner 設不了這兩個變數，所以回滾只能由 root 手動做。
- **migration 只往前。** 回滾程式前先看 `git diff --stat <舊 sha> <新 sha> -- backend/migrations`：沒有輸出，舊版程式通常能用；有 migration，舊程式可能跟新 schema 不相容，要另外評估。還原資料庫只能用部署前的備份，但會讓備份之後的資料一起消失，不要為了回滾程式而還原。

### 信任邊界與 GitHub 設定

- 能 push 到部署分支的人，仍可以讓任意 app 原始碼與 Dockerfile 在主機上被建置並執行，等同 root。包裝腳本與 sudoers 只限制 runner 的權限，**真正的防線是下面三項**：
  1. 分支保護（ruleset「Protect deploy/msi-cubi-staging」與對應的 intel 分支規則）：只能經 PR，禁止刪除與 force push。
  2. environment `prod-deploy`、`staging-deploy`：Required reviewers；deployment branch 限制為對應的部署分支。
  3. 兩個 deploy workflow 只接 `push`，不要加 `pull_request`（公開 repo，fork PR 不能觸發 self-hosted runner）。
- prod runner 只該跑 `prod-deploy.yml`。runner 上線後不要以 root 執行 runner 目錄裡的腳本（`svc.sh` 等由 runner 使用者擁有），改用 `systemctl`。

### 主機的一次性設定（首次設定或重建主機時）

- 正式：在 msi_cubi 執行 `scripts/host/setup-prod-autodeploy.sh`（互動式，檔頭有用法）。它建立 `/var/lib/socrates-prod` 與 `/var/backups/socrates-prod`、複製 `.env`、安裝包裝腳本與 sudoers、建立並啟動 runner。
- staging（root）：
  1. `git clone -b deploy/msi-cubi-staging <repo> /var/lib/socrates-staging`，再把 `.env.staging`、`.staging-credentials` 以 `install -o root -g root -m 0600` 放進去。
  2. `install -o root -g root -m 0755 scripts/host/socrates-staging-deploy /usr/local/sbin/socrates-staging-deploy`
  3. `visudo -cf scripts/host/gha-runner-staging.sudoers`，通過後 `install -o root -g root -m 0440 scripts/host/gha-runner-staging.sudoers /etc/sudoers.d/gha-runner-staging`。
  4. `gpasswd -d gha-runner docker`，再 `systemctl restart "actions.runner.*msi-cubi-staging*"`。
  5. 驗證：`runuser -u gha-runner -- docker ps` 應該失敗；`runuser -u gha-runner -- sudo -n /usr/local/sbin/socrates-staging-deploy 0000000000000000000000000000000000000000` 應該被拒絕。
- 包裝腳本與 sudoers **不會隨 commit 自動更新**。改了 `scripts/host/` 的檔案後，要由 root 重新安裝到 `/usr/local/sbin/` 與 `/etc/sudoers.d/`。

## 5. 手動操作

日常一律走自動部署。手動操作只用在改環境變數、單獨重啟容器、或緊急情況。`ray520` 在 `docker` 群組，能用 docker 等同 root。

### 讓管理者直接改環境變數檔

部署目錄與秘密檔預設是 root 擁有、0600。只動秘密檔的群組與權限，不用 `chgrp -R`，目錄的擁有者維持 root（部署腳本的 `git reset --hard` 不受影響）。這兩行只需要執行一次，檔案被刪除重建後要重做：

```bash
# staging
sudo chgrp docker /var/lib/socrates-staging/.env.staging /var/lib/socrates-staging/.staging-credentials
sudo chmod 660   /var/lib/socrates-staging/.env.staging /var/lib/socrates-staging/.staging-credentials

# 正式（只有 .env 一個秘密檔）
sudo chgrp docker /var/lib/socrates-prod/.env
sudo chmod 660   /var/lib/socrates-prod/.env
```

**目前狀態（2026-11-22 唯讀查核，msi_cubi）：**

- staging：`.env.staging` 與 `.staging-credentials` 已是 `root:docker` 660，ray520 可直接編輯。
- 正式：`/var/lib/socrates-prod/.env` 仍是 `root:root` 600，上面的 `chgrp`／`chmod` **尚未套用到正式環境**；ray520 改正式的 `.env` 仍需 sudo。
- 目錄：`/var/lib/socrates-prod` 是 `root:root` 755。`/var/lib/socrates-staging` 是 `root:docker` 750，群組不是 root，與上面「目錄擁有者維持 root」的說明只有擁有者一致。群組的 `docker` 只讓 ray520 能進入與列出目錄；改回 root 群組是主機上的操作，尚未決定。目錄權限不影響原地編輯既有的 660 檔案。

- 權限不要比 660 更寬。自動部署的 runner（`gha-runner`、`gha-runner-prod`）不在 `docker` 群組，讀不到這些檔案。
- 編輯器要「原地寫入」：目錄是 root 的，會先寫暫存檔再改名的編輯器會失敗；`nano`、`vim`（預設設定）都可以。
- 改之前先備份 `.env` 到安全的地方（內含資料庫密碼與 `TEACHER_KEY_ENCRYPTION_KEYS`）。

### 重建容器讓新設定生效

只重建會用到該檔的服務。正式環境的 `cascade` 載入模型要數分鐘，不要讓它一起重建（自動部署會自己決定要不要換 cascade，見 §4）。

```bash
# staging：staging-app 讀 .env.staging
cd /var/lib/socrates-staging
STAGING_APP_IMAGE_TAG=$(docker inspect -f '{{.Config.Image}}' socrates-staging-staging-app-1 | cut -d: -f2) \
  docker compose -f docker-compose.staging.yml up -d --no-build --no-deps --force-recreate staging-app

# 正式：app 讀 .env
cd /var/lib/socrates-prod
F="-f docker-compose.yml -f docker-compose.intel.yml -f docker-compose.msi-prod.yml -f docker-compose.tunnel.yml"
APP_IMAGE_TAG=$(docker inspect -f '{{.Config.Image}}' socrates-ntnu-app-1 | cut -d: -f2) \
  docker compose $F --profile app --profile cascade --profile proxy --profile tunnel up -d --no-build --no-deps --force-recreate app
```

- 重建 `app` 會中斷服務數秒，正式環境挑沒人使用的時段並先告知使用者。
- 改的若是 cascade 或 tunnel 讀的變數，要重建的是對應的服務，不是 `app`。

### 正式環境的 compose 狀態

```bash
cd /var/lib/socrates-prod
F="-f docker-compose.yml -f docker-compose.intel.yml -f docker-compose.msi-prod.yml -f docker-compose.tunnel.yml"
docker compose $F --profile app --profile cascade --profile proxy --profile tunnel ps
```

`~/socrates-intel` 是舊的手動部署目錄，不再用來啟動。不要在那裡執行 compose，會與正式專案搶同一組容器與 volume。

## 6. 反向代理（Pingap）與停機說明頁

- 映像 `vicanso/pingap:0.16.2`（版本固定）。設定：正式 `docker/proxy/pingap.toml`，staging `docker/proxy/pingap.staging.toml`；停機頁 `docker/proxy/down.html`。
- 路徑：`cloudflared` → `proxy:80` → `app:3000`。`proxy` 不 `depends_on` `app`，並設 `restart: unless-stopped`：後端停著時代理仍然活著。
- `/proxy-health` 只表示代理存活，與後端無關。
- 後端連不上（容器停掉、重建中、主機剛開機）時，代理回三語靜態說明頁「系統維護中，請稍後再試」（不含時間與原因）：穩定時 HTTP 503；剛停掉的約 22 秒內是 504。每個請求在轉為停機頁前會先等最多 5 秒。
- 只有代理自己產生的錯誤（502、503、504）會換成停機頁；後端自己回的任何狀態（包括維護模式的 503 與後端的 502、504）原樣轉發。
- 存取日誌只記來源位址、方法、路徑（不含查詢字串）、狀態、大小與耗時，不含 Cookie、Referer、User-Agent。**例外**：代理自己產生錯誤時，錯誤日誌會記該請求的 User-Agent（需求方已確認可以記錄，見 `docs/specs/decisions.md` S-16）。
- 語音 WebSocket 的讀寫逾時為 3600 秒，一般請求為 600 秒。AI 回覆的產生時間上限遠小於此（`AI_TOTAL_TIMEOUT_SECS` 預設 60 秒）；摘要論證樹在背景產生，有自己的上限（`ARGUMENT_TREE_TIMEOUT_SECS` 預設 300 秒，見 `docs/specs/summary-argument-tree.md`）。

### 固定位址與不要重建 default 網路

- `app` 與 `staging-app` 用 `ipv4_address` 固定在各自的 edge 網路上。Pingap 沿用 DNS 記錄的 TTL（Docker 內建 DNS 是 600 秒），app 換 IP 後代理要數分鐘才跟上；固定位址後重建 `app` 不影響代理。其他容器只從 `ip_range` 配置，不會佔用它。
- **不要在 `docker-compose.yml` 對 `default` 網路加 `ipam`、`labels`、`driver_opts`。** 任何一項都會讓 compose 判定網路設定有變而重建它，會發生三件事：
  1. 重建網路要先停掉所有接在上面的容器，整個正式環境停止再啟動。
  2. 接在網路上的外部容器（`staging-proxy`）會擋住網路移除，正式環境整個停著。
  3. 設定沒變的容器只是停了再啟，會失去服務別名（`db`、`cascade`…），沒有重建的 app 還會失去固定位址。
- staging 的 `default` 網路也一樣：`docker-compose.staging.yml` 裡它的標籤與網段是已套用的設定，移除任何一項都會讓 compose 重建它，並停掉 `staging-db`。
- 若真的必須重建 `socrates-ntnu_default`，先拆掉接在上面的外部容器（`docker network disconnect`），讓 db 與 cascade 在同一次部署被重建（別名才會正確），並準備好失敗時的 `docker compose ... rm -f`（**不加 `-v`**）。

### 改代理設定時

1. 在任何有 Docker 的機器上跑 `scripts/verify-proxy.sh`（用假後端，不碰正式資料）。它檢查：轉發、`X-Forwarded-For`、後端自己的 503 原樣轉發、SSE 逐段送達、WebSocket 升級、大請求、停機頁，以及後端停掉後的恢復。CI 的 `Proxy` workflow 在相關檔案變動時也會跑。
2. SSE 與語音要另外用真的後端確認：開一場對話，回覆要逐段出現；試一次語音。
3. 升級 Pingap 版本前同樣要跑 `scripts/verify-proxy.sh`。
4. 只重啟代理：`docker compose $F --profile proxy restart proxy`（正式）。

### 回退

- 代理行為不對、其他正常：不改容器，把 Cloudflare Zero Trust 的 tunnel ingress 從 `http://proxy:80` 改成 `http://app:3000`。這時沒有停機頁，`TRUSTED_PROXIES` 仍包含 cloudflared 所在的網段，來源 IP 照常。staging 同理，改 `staging-proxy` 的 ingress。
- 確認停機頁（選擇性）：先告知使用者（會中斷服務），`docker stop socrates-ntnu-app-1`，公開網址應出現停機頁，之後 `docker start socrates-ntnu-app-1`。`app` 重啟後位址要仍是 `172.29.250.10`。

## 7. 來源 IP（TRUSTED_PROXIES）

登入限流以來源 IP 為鍵。`TRUSTED_PROXIES=172.16.0.0/12`（compose 網段：`proxy` 與 `cloudflared`）讓後端由右往左跳過受信任代理，取得 Cloudflare 帶來的真實來源。不要放整段 `100.64.0.0/10`，否則 tailnet 內任何裝置都能偽造來源。Pingap 只採用私有網段來的 `X-Forwarded-For`。

改變入口後要重驗：

1. 用手機在行動網路（關 Wi-Fi）先查自己的 IP，馬上開公開網址，故意輸入錯誤密碼登入一次。
2. 查 `login_failures`：

   ```bash
   docker exec socrates-ntnu-db-1 psql -U socrates -d socrates -c "select left(login_key,8), ip, failures, last_failed_at from login_failures order by last_failed_at"
   ```

3. `ip` 應該是手機的公網位址，不是 `172.x`（Docker 網段）。IPv6 來源會合併成 `/64` 網段，所以只會看到前四段，例如 `2a09:bac3:5808:323::/64`。行動網路的位址會變動，查 IP 與登入要在同一分鐘內做。
4. 測完刪掉測試紀錄（`login_key` 若是真實帳號輸入也要刪，避免殘留失敗次數）：`delete from login_failures where login_key like '<前 8 碼>%'`。

## 8. 資料庫

### 位置與備份

- 正式資料庫放在命名 volume `socrates-ntnu_pgdata_v2`（`docker-compose.msi-prod.yml` 以 `!override` 取代 `docker-compose.yml` 的 `pgdata`）。staging 資料庫在 `socrates-staging_pgdata_staging`。
- 不要改成 bind mount 到可能沒掛上的主機路徑：沒掛上時 Docker 不會報錯，而是悄悄建一個空目錄，Postgres 就會初始化一個全新的資料庫。
- volume 不是備份。備份分兩種：
  - 正式部署前自動備份：`/var/backups/socrates-prod/socrates-predeploy-*.sql.gz`（保留 14 份）。此目錄是 root 擁有，ray520 不能直接列出（唯讀查核時 `ls` 回 Permission denied），要用 `sudo ls -lt /var/backups/socrates-prod | head` 檢查。
  - 手動備份（只讀，不影響執行中的資料庫）：

    ```bash
    mkdir -p ~/ntnu-backups && (umask 077; docker exec socrates-ntnu-db-1 pg_dump -U socrates -d socrates --no-owner | gzip > ~/ntnu-backups/socrates-<名稱>-$(date +%Y%m%d-%H%M%S).sql.gz)
    ```

  - 備份含真實使用者資料：不進版控、權限限本人、用完要刪除（`shred -u`）。每日加密備份與還原演練是否適用 NTNU 尚待決定（S-09.3 第 3 節）。
- 保留的舊資料（勿刪，作為退路）：`socrates-ntnu_pgdata`（舊的正式 volume）。`socrates-intel-test_pgdata` 是瀏覽器測試資料，與正式資料無關；`socrates-intel-test_cascade_hf`、`socrates-intel-test_cascade_cache` 是正式 cascade 也在用的模型與編譯快取（由 `docker-compose.msi-prod.yml` 以 `name:` 指定），不要刪。

### 重啟後數資料

主機或 Docker 重啟後，除了 `/health`、`/ready`，還要數一下資料：

```bash
docker exec socrates-ntnu-db-1 psql -U socrates -d socrates -At -c "select count(*) from users"
```

和上次記錄的數字比對。資料庫被重新初始化時，`/health` 與 `/ready` 仍會回 200。

### 資料庫被重新初始化時的還原

1. 先確認真的是空的：`users`、`conversations` 筆數，`pg_postmaster_start_time()` 是不是剛剛，`_sqlx_migrations` 是不是從頭套用。
2. 不要再重啟 `db`。用最新的 `pg_dump` 備份還原；備份是 `--no-owner` 的純文字（gzip），含 `_sqlx_migrations`，還原進**空的**資料庫即可：
   - `docker stop socrates-ntnu-app-1`；移除壞掉的 `db` 容器（`docker compose $F rm -sf db`，不加 `-v`）。
   - 把 `docker-compose.msi-prod.yml` 的 `db.volumes` 改成**新的**命名 volume（例如 `pgdata_v3`，並在底部宣告），`docker compose $F up -d --no-deps db`，等 healthy，確認 `information_schema.tables` 為 0。
   - `gunzip -c <備份>.sql.gz | docker exec -i socrates-ntnu-db-1 psql -U socrates -d socrates -v ON_ERROR_STOP=1 -q`。
   - `docker compose $F --profile app up -d --no-deps --no-build app`（啟動時會套用備份之後新增的 migration），核對 `users`、`conversations`、`audit_logs`、`_sqlx_migrations` 與 `/health`、`/ready`。
3. 還原不回來的是「備份之後到事故之間」的資料，所以備份頻率決定損失，部署前與每次重啟後都要先有一份。

### 刪除資料庫的限制

- 不要對 `socrates-ntnu` 執行 `down -v`，除非試用負責人明確同意刪除資料：它會刪掉專案的命名 volume，包含 `socrates-ntnu_pgdata_v2`。
- 刪除前先告知試用負責人，並留下 `_sqlx_migrations` 與 `users` 的摘要以便核對。

### migration 的執行方式

- 目前正式與 staging 的 `app` 都設 `RUN_MIGRATIONS=true`，由後端啟動時套用 migration。這與 S-09.3 的規格不同（規格要求部署前單獨執行 `sqlx migrate run`），維持現狀，直到維運負責人決定是否改變（見 §13）。
- 部署前一定要有備份（自動部署已內建；手動部署要自己做）。
- `sqlx::migrate!()` 在編譯時才嵌入 migration 檔；新增或還原 migration 後，要 touch `backend/src/main.rs` 強制重編，否則會跑到舊的 migration 內容。
- 日誌出現 `failed to run migrations` 時，表示資料庫的 migration 紀錄與程式不一致。先比對 `_sqlx_migrations` 的 checksum，不要直接 `down -v`。
- 不要修改已套用的 migration 檔；要修正 schema，新增一個 migration。
- migration 有變動時，先在還原的備份上演練：另起臨時 PostgreSQL（不要用正式的 db，也不要共用它的 volume），還原備份，用新版後端加上 `RUN_MIGRATIONS=true` 啟動（`LISTEN_ADDR` 改到別的連接埠），確認沒有 `VersionMismatch`，並檢查資料搬移結果。用完以 `docker rm -fv` 移除，連同 volume。

## 8.5 語音節點與內部端點隔離（S-19）

伺服器端語音可以有多個節點（cascade 整組所在的設備，硬體不限）。`CASCADE_REALTIME_URL`／`CASCADE_LLM_TOKEN` 是固定存在的「環境節點」（`CASCADE_MAX_SESSIONS` 選填，預設不限）；其他節點由管理者以 API 新增，**不必重啟後端**：

- `GET/POST /api/admin/voice-nodes`、`PATCH/DELETE /api/admin/voice-nodes/{id}`、`POST /api/admin/voice-nodes/{id}/rotate-token`（詳見 `/api/docs`）。新增與輪換的回應**只此一次**含節點密鑰，之後看不到，請當場存進新節點的 `CASCADE_LLM_TOKEN`。
- 新節點的 cascade 要滿足 S-19 第 2 節的契約：`/v1/realtime`、同主機同埠的 `/health`、`sglang-omni==0.2.0` 並套用 `patch_dialog.py`；`CASCADE_LLM_TOKEN` 用上面那把，`CASCADE_LLM_BASE_URL`（`docker/cascade/entrypoint.sh` 會代入 `launch.yaml`）指向後端的**內部端點位址**。
- 下線用 `status: "draining"`（不接新場次、進行中的繼續到結束）；節點遭入侵或必須立刻切斷用 `"disabled"` 或刪除（密鑰立即失效，進行中的場次約 2.5 秒內結束）。
- 學生的音訊走 `ws://` 是明文：遠端節點的 `realtime_url` 必須在私網（tailnet 之類的加密網路）內，否則用 `wss://`。後端不強制，由部署方負責。

### 讓內部端點只在私網（`INTERNAL_LISTEN_ADDR`）

現況下 `/internal/cascade/*` 與公開路由同埠，funnel 直接指向該埠，因此可從網際網路打到，只靠 Bearer。遠端節點要回呼它，所以要把它搬到獨立的埠。**下面兩件事必須在同一次部署一起做**，只做第一件會讓語音整個壞掉：

1. 後端環境設 `INTERNAL_LISTEN_ADDR=0.0.0.0:3001`。公開埠的 `/internal/cascade/*` 之後回 404。
2. 本機 cascade 的回呼改指向內部埠：`CASCADE_LLM_BASE_URL=http://app:3001/internal/cascade/v1`，並重建 `cascade` 容器。`docker-compose.ntnu.yml` 不進版控，**不要**把 3001 發佈到主機的公開介面；只在 compose 網路內讓 cascade 連，遠端節點要連時用 `tailscale serve`（不是 funnel）把它開在 tailnet。

啟用後確認：公開網址 `curl -i https://<公開網址>/internal/cascade/v1/models` 回 404；`docker logs socrates-ntnu-cascade-1` 沒有重啟迴圈。這條路徑尚未在 NTNU 實測。

## 9. 客訴通知信（SMTP）

客訴表單（`/complaint`）與管理者的「客訴」頁（`/admin/complaints`）不需要設定就能用；沒設定 SMTP 時客訴照常存資料庫，寄信狀態顯示「未寄信（尚未設定 SMTP）」。要讓管理者收到通知信，在正式的 `.env` 加上（變數說明見 `.env.example`）：

```
SMTP_HOST=...
SMTP_FROM=Socrates <noreply@example.edu>
COMPLAINT_TO=admin1@example.edu,admin2@example.edu
SMTP_USERNAME=...
SMTP_PASSWORD=...
```

- `SMTP_HOST`、`SMTP_FROM`、`COMPLAINT_TO` 缺任何一項就視為沒啟用；格式錯誤時後端啟動會失敗並指出是哪個變數（不印出位址或密碼）。
- 預設用 STARTTLS（埠 587）；Gmail 這類服務要用應用程式密碼。`SMTP_SECURITY=tls` 走埠 465；`none` 只給本機測試。
- 改 `.env` 後要重建 `app`（`--force-recreate`）才會生效。
- 寄信失敗不影響提交者。管理者在「客訴」頁看到「寄信失敗」、一直停在「寄送中」（寄送途中後端重啟）或「未寄信」（設定 SMTP 之前送出的）時，可按「重寄通知信」；已寄出的沒有這個按鈕。注意：「寄送中」其實仍在寄的時候按重寄可能寄出兩封。
- 日誌只會有客訴編號與失敗類別（`build`／`timeout`／`transport`）。要查原因請直接測 SMTP 連線。
- 信件不含學號、電子郵件與對話原文；描述的原文只在信件內文與管理者的「客訴」頁。

## 10. 管理者帳號與密碼

啟動時，`.env` 的 `ADMIN_EMAILS` 指定的信箱會建立管理者帳號，初始密碼來自 `ADMIN_INITIAL_PASSWORD`，且首次登入必須改密碼。

- 正式的 `.env` 不設定 `ADMIN_INITIAL_PASSWORD`。它只在建立新帳號時使用；啟動時不會重設既有帳號的密碼。
- 以初始密碼建立的管理者帳號 `must_change_password = true`，要由管理者登入後改密碼。

### 更改自己的密碼（已登入）

- 用臨時密碼登入後，網頁會要求先更改密碼。之後所有人都可點右上角的頭像進入「帳號」頁（`/account`）的「更改密碼」自行更改。沒有內建密碼的教師與管理者，頭像沒有連結，但帳號頁仍有更改密碼的按鈕（依帳號類型顯示）。示範帳號不能改密碼。目前密碼連續輸錯 5 次會鎖定 15 分鐘（S-18）。
- API：`POST /api/auth/change-password`，body 為 `current_password` 與 `new_password`。新密碼長度 8 到 128 個字元，不能與舊密碼相同。成功回 204，同一帳號的其他 session 會失效。

### 重設其他使用者的密碼（忘記密碼）

- 由另一位管理者在後台「管理者管理」頁（`/admin/admins`）點該帳號的鑰匙圖示，或呼叫 `POST /api/admin/users/reset-password`，body 為 `{"email": "..."}`。新增管理者也在同一頁（`POST /api/admin/admins`）。環境變數 `ADMIN_EMAILS` 的第一個管理者帳號受保護，密碼只能由本人重設（其他管理者無法重設）。
- 回應中的 `temporary_password` 只會出現這一次，要交給本人，不要寫進日誌、聊天或 issue。
- 該使用者的所有 session 會立即失效，登入後必須改密碼。
- 這個呼叫同樣需要 `Origin` 標頭等於公開網址。

### 沒有其他管理者可以重設時

1. 優先：如果管理者有使用同一信箱的 Google 帳號，可以用 Google 登入，會綁定到同一個帳號（不受改密碼限制）。
2. 如果上一步不可行，`ADMIN_INITIAL_PASSWORD` 無法重設既有帳號。只能直接修改資料庫中的密碼雜湊，必須先與試用負責人確認，並使用 Argon2id 產生 PHC 格式的雜湊。

## 11. 示範模式

正式環境**沒有開** `DEMO_MODE`（`.env` 沒有這一項）。需要發表時可以在 `.env` 加 `DEMO_MODE=true` 並重建 `app`，但注意正式環境是**公開網址**：

- 示範教師與學生的密碼寫在原始碼裡、登入頁也會列出來，任何人都能用它們登入，看到所有示範資料。示範帳號不含管理者。以 `DEMO_MODE=true` 啟動會把既有的示範帳號重新啟用並重設成公開密碼。
- 發表完成後把 `DEMO_MODE` 拿掉並重建 `app`：端點會回 404、登入頁不再列出帳號，但示範帳號仍帶著公開密碼，要在管理者介面停用或重設密碼。
- 目前正式資料庫的處置：10 位示範學生已停用；示範教師 `demo.teacher@example.com` 維持啟用（顯示名稱「測試用教師」），密碼已由管理者重設為非公開值。
- `scripts/seed-demo.sh` 跟不上目前的資料表（活動改為多題目後沒有 `topic_id` 欄位，第二段會中止），正式環境不要執行。staging 用 `scripts/seed-staging.sh`（見 §12）。

## 12. staging 環境

只有 app、db、proxy，**不接語音**（沒有 cascade、Whisper、TTS），空資料庫加合成資料。網址與作法與正式相同：Cloudflare for SaaS 自訂主機名稱加學校 DNS，進**同一條** tunnel（見 §2）。記憶體上限：`staging-app` 768 MB、`staging-db` 512 MB、`staging-proxy` 64 MB。

- 服務名是 `staging-app`、`staging-db`、`staging-proxy`，不叫 `app`、`db`、`proxy`（理由見 §2）。
- 正式環境沒在跑時，`staging-proxy` 起不來（它要接的網路 `socrates-ntnu_default` 不存在）。
- Google 登入要在 Google Cloud Console 加 `https://staging.se3socrateschat.ntnu.cc/api/auth/google/callback`。

### 合成資料（首次建立或重置）

以 root 在 `/var/lib/socrates-staging` 執行，並帶上 `STAGING_APP_IMAGE_TAG`：

```bash
sudo -H bash -c 'cd /var/lib/socrates-staging && export STAGING_APP_IMAGE_TAG=staging-$(git rev-parse --short=12 HEAD) && docker compose -f docker-compose.staging.yml up -d --no-build'
```

1. **公開網址已上線時，先停代理**：`sudo -H bash -c 'cd /var/lib/socrates-staging && export STAGING_APP_IMAGE_TAG=staging-$(git rev-parse --short=12 HEAD) && docker compose -f docker-compose.staging.yml stop staging-proxy'`，做完第 4 步。DEMO_MODE 開著時登入頁會列出公開的示範帳號與密碼，不能在公網可達時開。
2. 開 DEMO_MODE 讓後端建立示範帳號：`sudo -H bash -c 'cd /var/lib/socrates-staging && export STAGING_APP_IMAGE_TAG=staging-$(git rev-parse --short=12 HEAD) && STAGING_DEMO_MODE=true docker compose -f docker-compose.staging.yml up -d --no-build'`。
3. 匯入合成資料（可重複執行；不需要管理者密碼）：`DEMO_PSQL='docker exec -i socrates-staging-staging-db-1 psql -U socrates -d socrates' ./scripts/seed-staging.sh`。不用 `seed-demo.sh`，它是舊 schema。
4. **馬上把示範帳號密碼換成私有的**（`UPDATE users SET password_hash = <argon2id> WHERE student_id LIKE '115000%' OR email = 'demo.teacher@example.com'`），再以同樣的方式、不帶 `STAGING_DEMO_MODE` 執行 `up -d` 關掉 DEMO_MODE（`up -d` 會一併啟動 `staging-proxy`）。staging 在公網，公開的示範密碼不能留著。

### 注意

- `/var/lib/socrates-staging` 會被部署腳本 `git reset --hard`，**不要在那裡手動改程式**。
- 改 `.env.staging` 後，用 §5 的指令加 `--force-recreate staging-app` 重建。
- 不要用 `down -v`。

## 13. 語音（cascade，Intel 版）

正式環境的語音 cascade 用 `docker-compose.intel.yml` 疊在 `docker-compose.yml` 上（msi_cubi 沒有 NVIDIA）。映像 tag 由 `.env` 的 `CASCADE_INTEL_TAG` 指定，映像與 NVIDIA 版的 `socrates-cascade:latest` 分開。

| 服務 | 裝置 | 映像 |
| --- | --- | --- |
| `cascade-whisper` | NPU（OpenVINO GenAI，模型 `OpenVINO/whisper-small-int8-ov`；`WHISPER_OV_DEVICE` 預設 `NPU`） | `socrates-cascade-whisper-npu` |
| `cascade-tts` | iGPU（PyTorch XPU，Kokoro，`KOKORO_DEVICE=xpu`） | `socrates-cascade-tts-xpu` |
| `cascade`、`cascade-adapter` | CPU | `socrates-cascade-core` |

- 主機需求：核心已載入 `xe`（iGPU）與 `intel_vpu`（NPU），`/dev/dri/renderD128` 與 `/dev/accel/accel0` 存在；容器內的使用者要能存取這兩個裝置，`RENDER_GID` 填主機 render 群組的 gid（`getent group render`，msi_cubi 是 991）。
- 建置前提：NPU 的使用者空間驅動不在 Ubuntu 倉庫。要把 Intel linux-npu-driver 發行檔的 `intel-level-zero-npu`、`intel-driver-compiler-npu` 與 PPA 的 `libze1` 三個 `.deb`（版本與主機一致，msi_cubi 放在 `~/npu_driver`）放進 `docker/cascade-intel/npu-debs/`（已列入 `.gitignore`）。
- 建置與啟動：

  ```bash
  docker compose -f docker-compose.yml -f docker-compose.intel.yml --profile cascade build
  ```

  正式環境的啟動、重建方式見 §5（四個檔、四個 profile）。
- 換版：正式環境由自動部署處理（§4，tag 與判斷規則見該節），不需要手動改 `CASCADE_INTEL_TAG`。手動換版才改 `.env` 的 `CASCADE_INTEL_TAG`，再重建 cascade 服務（不要重建 `app`）。
- 自動建映像的限制：
  - `docker/cascade-intel/npu-debs/` 在部署目錄（`/var/lib/socrates-prod`）裡沒有 `.deb`，所以自動部署**不會重建 `cascade-whisper` 映像**，沿用現有映像並印警告；whisper 的變更要由 root 把 `.deb` 放進去後手動建置。
    - 放進去的是三個 `.deb`：`intel-level-zero-npu`、`intel-driver-compiler-npu`、`libze1`（msi_cubi 在 `~/npu_driver`，合計約 49 MB）。`intel-fw-npu` 是核心用的韌體，只裝在主機，**不要**放進這個目錄（`COPY *.deb` 會把它一起裝進映像）。
    - **不能用 symlink。** docker build 的 `COPY` 不跟隨指向 build context 外面的 symlink，會直接失敗（`"/debs/a.deb": not found`，2026-11-27 在 msi_cubi 以 docker 29.1.3 實測）。同一個檔案系統上的**硬連結**可以（`/home/ray520` 與 `/var/lib/socrates-prod` 在同一個裝置），複製也可以。
    - 放法（root）：`cd /var/lib/socrates-prod/docker/cascade-intel/npu-debs && ln ~ray520/npu_driver/{intel-level-zero-npu,intel-driver-compiler-npu,libze1}_*.deb .`（`ln` 失敗就改 `cp`）。`.deb` 被 `.gitignore` 忽略，部署時的 `git reset --hard` 不會刪它們。放好之後，下一次 whisper 輸入檔有變動，自動部署就會重建 whisper 映像。
  - 建置沒有快取，冷建一組約 30 分鐘以上（`socrates-cascade-core` 實測 21 分鐘），所以 workflow 的 `timeout-minutes` 是 120。建完會清建置快取（約 30 GB），避免寫滿磁碟；磁碟可用空間不足 50 GB 時不建。
  - 換 cascade 時，`cascade`、`cascade-adapter`、`cascade-tts`、`cascade-whisper` 會一起重建（tag 變了），語音中斷約 4 分鐘（`cascade-tts` 載入模型的 `start_period` 是 240 秒），所以腳本會先等沒有場次。
  - 包裝腳本要由 root 重新安裝才會生效（§4「主機的一次性設定」）。第一次安裝前，先對現有三個映像補上輸入標籤，避免自動部署把已經是新的映像再建一次：

    ```bash
    cd /var/lib/socrates-prod   # root；對每個映像，用該映像的輸入檔最後一次 commit 當標籤
    c() { git log -1 --format=%H -- "$@"; }
    T=intel-$(c docker/cascade docker/cascade-intel scripts/whisper_text.py scripts/whisper_echo.py | cut -c1-12)
    label() { # label <repo> <現行 tag> <輸入檔...>
      local repo=$1 cur=$2; shift 2
      printf 'FROM %s:%s\nLABEL socrates.inputs=%s\n' "$repo" "$cur" "$(c "$@")" | docker build -q -t "$repo:$T" -
    }
    label socrates-cascade-core        "$(sed -n 's/^CASCADE_INTEL_TAG=//p' .env)" docker/cascade/tts_adapter.py docker/cascade/patch_dialog.py docker/cascade/launch.yaml docker/cascade/entrypoint.sh docker/cascade-intel/core.Dockerfile
    label socrates-cascade-tts-xpu     "$(sed -n 's/^CASCADE_INTEL_TAG=//p' .env)" docker/cascade/kokoro_server.py docker/cascade-intel/tts.Dockerfile
    label socrates-cascade-whisper-npu "$(sed -n 's/^CASCADE_INTEL_TAG=//p' .env)" docker/cascade-intel/whisper.Dockerfile docker/cascade-intel/whisper_ov_server.py docker/cascade-intel/npu-debs scripts/whisper_text.py scripts/whisper_echo.py
    ```

    注意：自動建映像永遠從部署分支（`deploy/msi-cubi-intel`）的檔案建，不是從 main。`launch.yaml` 的 B（`vad`、`turn_detection` 併進 cascade 程序，`docs/specs/cascade-slimming.md`）若還沒合進部署分支，重建出來的映像就沒有 B。2026-11-27 起正式環境跑的是手動補上 B 的 `intel-20261127`；補標籤等於讓腳本把它當成目前部署分支的映像，下次 cascade 輸入檔有變動、重建時就會用部署分支的檔案，所以要先把 main 的 B 帶進 `deploy/msi-cubi-staging` 與 `deploy/msi-cubi-intel`，否則重建會失去 B。
- `cascade-tts` 的第一次啟動要載入模型並編譯 SYCL 核心，healthcheck 的 `start_period` 是 240 秒。這段期間 healthcheck 未過屬正常。

已知差異與風險：

- openvino-genai 不回 `no_speech_prob`、`avg_logprob`，沒有 faster-whisper 版的信心過濾；改靠能量門檻加 Silero VAD。真人、多口音、雜訊環境的辨識品質沒有用真實語料比過 faster-whisper，只用合成語音驗證了鏈路與噪音。
- 取不到功耗（RAPL、hwmon 要 root）；省電只以 CPU 秒數佐證。
- Kokoro 遇到新長度的句子首次會多 0.1 到 0.7 秒（SYCL 核心編譯，`SYCL_CACHE_PERSISTENT=1`，快取在 `cascade_cache` volume）。
- cascade 日誌有 mooncake、nixl 的 ERROR：可選的傳輸層沒裝，不影響功能。

## 14. 瀏覽器測試堆疊（msi_cubi，空資料庫）

`docker/cascade-intel/browser-test/compose.yml` 疊在 §13 的兩個檔案之上，專案名稱 `socrates-intel-test`（獨立 volume 與網路）。平常沒有在跑。設定範本是 `browser-test/env.example`；AI key 不放檔案，堆疊起來後在「系統 AI」頁設定。

- `DEMO_MODE=true`：登入頁列出公開的示範帳號，所以**只能透過 `tailscale serve`（tailnet 內）提供，不可開 funnel，不可對公網開放**。
- 起動：`docker compose -p socrates-intel-test -f docker-compose.yml -f docker-compose.intel.yml -f docker/cascade-intel/browser-test/compose.yml --profile app --profile cascade up -d --no-build`。
- cascade 啟動時會先探測 app 的內部 LLM 端點，app 還沒起來它會乾淨退出並被 `restart: unless-stopped` 反覆重啟；app 起來後即穩定。
- `scripts/seed-demo.sh` 的第二段已跟不上目前資料表，第一段（教室、名單、題目）可用；活動改用 API 建（示範教師登入 → `POST /api/activities` → `/publish`）。

## 15. 恢復檢查

所有服務都設 `restart: unless-stopped`，主機開機後 Docker 會自己拉起來。重啟後確認：

- 正式：`docker compose $F --profile app --profile cascade --profile proxy --profile tunnel ps`，`db`、`app`、`proxy`、`cloudflared`、四個 cascade 容器都在，且 healthy。staging 用 `docker compose -f docker-compose.staging.yml ps`。
- `/health` 與 `/ready` 都回 200，`/ready` 的本文是 JSON `{"status":"ok"}`。`/health` 只表示程序存活；`/ready` 以 `SELECT 1` 檢查資料庫（最多等 3 秒），不通回 503 `database_unavailable`。migration 是否全部套用要另外用 `_sqlx_migrations` 確認，並看 `docker logs socrates-ntnu-app-1` 有沒有 WARN、ERROR。
- 數一次 `users`（§8）。
- 公開網址首頁回 200。不通時看 `docker logs socrates-ntnu-cloudflared-1`（tunnel 連線）與 `proxy` 的狀態。
- `app` 停著時，`socrates-ntnu-cascade-1` 會不斷重啟（它啟動時要探測 `http://app:3000/internal/cascade/v1/models`，連不到就結束）。這是正常現象，`app` 回來後會自己恢復。
- 如果 `app` 正常、`cascade-1` 仍重啟，先看 `docker logs socrates-ntnu-cascade-1`。`cascade` 要等 `cascade-tts` healthy 才會起來。
- staging 的 `staging-proxy` 掛在正式網路上，正式環境沒起來時它也起不來；正式恢復後再看一次 staging。

### 部署後的驗證

- 容器：`docker ps --filter name=socrates-ntnu --format '{{.Names}} | {{.Image}} | {{.Status}}'`，全部 Up，proxy 是 `vicanso/pingap:0.16.2` 且 healthy。
- 網路：`docker network inspect socrates-ntnu_edge -f '{{range .IPAM.Config}}{{.Subnet}} {{.IPRange}}{{end}}'` 應為 `172.29.250.0/24 172.29.250.128/25`；app 位址 `docker inspect -f '{{(index .NetworkSettings.Networks "socrates-ntnu_edge").IPAddress}}' socrates-ntnu-app-1` 應為 `172.29.250.10`；`docker exec socrates-ntnu-proxy-1 getent hosts app` 應回 `172.29.250.10`。
- 代理：`curl -s http://127.0.0.1:3080/proxy-health` 回 `ok`；`curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3080/ready` 與 `https://se3socrateschat.ntnu.cc/ready` 都回 200。
- SSE：用自己的帳號開一場對話，回覆要逐段出現。
- 語音 WebSocket：開一場語音對話，確認連得上、能互動。
- 代理日誌沒有持續的連線錯誤：`docker logs --tail 30 socrates-ntnu-proxy-1`。

## 16. 未決事項

- 規格決策（S-09）：正式環境的部署方式，以及 migration 是否改為部署前單獨執行（目前是 `RUN_MIGRATIONS=true`，見 §8）。
- 維運負責人決定是否採用每日加密備份與還原演練（S-09.3 第 3 節）。
- 用真人帳號走一次完整語音對話（含打斷），確認 Kokoro 的聲音與延遲可接受；第一次經 Pingap 的語音 WebSocket 也要一併確認。
- 管理者改掉初始密碼（`must_change_password` 狀態待確認）。
