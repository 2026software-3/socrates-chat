# NTNU 正式環境維運說明

本文件描述 NTNU 正式環境（主機 `msi_cubi`）與同一台主機上 staging 的維運做法。部署用的 compose 疊加檔在版控內（`docker-compose.intel.yml`、`docker-compose.msi-prod.yml`、`docker-compose.tunnel.yml`、`docker-compose.staging.yml`）；`.env` 等秘密只存在主機上，不進版控。

**NTNU 自 2026-11-07 起視為正式環境**（[S-09.3](specs/S-09.3-ops-procedures.md)）：資料要保留，migration 只往前（已套用的 migration 檔不能改，改 schema 一律新增 migration），不重建資料庫。這不代表正式環境的部署方式已定案（S-09.1 仍待決定）。

## 基本資訊

- 主機：`ssh msi_cubi`（Ubuntu、Core Ultra 5 322、14 GB；Intel iGPU＋NPU，沒有 NVIDIA）。ssh 卡住時，多半是 Tailscale SSH 要在瀏覽器重新驗證。
- 部署目錄：`~/socrates-intel`（分支 `deploy/msi-cubi-intel`）。Compose 專案名稱 `socrates-ntnu`（由 `docker-compose.msi-prod.yml` 的 `name:` 指定）。
- 所有 compose 指令都帶同一組檔案：

  ```bash
  F="-f docker-compose.yml -f docker-compose.intel.yml -f docker-compose.msi-prod.yml -f docker-compose.tunnel.yml"
  docker compose $F --profile app --profile cascade --profile proxy --profile tunnel ps
  ```

- 服務：`db`、`app`、`proxy`（nginx）、`cloudflared`、四個 cascade 容器（`cascade`、`cascade-adapter`、`cascade-whisper`、`cascade-tts`）。
- 秘密與設定：`~/socrates-intel/.env`（含 `APP_PUBLIC_URL`、`APP_IMAGE_TAG`、`CASCADE_INTEL_TAG`、`TRUSTED_PROXIES`、`CLOUDFLARE_TUNNEL_TOKEN`、`CASCADE_LLM_TOKEN`、`TEACHER_KEY_ENCRYPTION_KEYS` 等）。
- 資料庫：Docker 命名 volume `socrates-ntnu_pgdata_v2`（見下一節）。
- 公開網址：`https://se3socrateschat.ntnu.cc`，由 `.env` 的 `APP_PUBLIC_URL` 設定為容器的 `APP_BASE_URL`。CSRF 檢查要求 `Origin` 標頭等於這個網址，所以用 curl 或腳本呼叫 API 時，`Origin` 要填它。
- 備用網址 `https://socrates.ray-dev.stream` 也連到同一個服務，但寫入請求會因 `Origin` 不符被 CSRF 擋掉，只能拿來看頁面。

## 資料庫位置（Docker 命名 volume）

`db` 的資料放在命名 volume `socrates-ntnu_pgdata_v2`，由 `docker-compose.msi-prod.yml` 的 `db` 以 `!override` 覆寫 `docker-compose.yml` 的 `pgdata`，並在檔案底部宣告：

```yaml
    volumes: !override
      - pgdata_v2:/var/lib/postgresql/data
...
volumes:
  pgdata_v2:
```

- 不要把資料庫改成 bind mount 到可能沒掛上的主機路徑：沒掛上時 Docker 不會報錯，而是悄悄建一個空目錄，Postgres 就會初始化一個全新的資料庫。
- volume 不是備份：Docker 或磁碟壞掉就沒了，備份仍要另外做（見「部署前檢查」）。
- `msi_cubi` 上的 `socrates-intel-test_pgdata` 是瀏覽器測試堆疊的測試資料，與正式資料無關；`socrates-intel-test_cascade_hf`、`socrates-intel-test_cascade_cache` 是正式 cascade 也在用的模型與編譯快取（`docker-compose.msi-prod.yml` 以 `name:` 指定），不要刪。
- **主機或 Docker 重啟後，除了 `/health`、`/ready`，還要數一下資料**：`docker exec socrates-ntnu-db-1 psql -U socrates -d socrates -At -c "select count(*) from users"`，和上次記錄的數字比對。資料庫被重新初始化時，`/health`、`/ready` 仍會回 200。

### 資料庫被重新初始化時的還原

1. 先確認真的是空的：`users`、`conversations` 筆數，`pg_postmaster_start_time()` 是不是剛剛，`_sqlx_migrations` 是不是從頭套用。
2. 不要再重啟 `db`。用最新的 `pg_dump` 備份（`~/ntnu-backups/`）還原；備份是 `--no-owner` 的純文字（gzip），含 `_sqlx_migrations`，還原進**空的**資料庫即可：
   - `docker stop socrates-ntnu-app-1`；移除壞掉的 `db` 容器（`docker compose $F rm -sf db`，不加 `-v`）。
   - 把 `docker-compose.msi-prod.yml` 的 `db.volumes` 改成**新的**命名 volume（例如 `pgdata_v3`，並在底部宣告），`docker compose $F up -d --no-deps db`，等 healthy，確認 `information_schema.tables` 為 0。
   - `gunzip -c ~/ntnu-backups/<備份>.sql.gz | docker exec -i socrates-ntnu-db-1 psql -U socrates -d socrates -v ON_ERROR_STOP=1 -q`。
   - `docker compose $F --profile app up -d --no-deps --no-build app`（啟動時會套用備份之後新增的 migration），核對 `users`、`conversations`、`audit_logs`、`_sqlx_migrations` 與 `/health`、`/ready`。
3. 還原不回來的是「備份之後到事故之間」的資料；所以備份頻率決定損失，部署前與每次重啟後都要先有一份。

## 對外入口（Cloudflare Tunnel）

- 路徑：網際網路 → Cloudflare（`se3socrateschat.ntnu.cc`）→ Cloudflare Tunnel → `cloudflared` 容器 → `proxy:80`（nginx）→ `app:3000`。
- `se3socrateschat.ntnu.cc` 在學校的 DNS，用 Cloudflare for SaaS 的自訂主機名稱接到 `socrates.ray-dev.stream`（同時是 fallback origin）。tunnel 與 `ray-dev.stream` 在同一個 Cloudflare 帳號；tunnel 不能直接用學校帳號下的網域（`cfargotunnel.com` 只代理同一帳號內的 DNS 記錄）。
- 學校 DNS 介面的名稱是相對於 `se3socrateschat.ntnu.cc` 的（`@` 代表它自己）：CNAME 指到 `socrates.ray-dev.stream`（灰雲），驗證用的 TXT 填 `_cf-custom-hostname`、`_acme-challenge`，不要再加 `.se3socrateschat`。
- tunnel 的 ingress 是遠端設定（Cloudflare Zero Trust → Networks → Tunnels）：`se3socrateschat.ntnu.cc`、`socrates.ray-dev.stream` → `http://proxy:80`，`staging.se3socrateschat.ntnu.cc` → `http://staging-proxy:80`。`cloudflared` 只需要 `.env` 的 `CLOUDFLARE_TUNNEL_TOKEN`。
- `proxy` 綁 `127.0.0.1:3080` 與主機的 tailnet 位址（`PROXY_TAILNET_IP`，埠 3080）；`app` 綁 `127.0.0.1:3000`。都不對區網開放。主機上可以直接 `curl http://127.0.0.1:3000/ready` 或 `curl http://127.0.0.1:3080/ready` 驗證。

### 換公開網址時

CSRF 只認一個網址，換網址要同步改：

1. 在 Cloudflare 建好新的主機名稱（自訂主機名稱或同帳號的 DNS 記錄）與 tunnel ingress，先確認新網址的首頁回 200。
2. 改 `.env` 的 `APP_PUBLIC_URL`，重建 `app`（`docker compose $F --profile app up -d --no-deps --no-build --force-recreate app`）。不改的話 CSRF 的 `Origin` 檢查會擋掉登入與所有寫入。
3. 到 Google Cloud Console，把 OAuth 的重新導向 URI（`<網址>/api/auth/google/callback`）與授權來源換成新網址，否則 Google 登入會失敗。
4. 通知使用者新網址。

### 驗證 TRUSTED_PROXIES（來源 IP）

登入限流以來源 IP 為鍵；`.env` 的 `TRUSTED_PROXIES=172.16.0.0/12`（compose 網段：nginx 與 cloudflared）讓後端由右往左跳過受信任代理，取得 Cloudflare 帶來的真實來源。`TRUSTED_PROXIES` 是必填，沒設 `app` 不會啟動。改變入口後要重驗：

1. 用手機在行動網路（關 Wi-Fi）先查自己的 IP，馬上開公開網址，故意輸入錯誤密碼登入一次。
2. 查 `login_failures`：

   ```bash
   docker exec socrates-ntnu-db-1 psql -U socrates -d socrates -c "select left(login_key,8), ip, failures, last_failed_at from login_failures order by last_failed_at"
   ```

3. `ip` 應該是手機的公網位址，不是 `172.x`（Docker 網段）。IPv6 來源會合併成 `/64` 網段，所以只會看到前四段，例如 `2a09:bac3:5808:323::/64`。行動網路的位址會變動，查 IP 與登入要在同一分鐘內做。
4. 測完刪掉測試紀錄（`login_key` 若是真實帳號輸入也要刪，避免殘留失敗次數）：`delete from login_failures where login_key like '<前 8 碼>%'`。

## 反向代理與停機說明頁（S-16）

`proxy` 服務（profile `proxy`，nginx，設定在 `docker/proxy/`）在正式環境的路徑上：`cloudflared` → `proxy:80` → `app:3000`。後端連不上（容器停掉、重建中、主機剛開機）時，代理回一頁固定的三語說明（HTTP 503，「系統維護中，請稍後再試」，不含時間與原因）；後端自己的回應（包含維護模式的 503）原樣轉發。

- 代理刻意不 `depends_on` `app`，也設了 `restart: unless-stopped`：`app` 停著時代理仍在。`curl http://127.0.0.1:3080/proxy-health` 回 200 `ok` 只表示代理活著，與後端無關。
- 代理的存取日誌只有來源位址、方法、路徑（不含查詢字串）、狀態與耗時；不記 Cookie、Referer、User-Agent。
- 改代理設定前，先在任何有 Docker 的機器上跑 `scripts/verify-proxy.sh`（用假後端，不碰 `socrates-ntnu`）：檢查轉發、`X-Forwarded-For`、後端自己的 503 原樣轉發、SSE、WebSocket 升級、大請求，以及後端停掉時的停機頁與回來後的自動恢復。SSE 與語音要另外用真的後端確認（開一場對話，回覆要逐段出現；試一次語音）。
- 確認停機頁：先告知使用者（會中斷服務），`docker stop socrates-ntnu-app-1`，公開網址應出現停機頁，之後 `docker start socrates-ntnu-app-1`。
- 代理出問題時的回退：把 tunnel ingress 的服務從 `http://proxy:80` 改成 `http://app:3000`（Cloudflare Zero Trust 的遠端設定，不用動容器）。這時沒有停機頁，`TRUSTED_PROXIES` 仍包含 cloudflared 所在的 compose 網段，來源 IP 照常。

### 已知限制

- 代理只在**後端連不上或逾時**（nginx 自己產生的 502、504）時回停機頁；後端活著但回錯誤（包含它自己回的 502、504）會原樣轉發。
- 一般請求最多等 600 秒、語音 WebSocket 3600 秒；AI 回覆的產生時間上限遠小於此（`AI_TOTAL_TIMEOUT_SECS` 預設 60 秒）；摘要論證樹在背景產生，有自己的上限（`ARGUMENT_TREE_TIMEOUT_SECS` 預設 300 秒，見 `docs/specs/summary-argument-tree.md`）。

## 部署

`app` 用預先建好的映像（`docker-compose.msi-prod.yml` 的 `image: socrates-ntnu-app:${APP_IMAGE_TAG}`，沒有 `build`）。在 `~/socrates-intel`：

```bash
git pull --ff-only                                     # 分支 deploy/msi-cubi-intel
TAG=<名稱>-$(date +%Y%m%d)
docker build -t socrates-ntnu-app:$TAG .               # 先建好映像，服務不中斷
# 先做「部署前檢查」的備份，再把 .env 的 APP_IMAGE_TAG 改成 $TAG
docker compose $F --profile app up -d --no-deps --no-build app
```

- 換版只重建 `app`，中斷約數秒（期間代理回停機頁）。上一版映像的 tag 留著，退回時把 `APP_IMAGE_TAG` 改回去再跑同一行 `up`；migration 只往前，退回映像不要還原資料庫。
- 成功的標準：`curl http://127.0.0.1:3000/health` 與 `/ready` 都回 200，且 `/ready` 的本文是 JSON `{"status":"ok"}`。`/health` 只表示程序存活；`/ready` 以 `SELECT 1` 檢查資料庫（最多等 3 秒），不通回 503 `database_unavailable`。migration 是否全部套用要另外用 `_sqlx_migrations` 確認，並看 `docker logs socrates-ntnu-app-1` 有沒有 WARN、ERROR。
- 日誌出現 `failed to run migrations` 時，表示資料庫的 migration 紀錄與程式不一致。先比對 `_sqlx_migrations` 的 checksum，不要直接 `down -v`。
- cascade 換版時改 `CASCADE_INTEL_TAG`（映像建置見下方〈Intel 版 cascade〉），再 `docker compose $F --profile app --profile cascade up -d --no-build`。
- compose 檔中 `db` 的設定有變動時，下一次 `up` 會重建 `db` 容器（資料在 volume 裡不會丟，但會中斷幾秒）。

### 部署前檢查（含 migration 的版本）

1. **備份**（只讀，不影響執行中的資料庫）。備份含真實使用者資料：不進版控、權限限本人、用完要刪除（`shred -u`）。每日加密備份與保存方式見 [S-09.3 第 3 節](specs/S-09.3-ops-procedures.md)（是否適用 NTNU 待決定）。

   ```bash
   mkdir -p ~/ntnu-backups && (umask 077; docker exec socrates-ntnu-db-1 pg_dump -U socrates -d socrates --no-owner | gzip > ~/ntnu-backups/socrates-pre-<名稱>-$(date +%Y%m%d-%H%M%S).sql.gz)
   ```

2. **migration 有變動時，先在還原的備份上演練**：另起臨時 PostgreSQL（不要用 `socrates-ntnu` 的 db，也不要共用它的 volume），還原備份，用新版後端加上 `RUN_MIGRATIONS=true` 啟動（`LISTEN_ADDR` 改到別的連接埠），確認沒有 `VersionMismatch`，並檢查資料搬移結果。用完以 `docker rm -fv` 移除，連同 volume。
   - `sqlx::migrate!()` 在編譯時才嵌入 migration 檔；新增或還原 migration 後，要 touch `backend/src/main.rs` 強制重編，否則會跑到舊的 migration 內容。
3. **不要修改已套用的 migration 檔。** 要修正 schema，新增一個 migration。
4. 主機磁碟空間有限，建映像前先看 `df -h /`；用不到的舊映像可以 `docker image rm`（保留目前與上一版）。

## 重啟後的恢復

所有服務都設 `restart: unless-stopped`，主機開機後 Docker 會自己拉起來。重啟後確認：

- `docker compose $F --profile app --profile cascade --profile proxy --profile tunnel ps`：`db`、`app`、`proxy`、`cloudflared`、四個 cascade 容器都在，且 healthy。
- `/health` 與 `/ready` 都回 200（`/ready` 的本文要是 JSON `{"status":"ok"}`），並照上面數一次 `users`。
- 公開網址首頁回 200。不通時看 `docker logs socrates-ntnu-cloudflared-1`（tunnel 連線）與 `proxy` 的狀態。
- `app` 停著時，`socrates-ntnu-cascade-1` 會不斷重啟（它啟動時要探測 `http://app:3000/internal/cascade/v1/models`，連不到就結束）。這是正常現象，`app` 回來後會自己恢復，不用動 cascade。
- 如果 `app` 正常、`cascade-1` 仍重啟，先看 `docker logs socrates-ntnu-cascade-1`。`cascade` 要等 `cascade-tts`（Kokoro，iGPU）healthy 才會起來；第一次啟動要載入模型並編譯 SYCL 核心（healthcheck 的 `start_period` 是 240 秒），這段期間 healthcheck 未過屬正常。
- staging 的 `staging-proxy` 掛在正式環境的網路上，正式環境沒起來時它也起不來；正式恢復後再看一次 staging（見〈staging 環境〉）。

## 客訴通知信（SMTP）

客訴表單（`/complaint`）與管理者的「客訴」頁（`/admin/complaints`）不需要設定就能用；沒設定 SMTP 時客訴照常存資料庫，寄信狀態顯示「未寄信（尚未設定 SMTP）」。要讓管理者收到通知信，在 `~/socrates-intel/.env` 加上（變數說明見 `.env.example`）：

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
- 寄信失敗不影響提交者；管理者在「客訴」頁看到「寄信失敗」、一直停在「寄送中」（寄送途中後端重啟）或「未寄信」（設定 SMTP 之前送出的）時，可按「重寄通知信」；已寄出的沒有這個按鈕。注意：「寄送中」其實仍在寄的時候按重寄可能寄出兩封。日誌只會有客訴編號與失敗類別（`build`／`timeout`／`transport`），要查原因請直接測 SMTP 連線。
- 信件不含學號、電子郵件與對話原文；描述的原文只在信件內文與管理者的「客訴」頁。

## 刪除資料庫的限制

- NTNU 是正式環境。不要對 `socrates-ntnu` 執行 `down -v`，除非試用負責人明確同意刪除資料：它會刪掉專案的命名 volume，包含資料庫 `socrates-ntnu_pgdata_v2`。
- 刪除前要先告知試用負責人，並留下 `_sqlx_migrations` 與 `users` 的摘要以便核對。

## 管理者帳號與密碼

啟動時，`.env` 的 `ADMIN_EMAILS` 指定的信箱會建立管理者帳號，初始密碼來自 `ADMIN_INITIAL_PASSWORD`，且首次登入必須改密碼。

- 正式的 `.env` 沒有 `ADMIN_INITIAL_PASSWORD`。它只在建立新帳號時使用；啟動時不會重設既有帳號的密碼。
- 以初始密碼建立的管理者帳號 `must_change_password = true`，要由管理者登入後改密碼，見下一節。

### 更改自己的密碼（已登入）

- 用臨時密碼登入後，網頁會要求先更改密碼。之後所有人都可點右上角的頭像進入「帳號」頁（`/account`）的「更改密碼」自行更改（沒有內建密碼的教師與管理者，頭像沒有連結）；示範模式的示範帳號不能改，目前密碼連續輸錯 5 次會鎖定 15 分鐘（S-18）。
- API：`POST /api/auth/change-password`，body 為 `current_password` 與 `new_password`。新密碼長度 8 到 128 個字元，不能與舊密碼相同。成功回 204，同一帳號的其他 session 會失效。

### 重設其他使用者的密碼（忘記密碼）

- 由另一位管理者呼叫 `POST /api/admin/users/reset-password`，body 為 `{"email": "..."}`。
- 回應中的 `temporary_password` 只會出現這一次，要交給本人，不要寫進日誌、聊天或 issue。
- 該使用者的所有 session 會立即失效，登入後必須改密碼。
- 這個呼叫同樣需要 `Origin` 標頭等於公開網址。

### 沒有其他管理者可以重設時

1. 優先：如果管理者有使用同一信箱的 Google 帳號，可以用 Google 登入，會綁定到同一個帳號（不受改密碼限制）。
2. 如果上一步不可行，`ADMIN_INITIAL_PASSWORD` 無法重設既有帳號。只能直接修改資料庫中的密碼雜湊，必須先與試用負責人確認，並使用 Argon2id 產生 PHC 格式的雜湊。

## 示範模式與 seed-demo.sh

正式環境**沒有開** `DEMO_MODE`（`.env` 沒有這一項）。需要發表時可以在 `.env` 加 `DEMO_MODE=true` 並重建 `app`，但注意它是**公開網址**：

- 示範教師與學生的密碼寫在原始碼裡、登入頁也會列出來，任何人都能用它們登入，看到所有示範資料。示範帳號不含管理者。以 `DEMO_MODE=true` 啟動會把既有的示範帳號重新啟用並重設成公開密碼。
- 發表完成後把 `DEMO_MODE` 拿掉並重建 `app`：端點會回 404、登入頁不再列出帳號，但示範帳號仍帶著公開密碼，要在管理者介面停用或重設密碼。
- 正式資料庫裡的 10 位示範學生目前是停用的；示範教師 `demo.teacher@example.com` 啟用中，顯示名稱「測試用教師」，密碼已由管理者重設為非公開值。
- `scripts/seed-demo.sh` 跟不上目前的資料表（活動改為多題目後沒有 `topic_id` 欄位，第二段會中止），正式環境不要執行；staging 用 `scripts/seed-staging.sh`（見〈staging 環境〉）。

## 雲端部署評估（2026-11-10）

評估「app 放雲端、資料庫留在這台電腦的 D 槽、語音 cascade（要 GPU）也留在本機」是否可行。**結論：NTNU 維持本機部署。** 這不是 S-09 的決定，正式環境的部署方式仍待決定。

理由：DB 與 cascade 都在這台電腦，雲端的 app 沒有消除對它的依賴（電腦關機或斷線，雲端 app 讀不到資料，照樣整站掛掉），只多了一段網路路徑和費用。要真正脫離這台電腦，DB 也得搬到雲端，那是另一筆費用（託管 PostgreSQL）和待決的規格（S-09）。

### Docker cloud sandbox（`sbx --cloud`）

不適合當正式環境。它是給 AI agent 用的暫時性環境，文件標示為 experimental（Docker Agentic Platform；cloud volumes 另標 experimental）。

| 項目 | 結果 | 依據 |
| --- | --- | --- |
| 連回本機 | 不行：「沒有路徑回到執行 sbx 的機器」；也沒有 GPU、host bind mount | Docker 文件 |
| **UDP 不通** | `tailscale netcheck` 回 `UDP: false`、沒有對外 IPv4。對外網路政策只能放行「網域:埠」，不支援 `--protocol`，所以 Tailscale 打不了洞，**只能走 DERP 中繼** | 實測 |
| **位在美西** | 最近的 DERP 是 Seattle（13 ms）；本機到 Seattle 是 172 ms，這是跨太平洋的距離，就算直連往返也在 130–170 ms 左右。`sbx create` 沒有選區域的旗標 | 實測 |
| **24 小時過期** | 預設 1 小時；延長最多到建立後 24 小時；`--ttl 0` 是錯誤。到期動作 `stop`／`restart`／`delete`（`restart` 的 `--ttl` 至少 1 小時） | Docker 文件 |
| 規格 | 只有 micro（1 vCPU／2 GiB）、small、medium、large、xl；磁碟大小文件未載明（實測根目錄約 9.9 GB） | 文件、實測 |
| volume | 實驗性；只在 sandbox 結束時存快照，不是持續寫入；有 volume 的 sandbox 不能 stop，且要 `--on-timeout delete` | Docker 文件 |
| compose | sandbox 內 `docker exec` 可能落在 VM 的檔案系統而非容器內，`docker compose exec` 與 healthcheck 都受影響，`service_healthy` 可能卡住 | Docker 文件 |

實測結果（拋棄式 micro sandbox，已刪除）：

- sandbox 內可 `sudo`、有 `/dev/net/tun`；用 Tailscale 靜態安裝包加 `tailscaled --tun=tailscale0` 可以加入 tailnet。
- 從 sandbox 連本機 `db:5432`：Postgres 回 SCRAM 認證要求，連的是真的資料庫（只測到握手，沒有登入或查詢資料）。連 cascade 的 `/v1/realtime`：WebSocket 握手回 101。
- 但只走 DERP（香港），**往返約 170–200 ms**，TCP 連線建立的中位數約 197 ms。app 每個資料庫查詢都要付一次，一個有十幾個循序查詢的頁面會慢數秒；cascade 的語音串流延遲也會變差。實際頁面的耗時沒有量。

適合與不適合放上去的東西：

- 適合（不含真實資料、不用 GPU）：叫 agent 做 Issue 並開 PR；用 `seed-demo.sh` 合成資料起預覽環境給人看（網址是公開的，示範帳號密碼也是公開的）；AI 行為評測（要放行 `api.openai.com`）；吃 CPU 的建置（micro 的 2 GiB 編 Rust 會吃緊，用 medium 以上）。
- 不適合：**真實資料**（正式 DB、備份、學生對話原文都不要上去，測試資料一律合成）；cascade 語音；正式的 app 與 DB。

重做這個驗證的做法：建 micro sandbox，只放行 `pkgs.tailscale.com:443` 與 `*.tailscale.com:443`；本機用拋棄式 socat 容器把 `db:5432`、`cascade:8110` 轉出，**埠只綁在這台機器的 Tailscale IP**（不要綁 `0.0.0.0`）；金鑰用一次性、會過期的 auth key，經 stdin 傳入，不要放進指令列或版控。用完 `tailscale logout`、`sbx --cloud rm`、刪掉轉送容器，並到 Tailscale admin console 移除殘留的節點。

### Cloudflare

帳號現況（2026-11-10 唯讀查詢）：免費方案；沒有網域、Worker、Tunnel、Hyperdrive；Containers 需要 Workers Paid 方案，目前被拒絕。

- **把後端改寫成 Worker（Rust 編成 Wasm）：不建議。** 後端用 Axum、Tokio、SQLx、reqwest；`workers-rs` 文件列出的資料庫 crate 是 `tokio-postgres`（要改用 Workers 的 Socket），SQLx 不在清單內，等於重寫資料層與串流。`AGENTS.md` 也規定不得擅自更換主要框架。逐項評估見下方「後端改寫成 Worker（Workers Paid）」。
- **Cloudflare Containers（跑現有 Docker image）：** 技術上可行，需要 Workers Paid（每月 5 美元起）。但仍沒有 PostgreSQL：D1 是 SQLite，規格要求 PostgreSQL，`AGENTS.md` 禁止引入 Supabase 等未列出的基礎設施。DB 只能留在本機，經 Cloudflare Tunnel 連回，這需要網域、Access 服務金鑰與容器內的 `cloudflared`。
- **Cloudflare Tunnel 接現有本機部署：** 免費，支援 WebSocket 與 SSE，可取代 Tailscale Funnel。前提是要有一個放在 Cloudflare 的網域（`workers.dev` 不能當 Tunnel 主機名；沒網域只能用沒有穩定性保證的 quick tunnel）；換網址後 `APP_BASE_URL` 與 Google OAuth 的重新導向 URI 都要更新。

#### Containers 費用估算

目前的實際用量很小：app 記憶體約 26 MiB、CPU 約 0%、最近 24 小時 572 個請求（351 則對話訊息）、資料庫 10 MB、40 位使用者。流量遠低於任何方案的額度，費用取決於「容器開著的時間」。

Workers Paid 每月含 25 GiB-小時記憶體、375 vCPU-分鐘、200 GB-小時磁碟，超出的單價是每 GiB-秒 0.0000025 美元、每 vCPU-秒 0.000020 美元、每 GB-秒 0.00000007 美元。

| 容器規格（24 小時常駐，730 小時） | 超出額度的費用 | 加基本費 5 美元 |
| --- | --- | --- |
| lite：1/16 vCPU、256 MiB、2 GB 磁碟（預設） | 約 4.6 美元 | 約 9.6 美元 |
| 1/4 vCPU、512 MiB、2 GB 磁碟 | 約 16.1 美元 | 約 21 美元 |

為什麼會超出：額度是固定的，換算成 lite 規格，記憶體、CPU、磁碟三項都只夠開約 100 小時（25 ÷ 0.25、6.25 vCPU-小時 ÷ 0.0625、200 ÷ 2）；常駐 730 小時就有約 630 小時要照單價付費。規格越大，額度撐得越短，費用隨規格線性增加。

估算的假設與未驗證之處：

- CPU 以「配置量」計算（最壞情況）。文件只寫「依實際執行的每 10 毫秒計費」，沒有明說 CPU 是依配置量或實際使用量；若依實際使用量，CPU 那項會小很多。
- 容器設 `sleepAfter` 讓閒置時睡眠可以降費用，代價是第一個請求要等冷啟動。
- lite 只有 1/16 vCPU，登入時的 Argon2id 密碼雜湊會明顯變慢，若採用要實測登入耗時。
- 容器會被放在哪個區域、到台灣 DB 的延遲多少，文件沒有說，要實測。SQLx 從容器經 Hyperdrive 連資料庫是否可行，也沒有查到。

#### 後端改寫成 Worker（Workers Paid）

評估把整個後端改寫成 Worker（Rust 編成 Wasm，用 `workers-rs`），搭配 Workers Paid（每月 5 美元起）。**結論：費用最低，但這是重寫而不是搬家，而且資料庫仍在這台電腦，可用性沒有改善；不採用。**

後端規模（2026-11-10 量測）：Rust 約 15,440 行、74 個端點（`#[utoipa::path]`）、180 處 SQLx 查詢、14 筆 migration、4 處 `tokio::spawn`（`auth_store.rs`、`reply.rs`、`ai.rs`、`summary.rs`）。主要相依：Axum（含 `ws`）、Tokio 多執行緒、SQLx、reqwest（`rustls-tls`）、tokio-tungstenite、tower_governor、tower-http（`fs`）、argon2、openidconnect、utoipa-redoc。

各元件在 Workers 上的狀況：

| 元件 | 狀況 |
| --- | --- |
| SQLx（180 處查詢） | 不在 `workers-rs` 文件列出的支援清單。要改成 `tokio-postgres` 加 Workers 的 Socket，查詢與資料列對應都要重寫；`sqlx::migrate!` 不能在 Worker 內執行，migration 要移到別處跑。 |
| Tokio 多執行緒、`tokio::spawn`、interval／sleep | Worker 沒有常駐行程。回應結束後的背景工作（對話回覆、摘要生成）要改用 `waitUntil`、Queues 或 Durable Objects，各自的限制未逐一確認。 |
| tokio-tungstenite（連 cascade 的 WebSocket） | 要改用 Workers 自己的 WebSocket API；cascade 在本機，要經 Tunnel，需要網域。 |
| tower_governor（記憶體內的 IP 限流） | Worker 是多個彼此隔離的實例，記憶體不共享，限流會失效，要改成存資料庫或 Durable Object。 |
| tower-http 靜態檔 | 改用 Workers Static Assets。 |
| reqwest（`rustls-tls`） | 要換成 Wasm 版（底層用 fetch），功能（串流、multipart）要逐項確認。 |
| argon2 | 純 Rust，可以編成 Wasm；但免費方案 CPU 只有 10 ms，必須用 Workers Paid。 |
| 資料庫 | 沒有託管 PostgreSQL。D1 是 SQLite，與規格（PostgreSQL）不符，14 筆 migration 與 SQL 語法都要改，不採用。只能讓 Worker 經 Hyperdrive 連回本機的 D 槽資料庫。 |

平台限制（查自 Cloudflare 文件，Workers Paid）：

| 項目 | 免費 | 付費 |
| --- | --- | --- |
| CPU 時間 | 10 ms | 預設 30 秒，上限 5 分鐘 |
| subrequest | 每個請求 50 次 | 每個請求 10,000 次 |
| 記憶體 | 每個實例 128 MB | 每個實例 128 MB（含 Wasm 配置） |
| Worker 大小 | 64 MiB（未壓縮） | 64 MiB（未壓縮），沒有壓縮後上限 |
| 啟動時間 | 1 秒 | 1 秒 |
| 同時對外連線 | 每個請求 6 條 | 每個請求 6 條 |
| Hyperdrive 查詢 | 每天 10 萬次 | 不限 |

資料庫連回本機：Hyperdrive 的設定支援 Access 服務金鑰（`--access-client-id`、`--access-client-secret`）與 Workers VPC 服務（`--service-id`）兩種方式連私網資料庫。Tunnel 的公開主機名需要放在 Cloudflare 的網域；Workers VPC 方式是否需要網域，沒有查證。

費用：Workers Paid 基本費 5 美元。目前一天約 570 個請求（約每月 1.7 萬個），遠低於任何方案的額度，所以月費就是 5 美元；若走 Tunnel 連資料庫，再加網域約 10 美元／年。平台包含的請求與 CPU 額度數字這次沒有查文件，不列入。真正的成本在重寫：沒有逐檔評估，粗估資料層與執行環境的黏合程式碼要重寫大半，等於另寫一套後端，需要數週，且要同時維護原生版（`cargo test` 與本機部署）與 Wasm 版。

為什麼不採用：

- 資料庫仍在這台電腦，電腦關機或斷線，Worker 讀不到資料，整站照樣掛掉，可用性沒有改善。
- 規格已確定 Rust、Axum、Tokio、SQLx（`docs/specs/decisions.md`），`AGENTS.md` 規定不得擅自更換主要框架；要走這條路必須先以 PR 更新 `docs/specs/`，並決定 S-09 的正式環境方式。
- 未驗證：Worker 到本機資料庫的實際延遲、SSE 串流與 WebSocket 轉送在 Worker 上的行為、`waitUntil` 能否撐住摘要生成。

只有同時決定「資料庫也搬到雲端託管 PostgreSQL，並要求不依賴這台電腦」時才值得評估，組合是 Workers 加 Hyperdrive 加託管 PostgreSQL，費用為 5 美元加資料庫費用。

### 各方案取捨

| 方案 | 費用 | 到 DB 的延遲 | 是否依賴這台電腦 | 結論 |
| --- | --- | --- | --- | --- |
| 本機部署加 Tailscale Funnel | 免費 | 低（同機） | 是 | 不採用 |
| 現況：本機部署加 Cloudflare Tunnel | 免費，加網域約 10 美元／年 | 低（同機；入口走最近的 Cloudflare 機房，未實測） | 是 | **採用**（見〈對外入口〉） |
| Docker cloud sandbox 當正式環境 | 按秒計費（單價文件未載明） | 高，170–200 ms | 是 | 不採用 |
| Cloudflare Containers，DB 經 Tunnel 回本機 | 約 9.6–21 美元／月 | 未知，要實測 | 是 | 暫不採用 |
| 後端改寫成 Worker 加 Workers Paid，資料庫經 Hyperdrive 回本機 | 約 5 美元／月（加網域約 10 美元／年），另加數週的重寫成本 | 未知，要實測 | 是 | 不採用；要先走規格決策 |
| 全搬雲端（app 與託管 PostgreSQL） | 另計 | 低 | 否 | 要等 S-09 決定 DB 方案，再評估 |

重新評估的條件：S-09 決定正式環境的 DB 方案，或需要不依賴這台電腦的可用性。

## Intel 版 cascade（msi_cubi：iGPU＋NPU）

正式環境的語音 cascade 用 `docker-compose.intel.yml` 疊在 `docker-compose.yml` 上（`msi_cubi` 沒有 NVIDIA）。映像 tag 由 `.env` 的 `CASCADE_INTEL_TAG` 指定。

| 服務 | 裝置 | 映像（tag `CASCADE_INTEL_TAG`，目前 `intel-20261116`） |
| --- | --- | --- |
| `cascade-whisper` | NPU（OpenVINO GenAI，`OpenVINO/whisper-small-int8-ov`） | `socrates-cascade-whisper-npu`（約 2 GB） |
| `cascade-tts` | iGPU（PyTorch XPU，Kokoro 同一支 `kokoro_server.py`，`KOKORO_DEVICE=xpu`） | `socrates-cascade-tts-xpu`（約 11 GB） |
| `cascade`、`cascade-adapter` | CPU（本來就沒用 GPU） | `socrates-cascade-core`（約 20 GB，Ubuntu 24.04 底，不含 CUDA 基底） |

**建置前提**：NPU 的使用者空間驅動不在 Ubuntu 倉庫。要把 Intel linux-npu-driver 發行檔的 `intel-level-zero-npu`、`intel-driver-compiler-npu`、PPA 的 `libze1` 三個 `.deb`（版本與主機一致，msi_cubi 在 `~/npu_driver`）放進 `docker/cascade-intel/npu-debs/`（`.gitignore` 排除）。主機要能用 `/dev/dri/renderD128`、`/dev/accel/accel0`，`RENDER_GID` 填主機 render 群組（msi_cubi 是 991）。

**驗證（`docker/cascade-intel/smoke/`，假的內部 LLM＋合成語音走真的 realtime WebSocket）**：

| 項目 | 結果 |
| --- | --- |
| 單元測試（`test_whisper_ov_server.py`） | 15 項通過，主機與映像內各跑一次 |
| Whisper NPU，7 秒句 | 約 0.45 秒（CPU 路徑約 2 秒），輸出繁體 |
| Kokoro iGPU，7 秒句 | 暖機後 0.56／1.1／1.1 秒（首句／中句／長句）；CPU 是 0.8／1.9／2.2 秒 |
| 鏈路（學生說完 → …） | 辨識文字 0.7 秒、開始出聲 1.3–1.7 秒；單人 3/3、3 人、5 人同時全部通過 |
| 記憶體 | whisper 1.8 GB、tts 3.7 GB、cascade 2.7 GB，合計約 8.2 GB／14.6 GB |
| 噪音幻聽 | 無 VAD 時，高於能量門檻的噪音會辨識出 1–4 字；加上與 faster-whisper 版相同的 Silero VAD 後 6 種噪音／靜音全為 0 字 |

**已知差異與風險**：
- openvino-genai 不回 `no_speech_prob`／`avg_logprob`，沒有 faster-whisper 版的 `keep_segment` 信心過濾；改靠能量門檻＋Silero VAD。真人、多口音、雜訊環境的辨識品質**沒有**用真實語料比過 faster-whisper，只用合成語音驗證了鏈路與噪音。
- 取不到功耗（RAPL／hwmon 要 root）；「省電」只以 CPU 秒數佐證：Whisper 一句 4.2 → 0.12 CPU 秒（NPU），Kokoro 一句 10.4 → 1.1 CPU 秒（iGPU）。
- Kokoro 遇到新長度的句子首次會多 0.1–0.7 秒（SYCL 核心編譯，`SYCL_CACHE_PERSISTENT=1`，快取在 `cascade_cache` volume）。
- cascade 日誌有 mooncake／nixl 的 ERROR：可選的傳輸層沒裝，不影響功能。

### msi_cubi 瀏覽器測試環境（空資料庫，不碰正式環境）

`docker/cascade-intel/browser-test/compose.yml` 疊在上面兩個檔案之上，專案名稱 `socrates-intel-test`（獨立 volume 與網路）。平常沒有在跑；它的 `cascade_hf`、`cascade_cache` volume 正式環境也在用，`pgdata` volume 只有測試資料。設定範本 `browser-test/env.example`：模型設定與正式一致，Google 憑證填佔位值，`CASCADE_LLM_TOKEN` 在主機上用 `openssl rand -hex 32` 產生；AI key 不放檔案，測試堆疊起來後在「系統 AI」頁設定。`DEMO_MODE=true`：登入頁列出公開的示範帳號，所以**只能透過 `tailscale serve`（tailnet 內）提供，不可開 funnel**。

- 測試堆疊的設定與正式不同（正式的推理強度是 `high`）：`OPENAI_REASONING_EFFORT=medium`；app 映像 `socrates-ntnu-app:intel-test-diag`（`.env` 的 `APP_IMAGE_TAG`）含語音診斷日誌（`voice event`：事件種類、狀態、播放毫秒數，不含音訊與文字；`voice turn received`：回合數與是否併句）與「重連後接著說不再回 409」的改動（見 `docs/specs/cascade-voice-spike.md`），這個改動需求方尚未確認，不要部署到正式。
- 起動：`docker compose -p socrates-intel-test -f docker-compose.yml -f docker-compose.intel.yml -f docker/cascade-intel/browser-test/compose.yml --profile app --profile cascade up -d --no-build`。
- cascade 啟動時會先探測 app 的內部 LLM 端點，app 還沒起來它會乾淨退出並被 `restart: unless-stopped` 反覆重啟；app 起來後即穩定。
- `scripts/seed-demo.sh` 的第二段已跟不上目前資料表（活動改為多題目後沒有 `topic_id` 欄位，會中止），第一段（教室、名單、題目）可用；活動改用 API 建（示範教師登入 → `POST /api/activities` → `/publish`）。
- 經 app 代理的鏈路測試：`smoke/chain_client.py` 設 `CASCADE_URL=ws://app:3000/api/conversations/<id>/voice`、`COOKIE`、`ORIGIN`。

## staging 環境（msi_cubi）

只有 app＋db＋proxy，**不接語音**（沒有 cascade／Whisper／TTS），空資料庫加合成資料。與正式環境（專案 `socrates-ntnu`）獨立：
專案名 `socrates-staging`、db volume `socrates-staging_pgdata_staging`、自己的網路；記憶體上限 app 768 MB、db 512 MB、proxy 64 MB，
實際合計約 50 MB。網址 `https://staging.se3socrateschat.ntnu.cc`，作法與正式相同（Cloudflare for SaaS 自訂主機名稱＋學校 DNS 記錄，
進**同一條** tunnel）。

- 檔案：`docker-compose.staging.yml`、`docker/proxy/staging.conf`、`scripts/seed-staging.sh`，在分支 `deploy/msi-cubi-staging`。部署目錄是 `/var/lib/socrates-staging`（root 擁有，由自動部署更新，見下一節）；`.env.staging`、`.staging-credentials` 放在那裡（root 0600），不進版控。
- `.env.staging` 由正式 `.env` 衍生，**拿掉** SMTP／客訴信箱（staging 不寄信）、語音（CASCADE_*、STT／TTS）、tunnel token；管理者初始密碼另外產生。
  AI key 在 staging 介面自己設定（與正式的資料庫分開）。`TEACHER_KEY_ENCRYPTION_KEYS` 要用 staging 自己的值，不要與正式共用。
- 改 `.env.staging` 後要重建 `staging-app` 才會生效（用下面〈注意〉的 root compose 指令加 `--force-recreate staging-app`）。
- 服務名是 `staging-app`／`staging-db`／`staging-proxy`，不叫 `app`／`db`／`proxy`：staging-proxy 同時加入正式環境的網路（別名 `staging-proxy`）讓既有 cloudflared 連得到，
  若叫 `proxy`，正式網路裡的 `app` 解析會被 staging 干擾。tunnel remote ingress 的 `staging.se3socrateschat.ntnu.cc` → `http://staging-proxy:80`。
  正式環境沒在跑時 staging-proxy 起不來（網路不存在）。
- 更新：推送到 `deploy/msi-cubi-staging` 並核准部署（見下一節）。手動跑 compose 時 `STAGING_APP_IMAGE_TAG` 必填、沒有預設值，要用命令列環境變數給（compose 做 `${}` 替換時不讀 `.env.staging`，只有它的 `env_file` 會讀），指令見下一節〈注意〉。不要用 `down -v`。
- 首次建立與重置合成資料（每一步的 compose 指令都要以 root 在 `/var/lib/socrates-staging` 執行並帶 `STAGING_APP_IMAGE_TAG`，見下一節〈注意〉）：
  0. **ingress 已上線後重播時，先 `docker compose -f docker-compose.staging.yml stop staging-proxy`**，做完第 3 步再 `start`；DEMO_MODE 開著時登入頁會列出公開的示範帳號與密碼，不能在公網可達時開。
  1. `STAGING_DEMO_MODE=true docker compose -f docker-compose.staging.yml up -d --no-build`（讓後端建立示範帳號）。
  2. `DEMO_PSQL='docker exec -i socrates-staging-staging-db-1 psql -U socrates -d socrates' ./scripts/seed-staging.sh`（可重複執行）。
     不用 `seed-demo.sh`：它還是舊 schema（活動有 `topic_id`），在 2026-11-13 之後的 migration 上會失敗。
  3. **馬上把示範帳號密碼換成私有的**（`UPDATE users SET password_hash = <argon2id> WHERE student_id LIKE '115000%' OR email = 'demo.teacher@example.com'`），
     再 `docker compose -f docker-compose.staging.yml up -d --no-build`（不帶 `STAGING_DEMO_MODE`）關掉 DEMO_MODE。staging 在公網，公開的示範密碼不能留著。
- Google 登入要在 Google Cloud Console 加 `https://staging.se3socrateschat.ntnu.cc/api/auth/google/callback`。
- 學校 DNS（名稱相對於 `se3socrateschat.ntnu.cc`）：CNAME `staging` → `socrates.ray-dev.stream`（灰雲）、TXT `_cf-custom-hostname.staging`、兩筆 TXT `_acme-challenge.staging`。

### 自動部署（推送到 deploy/msi-cubi-staging 即更新）

- 流程：推送 `deploy/msi-cubi-staging` → GitHub Actions「Staging deploy (msi_cubi)」（`.github/workflows/staging-deploy.yml`，需 environment `staging-deploy` 核准）→ msi_cubi 上的 self-hosted runner（使用者 `gha-runner`，label `msi-cubi-staging`）執行 `sudo -n /usr/local/sbin/socrates-staging-deploy <sha>` → 包裝腳本（原始檔 `scripts/host/socrates-staging-deploy`）：驗證 SHA 格式、確認 SHA 是 `origin/deploy/msi-cubi-staging` 的祖先（含 head）、在 `/var/lib/socrates-staging` 切到該 commit、`docker build -t socrates-ntnu-app:staging-<sha12>`、`STAGING_APP_IMAGE_TAG=… docker compose -f docker-compose.staging.yml up -d --no-build`、經 `127.0.0.1:3081/ready` 健康檢查（60 秒內要回 200），最後只保留最近 5 個 staging 映像。
- 權限模型：
  - `gha-runner` 不在 docker 群組，不能直接使用 docker。它唯一能做的 root 操作是 sudoers 允許的那一個指令；sudoers 不限制參數（參數不接受萬用字元），參數限制完全由包裝腳本負責：只接受一個 40 位小寫十六進位 SHA，且必須是分支目前的 head。
  - 部署邏輯來自主機上 root 擁有的 `/usr/local/sbin/socrates-staging-deploy`，**不取自被推送的 commit**。
  - 密鑰（`.env.staging`、`.staging-credentials`）放在 `/var/lib/socrates-staging`，root 0600，runner 讀不到。
  - 只部署分支目前的 head；舊 commit 與分支外 commit 會被拒絕。回滾是 root 的緊急手段：`sudo STAGING_ALLOW_ROLLBACK=1 /usr/local/sbin/socrates-staging-deploy <祖先 sha>`（sudoers 不保留環境變數，runner 設不了）。
- 不碰正式環境：沒有 `down`、沒有 `-v`，不動 `socrates-ntnu`。不設 `STAGING_DEMO_MODE`，自動部署不會開示範帳號。
- 同一時間只跑一個部署：workflow 的 concurrency 加上包裝腳本的 `flock`。部署中途不取消；排隊中的舊執行會被較新的 push 取代。
- **信任邊界（重要）**：包裝腳本不能防止能 push 到 `deploy/msi-cubi-staging` 的人。部署的 app 原始碼與 Dockerfile 來自該分支，`docker build` 與 compose 會在主機上執行它們，等同 root。真正的防線是：
  1. 分支保護：ruleset「Protect deploy/msi-cubi-staging」禁止刪除與 force push。
  2. environment `staging-deploy`：Required reviewers，每次部署都要人工核准。
  3. fork PR 不觸發：公開 repo，workflow 只接 `push`，不要加 `pull_request` 觸發。
  4. 包裝腳本與 sudoers 只限制 runner 行程的權限，不取代上面三項。
- 主機設定（root，一次性；檔案在 repo 的 `scripts/host/`）：
  1. 建立部署目錄：`git clone -b deploy/msi-cubi-staging https://github.com/2026software-3/socrates-chat.git /var/lib/socrates-staging`，再把 `.env.staging`、`.staging-credentials` 以 `install -o root -g root -m 0600` 放進去。
  2. 安裝包裝腳本：`install -o root -g root -m 0755 scripts/host/socrates-staging-deploy /usr/local/sbin/socrates-staging-deploy`。它不會隨 commit 自動更新，之後每次改動都要 root 重新安裝。
  3. 安裝 sudoers：先 `visudo -cf scripts/host/gha-runner-staging.sudoers`，通過後 `install -o root -g root -m 0440 scripts/host/gha-runner-staging.sudoers /etc/sudoers.d/gha-runner-staging`。
  4. 移除 runner 的 docker 群組：`gpasswd -d gha-runner docker`，然後用 `systemctl restart "actions.runner.*msi-cubi-staging*"` 重啟 runner 服務，群組變更才會生效（不要以 root 執行 runner 目錄裡由 runner 使用者擁有的 `svc.sh`）。
  5. 驗證：`runuser -u gha-runner -- docker ps` 應該失敗；`runuser -u gha-runner -- sudo -n /usr/local/sbin/socrates-staging-deploy 0000000000000000000000000000000000000000` 應該被拒絕（SHA 不在分支上）。
  - runner 註冊：repo 的 Settings → Actions → Runners 取得 registration token（約 1 小時失效），用 `--labels msi-cubi-staging --unattended` 以 `gha-runner` 身份註冊，再由 root 在 runner 剛註冊、還沒啟動時執行一次 `svc.sh install gha-runner` 與 `svc.sh start`（之後不要再以 root 執行它，改用 `systemctl`）。
  - repo 設定：environment `staging-deploy`（Required reviewers、deployment branch 限定為 `deploy/msi-cubi-staging`），以及 ruleset「Protect deploy/msi-cubi-staging」。
- 注意：
  - `/var/lib/socrates-staging` 會被包裝腳本 `git reset --hard`，**不要在那裡手動改程式**。
  - 手動部署：`sudo /usr/local/sbin/socrates-staging-deploy <sha>`，sha 必須在 deploy 分支上。
  - 手動跑 compose（例如示範資料或 DEMO_MODE 操作）要用 root，並先設 tag：`sudo -H bash -c 'cd /var/lib/socrates-staging && export STAGING_APP_IMAGE_TAG=staging-$(git rev-parse --short=12 HEAD) && docker compose -f docker-compose.staging.yml up -d --no-build'`。上面「首次建立與重置合成資料」的指令都要照這個方式執行。
  - 要重跑目前 head 的部署：在 Actions 頁面對該次執行按 Re-run（會再次需要 environment 核准）。
  - 健康檢查失敗時新版容器可能已經取代舊的：看 `docker logs socrates-staging-staging-app-1`，修好後推新 commit 或 Re-run。
  - runner 離線時新的執行會排隊、不會失敗；用 `systemctl status "actions.runner.*msi-cubi-staging*"` 查看。

## 正式環境自動部署（推送到 deploy/msi-cubi-intel 即更新）

與 staging 同一套做法，但完全獨立：**從 `deploy/msi-cubi-staging` 開 PR 合併到 `deploy/msi-cubi-intel`**，合併（push）後由 GitHub Actions「Production deploy (msi_cubi)」（`.github/workflows/prod-deploy.yml`，environment `prod-deploy` 需人工核准）部署。

- 流程：核准後 msi_cubi 的 self-hosted runner（使用者 `gha-runner-prod`，label `msi-cubi-prod`，與 staging 的 `gha-runner` 不同人）執行 `sudo -n /usr/local/sbin/socrates-prod-deploy <sha>`。
- 包裝腳本（`scripts/host/socrates-prod-deploy`，root 擁有，不取自被推送的 commit）：只接受 40 位小寫十六進位 SHA 且必須是 `deploy/msi-cubi-intel` 的祖先；**先 pg_dump 備份到 `/var/backups/socrates-prod`（保留 14 份，備份失敗就中止，不動容器）**；`git reset --hard` 到該 commit；建置 `socrates-ntnu-app:prod-<sha12>`；以 `APP_IMAGE_TAG` 覆寫並 `up -d --no-build`（四個 compose 檔、四個 profile）；經 proxy 的 `/ready` 健康檢查（最多 90 秒）；只保留最近 5 個 prod 映像。不使用 `down`、不使用 `-v`，不碰資料庫 volume 與 cascade 映像。
- 部署目錄 `/var/lib/socrates-prod`（root 擁有的 clone）；秘密只有 `.env`（root 0600，runner 讀不到）。**不要在那裡手動改程式**。`~/socrates-intel` 是舊的手動部署目錄，切換後不再使用（保留作退路，勿再用它啟動，以免兩邊搶同一個 compose 專案）。
- 注意：第一次從新目錄部署時，bind mount 路徑變了，proxy 與 cascade 容器會被重建（cascade 模型載入需要數分鐘，語音暫時不可用）。
- 只部署 `deploy/msi-cubi-intel` 目前的 head；舊 commit 與分支外 commit 都會被包裝腳本拒絕（避免別的 workflow 跑在 prod runner 上時繞過核准回滾）。回滾是 root 的緊急手段：`sudo PROD_ALLOW_ROLLBACK=1 /usr/local/sbin/socrates-prod-deploy <祖先 sha>`（sudoers 不保留環境變數，runner 設不了）。**migration 只往前**，舊映像不一定相容新 schema；回滾前先確認，必要時用部署前備份還原。
- runner 隔離：prod runner 只該跑 `prod-deploy.yml`。repo 設定要關閉 fork PR 觸發 self-hosted runner；若 repo 之後搬到 organization，把 runner 放進限定 `prod-deploy.yml@refs/heads/deploy/msi-cubi-intel` 的 runner group。runner 上線後，**不要以 root 執行 runner 目錄裡的腳本**（`svc.sh` 等由 runner 使用者擁有），改用 `systemctl`。
- 信任邊界：同 staging。防線是分支保護（只能經 PR、禁止 force push）、environment `prod-deploy` 的 Required reviewers、workflow 只接 `push`（不要加 `pull_request`）。
- 主機端一次性設定：合併 PR 後在 msi_cubi 用 `scripts/host/setup-prod-autodeploy.sh`（`~/socrates-intel` 不是 git clone，要先 `scp` 過去；檔頭有一行指令版本，token 由本機 `gh` 產生並自動帶入，runner 固定 v2.338.2 並驗 SHA256）。它建立目錄、複製 `.env`（缺 `TEACHER_KEY_ENCRYPTION_KEYS` 會產生並要你存好）、安裝包裝腳本與 sudoers、建立 runner。
- **AI key 淘汰（2026-11-19）**：後端不再讀 `.apikeylist`／`OPENAI_API_KEYS`。升級後管理者第一件事是到「系統 AI」設定模型與 key，在那之前系統用途的 AI（個人練習、翻譯、主張分組、公告翻譯等）不可用；教師 key 與教室 AI 不受影響。`TEACHER_KEY_ENCRYPTION_KEYS` 遺失就解不開資料庫裡所有 key，要另存。主機上舊的 `.apikeylist` 可在確認 AI 正常後刪除。

## 狀態紀錄

| 日期 | 事件 |
| --- | --- |
| 2026-10-27 | 重建 `socrates-ntnu` 資料庫並重新部署（`/ready` 回 200，8 個 migration 全部成功）。以 `.env` 的 `ADMIN_EMAILS` 建立管理者帳號，登入測試通過。 |
| 2026-10-27 | 從 `.env` 移除 `ADMIN_INITIAL_PASSWORD`。 |
| 2026-11-07 | 決定 NTNU 自今天起視為正式環境：資料保留、migration 只往前（S-09.3）。 |
| 2026-11-07 | 在還原的 NTNU 備份上演練 `fix/f-29.1-activity-topics-alter-migration`（還原 `20261002010000`、新增 `20261107000000`）：10 個活動全部搬入 `activity_topics`，既有 8 個 checksum 不變，後端不再 `VersionMismatch`。尚未部署到 NTNU。 |
| 2026-11-07 | WSL 關機後 `db`（原本 `restart: no`）與 `app`（建立容器時 bind mount 路徑失效，`Exited (127)`）停止，網站約 30 分鐘連不上（13:17 起）；啟動 `db`、重建 `app` 後 `/ready` 回 200，cascade-1 恢復。`db` 改為 `restart: unless-stopped`。 |
| 2026-11-08 | WSL 內的 Tailscale 節點 `raynb` 消失，對外入口改為 Windows 端 Tailscale Funnel，網址改為 `https://ray-nb.tail5fc9fe.ts.net`（機器名稱曾短暫為 `ray-nb-win`）。`APP_BASE_URL` 已改並重建 `app`，用 `serve reset` 清掉舊網域後重設 funnel，Google OAuth 的重新導向 URI 已更新。 |
| 2026-11-08 | WSL 重開機後 `app`（`Exited (127)`）與 `cascade-1`（找不到 `/refs/reference.wav`；當時用 Qwen 的參考音，Kokoro 後已無此掛載）都因 bind mount 失效起不來，重建兩者後恢復（`/ready` 回 200，cascade 與 cascade-tts 皆 healthy）。 |
| 2026-11-08 | 以手機行動網路經 Funnel 故意登入失敗，`login_failures` 記到手機的真實 IPv6 `/64` 網段，確認 `TRUSTED_PROXIES` 在新入口下正常；測試紀錄已刪除。 |
| 2026-11-10 | 資料庫由 volume `socrates-ntnu_pgdata` 搬到 D 槽 `D:\socrates-ntnu\pgdata`（bind mount，來源用 `/run/desktop/mnt/host/d/...`）。搬前 `pg_dump` 備份；搬後 `users` 40、`conversations` 66、`_sqlx_migrations` 14 筆與搬前一致，`/health`、`/ready` 回 200，公開網址 `/ready` 回 200。中斷約 1 分鐘。舊 volume 保留當退路。 |
| 2026-11-10 | 評估 app 放雲端（DB 留 D 槽、cascade 留本機）：Docker cloud sandbox 的 UDP 不通、位在美西、24 小時過期，經 Tailscale 只能走 DERP，往返 170–200 ms；Cloudflare Containers 約 9.6–21 美元／月且仍需把 DB 接回本機。結論：NTNU 維持本機部署，見「雲端部署評估」。 |
| 2026-11-10 | 補充評估「後端改寫成 Worker 加 Workers Paid」：約 5 美元／月，但後端約 15,440 行、74 個端點、180 處 SQLx 查詢要重寫，資料庫仍在本機、可用性沒改善，且需先改規格。結論：不採用。 |
| 2026-11-11 | 部署 PR #509（`perf/chat-merge-queries`，cherry-pick `7c8eb51`、`c64fb21`、`ba1728c`）：送出訊息與保存 AI 回覆合併查詢，每則文字訊息的資料庫往返 13 → 10 次（教室對話），沒有 migration。部署前在 deploy 樹跑 sbx-test：750 通過、0 失敗；`pg_dump` 備份 `~/ntnu-backup-2026-11-11.sql`。只重建 `app`，`/health`、`/ready`（本機與公開網址）皆正常，`users` 40、`conversations` 70、`messages` 631、`_sqlx_migrations` 14 筆與部署前一致。舊映像留為 `socrates-ntnu-app:pre-chat-merge-20261111`，退回時把它改標回 `latest` 再以 `--no-build --force-recreate app` 重建。 |
| 2026-11-12 | 部署 F-34 公告與維護模式（#519）、F-35 反向代理檔案（#520，**只加入檔案，`proxy` 服務與 funnel 都沒動**）、F-36 客訴與寄信（#526 的提交，含後續的修正）：cherry-pick 進 `deploy/ntnu-integration`（不整條合 main，因為 main 上還有 NTNU 沒部署的 #496～#500）。新增 migration `20261111000000`、`20261111000100`；先在還原的備份上演練（表建立成功、稽核約束保留既有動作並收下新動作、`users` 40／`audit_logs` 204 不變）。部署前 sbx-test：後端 786 通過、前端 693 通過、fmt／clippy 通過。`pg_dump` 備份 `~/ntnu-backup-2026-11-12.sql`，舊映像另存標籤 `socrates-ntnu-app:pre-s15-s17-20261112`。只重建 `app`；`/health`、`/ready` 回 200 且本文為 JSON，`_sqlx_migrations` 16 筆全部 success。SMTP 尚未設定，客訴寄信狀態會是 `skipped`。 |
| 2026-11-12 | **資料庫被重新初始化**：09:16 WSL／Docker Desktop 重啟後，`db` 掛的 `/run/desktop/mnt/host/d/socrates-ntnu/pgdata` 在 Docker 內是 tmpfs，`D:\socrates-ntnu\pgdata` 也不見了（原因不明；資源回收筒與 D 槽都找不到），Postgres 建了全新資料庫：`users` 12（只有示範帳號與管理者）、對話 0；`/health`、`/ready` 仍回 200，所以沒有警報。發現時網站已經以空資料庫跑了約 7 分鐘，沒有真實使用者的寫入。處理：停 `app`、移除 `db` 容器、`docker-compose.ntnu.yml`（已備份到 `~/docker-compose.ntnu.yml.bak-20261112`）的 `db.volumes` 改為新命名 volume `pgdata_v2`、還原 `~/ntnu-backup-2026-11-12.sql`（部署 F-34～F-36 前的備份）、重啟 `app` 自動套用 `20261111000000`、`20261111000100`。結果：`users` 40、`conversations` 70、`messages` 647、`audit_logs` 204，`_sqlx_migrations` 16 筆全部 success，`/health`、`/ready` 回 200，示範教師與學生可登入且看得到示範教室與對話。遺失的只有備份之後到事故之間的變動（約 02:20～09:16，深夜；無法確認有沒有）。舊 volume `socrates-ntnu_pgdata` 未動。`scripts/seed-demo.sh` 沒有執行：示範資料已在備份裡，而且腳本會因為非示範活動「測試用活動2」使用示範題目 `…0101` 而違反外鍵中止（整筆交易回滾，資料未變）。 |
| 2026-11-12 | 部署 F-37 個人設定更改密碼（PR #537，cherry-pick `104baa4`、`1a9fb00`、`956e1f0`、`0255297`，另有整合分支的 `78596bb` 修 clippy）：學生在個人設定、教師與管理者在 `/account` 更改密碼；`GET /api/me` 新增 `can_change_password`；示範帳號 403 `demo_account`；目前密碼連續輸錯 5 次鎖 15 分鐘；成功記稽核 `change_password`；新增 migration `20261112000000`。同次上線的還有整合分支上另一個 session 的 F-33.2 管理者系統總覽與診斷頁（只有前端）。部署前在 deploy 樹跑 sbx-test：後端 816 通過、0 失敗（含 fmt、clippy），前端 741 通過、0 失敗；`pg_dump` 備份 `~/ntnu-backup-2026-11-12-pre-f37-final.sql`。只重建 `app`，`/health`、`/ready`（本機與公開網址）皆正常，`users` 40、`conversations` 70、`messages` 647 與部署前一致，`_sqlx_migrations` 17 → 18 筆且都成功。用公開的示範教師帳號驗證：`can_change_password` 為 false、change-password 回 403 `demo_account`、示範登入照常，稽核沒有新增 `change_password`（沒有更改任何真實帳號的密碼，成功更改與限流由 sandbox 測試覆蓋）。舊映像標為 `socrates-ntnu-app:pre-f37-20261112`，退回時把它改標回 `latest` 再以 `--no-build --force-recreate app` 重建；migration 只往前，退回映像不需要還原資料庫（新增的稽核動作對舊程式無害）。 |
| 2026-11-12 | 關閉 NTNU 的 `DEMO_MODE`（需求方指示，因示範帳號在示範模式下不能改密碼）：從未納入版控的 `docker-compose.ntnu.yml` 拿掉該行，只重建 `app`（映像與資料庫不動）。`/api/demo/accounts` 回 404、登入頁不再列出示範帳號。既有示範帳號的處置見下一列。 |
| 2026-11-12 | 部署 F-38 個人儀表板併入 `/account`（PR #543，cherry-pick 規格 `6f1dd05` 的運維文件部分與程式碼 `c19fc61`；規格檔 `decisions.md` 不帶進整合分支，與 F-37 相同）：學生的 `/account` 含個人設定與原儀表板內容，`/dashboard` 一律導向 `/account` 並保留查詢字串；入口改為右上角頭像，學生新增只放頭像的頂欄，側欄不再有「我的儀表板」；Google 連結回呼導向 `/account?link=ok`。沒有 migration。部署前 sbx-test：後端 806 通過、前端 734 通過、lint／typecheck／build 通過；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-f38-20261112-145959.sql.gz`；回滾映像 `socrates-ntnu-app:pre-f38-20261112`。只重建 `app`，本機與公開網址首頁 200、`/api/me` 未登入 401，使用者數 40 不變，新容器沒有錯誤日誌；打包檔確認含頭像連結與 `/dashboard` 導向。 |
| 2026-11-12 | 部署 PR #541（`feat/kokoro-voice`，cherry-pick `2467808`、`501c805`）：cascade 的 TTS 由 Qwen3-TTS 換成 Kokoro-82M（`zm_yunjian`），不再使用參考音；沒有 migration，沒動 `app`、`db`、Whisper。部署前 `cascade-tts` 已因先前 GPU 評估而停著（exit 137），所以沒有打斷使用中的語音。先用另一個 tag 建映像並在 GPU 上跑拋棄式容器驗證：啟動含預熱約 45 秒、預熱後首句 0.46 秒、純標點與表情回 50 ms 靜音（200）、4 併發依序完成、顯存約 0.9 GB；再以 `pg_dump` 備份 `~/ntnu-backup-2026-11-12-pre-kokoro.sql`（`users` 40），重建 `cascade-tts`、`cascade-adapter`、`cascade`，三者 healthy，經轉接層送繁體句與標點句皆 200，`/ready`（本機與公開網址）200，`users` 仍 40。**尚未用真人走完整語音鏈路（瀏覽器→Rust→cascade→Kokoro），也未測多人併發與長時間穩定性。**舊的 Qwen 映像留為 `socrates-cascade:pre-kokoro-20261112`，退回時：把它改標回 `latest`（`docker tag socrates-cascade:pre-kokoro-20261112 socrates-cascade:latest`），checkout 部署前的 `9d507e0` 的 `docker/` 與 `docker-compose.yml`，再以 `--no-build --force-recreate` 重建三個 cascade 服務（需要 `.env` 的 `CASCADE_REFERENCE_TEXT`、`CASCADE_REFS_DIR`，目前仍保留）。 |
| 待辦 | 用真人帳號走一次完整語音對話（含打斷），確認 Kokoro 的聲音與延遲可接受。 |
| 2026-11-12 | 部署 F-38 的後續：更改密碼改為「密碼」列的按鈕加彈出視窗，不攤開在帳號頁（PR #543 的 `21746d9`，cherry-pick 為 `b7f2a8e`；規格檔不帶進整合分支）。只改前端，沒有 migration。部署前 sbx-test：前端 737 通過、lint／typecheck／build 通過（後端沒有變動，沿用 806 通過）；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-f38-dialog-20261112-151741.sql.gz`（26 張表）；回滾映像 `socrates-ntnu-app:pre-f38-dialog-20261112`。整合分支上同時有另一個 session 的 Kokoro 提交（只動 cascade 的 `docker/`，後端無變更），`app` 本次建置不受影響。只重建 `app`，本機與公開網址首頁 200、`/api/me` 未登入 401，使用者數 40 不變，新容器沒有錯誤日誌；打包檔確認含新的「密碼」標題字串。 |
| 2026-11-12 | 處置 DEMO_MODE 關閉後仍帶公開密碼的示範帳號（需求方指示）：**10 位示範學生停用**（`disabled_at`、刪除其 session、各記一筆 `account_disabled` 稽核，操作者留空、原因註明為直接操作資料庫）；**示範教師 `demo.teacher@example.com` 維持啟用，顯示名稱改為「測試用教師」**。他們的對話與資料都保留。動手前 `pg_dump` 備份 `~/ntnu-backups/socrates-pre-demo-accounts-20261112-152709.sql.gz`。驗證：10 位皆停用、session 0、其他使用者沒有被停用（共 40 位不變）；用公開示範密碼登入示範學生回 `403 account_disabled`。示範教師的密碼**已由管理者在管理者介面重設（同日 15:44，稽核 `password_reset`），不再是原始碼裡的公開值**；驗證：用公開的示範密碼登入回 `401 invalid_credentials`。要恢復示範時以 `DEMO_MODE=true` 啟動會把這些帳號重新啟用並重設密碼。 |
| 2026-11-12 | 部署 F-38 的第二次調整：「更改密碼」按鈕放進個人設定、「儲存」旁，不另開一列或一張卡片；教師與管理者的帳號頁只有這個按鈕（PR #543 的 `ac749cf`，cherry-pick 為 `835fbd7`；規格檔不帶進整合分支）。彈出視窗渲染在表單外面，避免視窗內送出密碼連帶觸發儲存顯示名稱（有測試鎖住）。只改前端，沒有 migration。部署前 sbx-test：前端 739 通過、lint／typecheck／build 通過（後端沒有變動）；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-f38-inline-20261112-153623.sql.gz`（26 張表）；回滾映像 `socrates-ntnu-app:pre-f38-inline-20261112`。只重建 `app`，本機與公開網址首頁 200、`/api/me` 未登入 401，使用者數 40 不變，新容器沒有錯誤日誌；打包檔確認個人設定的按鈕為 `type=button`、舊的「密碼」標題字串已移除。 |
| 2026-11-12 | 部署 F-38 的第三次調整：學生在 `can_change_password` 為 `false`（只用 Google 登入，或示範帳號）時，個人設定「儲存」旁顯示「這個帳號不能在這裡更改密碼。」，與教師、管理者同一句，不再讓學生看不到按鈕卻沒有說明（PR #543 的 `4e15af5`，cherry-pick 為 `9d9ab07`；規格檔不帶進整合分支）。只改前端，沒有 migration。部署前 sbx-test：前端 743 通過、lint／typecheck／build 通過（後端沒有變動）；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-f38-notice-20261112-161240.sql.gz`（26 張表）；回滾映像 `socrates-ntnu-app:pre-f38-notice-20261112`。只重建 `app`，本機與公開網址首頁 200、`/api/me` 未登入 401，使用者數 40 不變，新容器沒有錯誤日誌；打包檔確認個人設定表單的備援分支會顯示 `account.unavailable`。 |
| 2026-11-12 | 把 `origin/main`（到 #544）整個合進 `deploy/ntnu-integration`（合併提交 `d7ab3b0`，需求方指示；先前都是 cherry-pick，不整條合）並部署。這次實際新增到整合分支的行為：#544 移除程序記憶體的 request 日誌與 `GET /api/admin/logs`（stdout 的 `request completed` 與 `x-request-id` 不變）；#492 對話列表顯示最後更新時間並依時間排序；#418／#497 總結列表改依完成時間排序（含分頁游標失效時「重試」改為從第一頁重新載入）；#498 活動對話標題一律用題目名稱。其餘多為規格文件與測試。診斷後端（#490）與 F-37 以 main 的 squash 版本為準，前端帳號頁與密碼區保留整合分支（F-38 與後續調整，main 還沒有）；`S-07.3` 成就文字與本文件的帳號入口也保留整合分支（尚未進 main 的 PR #477）。**新增 2 個 migration**：`20261109000000`（更換 `summaries_check` 與 `summaries_ready_idx`）、`20261109000100`（把標題等於活動名稱的活動對話補成題目名稱，**正式資料 31 筆標題被改**）；版本號早於已套用的 `20261109010000`，由 sqlx 當作未套用的舊版本補上。先在還原的備份上演練：兩個 migration 成功，補正後 0 筆殘留，`users` 40、`conversations` 70、`messages` 647 不變。部署前 sbx-test：後端 fmt／clippy 通過、811 通過（1 略過），前端 lint／typecheck／測試／build 通過（49 個測試檔）；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-main-merge-20261112-163113.sql.gz`（26 張表）；回滾映像 `socrates-ntnu-app:pre-main-merge-20261112`。只重建 `app`；`/health`、`/ready` 本機與公開網址皆 200，`/api/admin/logs` 回 404（已移除），`/api/me` 未登入 401；`users` 40、`conversations` 70、`messages` 647、`audit_logs` 240、`summaries` 58 與部署前一致，`_sqlx_migrations` 18 → 20 筆且都成功，新容器沒有錯誤日誌。退回映像時把 `pre-main-merge-20261112` 改標回 `latest` 再以 `--no-build --force-recreate app` 重建；migration 只往前，不要為了退回映像還原資料庫（還原備份會讓這段時間的新資料一起消失，標題也回到補正前）。 |
| 2026-11-12 | 部署 F-38 的第四次調整：教師與管理者的 `/account` 與學生共用同一個「個人設定」卡片版型，內容是唯讀的帳號資訊（顯示名稱、電子郵件、角色）與卡片裡的「更改密碼」按鈕，不能改密碼的帳號在按鈕的位置顯示說明；更改密碼的視窗狀態抽成 `usePasswordChange`（PR #543 的 `5c0b3c1`，cherry-pick 為 `02a5912`）。**這次 cherry-pick 在另一個 session 把 main 合進整合分支之後**；`decisions.md` 的 S-18 段落因此衝突（整合分支帶的是 main 的舊版文字），解法是把整段 S-18 換成 PR #543 的版本，其他段落沒動。只改前端，沒有 migration。部署前在部署樹跑 sbx-test：前端 lint／typecheck／build 通過、49 個測試檔 773 項通過（後端沒有變動，沿用合併 main 那次的 811 通過）；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-f38-staffcard-20261112-174615.sql.gz`（26 張表）；回滾映像 `socrates-ntnu-app:pre-f38-staffcard-20261112`。只重建 `app`，`/health`、`/ready`、首頁本機與公開網址皆 200，`/api/me` 未登入 401，`/api/admin/logs` 404（#544 已移除）；`users` 40、`conversations` 70、`messages` 647 不變，`_sqlx_migrations` 20 筆都成功，新容器沒有錯誤日誌；打包檔確認含 `account.roles`、`account.email`。；事後在同一棵樹（`0e81dd5`，與部署的樹只差本列文件）補跑 sbx-test all：後端 fmt／clippy 通過、811 通過（1 略過），前端 lint／typecheck／build 通過、49 個測試檔 773 項通過，補上部署前沒有重跑的後端 |
| 2026-11-12 | 部署 F-38 的第五次調整：教師與管理者也能修改顯示名稱，並與學生共用同一個個人設定表單（可編輯名稱、儲存、更改密碼，另有唯讀的電子郵件與角色兩列；移除先前分開的唯讀卡片）（需求方指示；來源提交 `04725d0`，cherry-pick 為 `f334fbf`，無衝突）。**後端行為變更（無 migration）**：`PATCH /api/me` 的 `display_name` 從只限學生放寬到學生、教師、管理者，沒有任何角色的帳號仍回 403；規則不變（30 字元、不可含控制字元），教師與管理者清除名稱後顯示電子郵件。S-08.2、S-18 與 API 文件隨提交更新。**PR #543 在這次的來源提交產生前已合併（squash `4b36733`）**，所以 `04725d0` 尚未進 `main`，需另開新 PR；整合分支先行部署。部署前的驗證在功能分支完成（後端 811 通過 1 略過、fmt／clippy 通過，前端 750 通過、lint 17 項既有警告）；部署後在部署樹跑完整 sbx-test：後端 fmt／clippy 通過、816 項通過 1 略過，前端 typecheck／test／build 通過、49 個測試檔 777 項通過。`pg_dump` 備份 `~/ntnu-backups/socrates-pre-f38-displayname-20261112-233900.sql.gz`（gzip 檢查通過）；回滾映像 `socrates-ntnu-app:pre-f38-displayname-20261112`。只重建 `app`，本機與公開網址首頁 200、`/health`、`/ready` 200、`/api/me` 未登入 401，使用者／對話／訊息 40／70／647 與 migration 20 筆都不變，新容器沒有 WARN 或 ERROR 日誌（只有未登入探測的 401 存取紀錄）；打包檔確認含個人設定字串。 |
| 2026-11-14 | 把 `origin/main`（到 #563）整個合進 `deploy/ntnu-integration`（合併提交 `da4ec7d`，需求方指示）並部署。實際新增的行為：#560 前端建置產物在映像建置時預先壓成 `.gz`，瀏覽器接受 gzip 時直接送出；`/assets/`（檔名帶內容雜湊）回 `Cache-Control: public, max-age=31536000, immutable`，找不到的檔案回 404（以前回 `index.html`）；`index.html` 與前端路由回 `no-cache`；教師與管理者頁面延遲載入；`GET /api/tasks` 合成一個語句；#561 移除未使用的 assistant-ui 元件；#562 session 的 `last_seen_at` 最多每分鐘寫入一次（S-08.2，閒置期限最多提早 1 分鐘到期）；#563 只改測試。**沒有新的 migration。** 衝突六個檔案：`backend/src/argument_tree.rs` 與三份文件（`api-reference.md`、`decisions.md`、`summary-argument-tree.md`）以 main 為準；`backend/tests/display_name.rs` 與前端 `account.test.tsx` 保留整合分支（教師的顯示名稱提示 `profile.hintStaff` 尚未進 main）。部署前在部署樹跑 sbx-test：後端 fmt／clippy 通過、920 通過（1 略過），前端 lint／typecheck／build 通過、50 個測試檔 804 項通過；`pg_dump` 備份 `~/ntnu-backups/socrates-pre-asset-delivery-20261114-140310.sql.gz`（29 張表，gzip 檢查通過）；回滾映像 `socrates-ntnu-app:pre-asset-delivery-20261114`。先建好映像再只重建 `app`，約 4 秒後 `/ready` 回 200。驗證：`/health`、`/ready`（本機與公開網址，本文為 JSON）200，`/api/me` 未登入 401；`users` 40、`conversations` 73、`messages` 685、`sessions` 53、`_sqlx_migrations` 27 筆全部 success，與部署前一致；新容器沒有 WARN 或 ERROR 日誌。進入點 JS 的傳輸量（帶 `Accept-Encoding: gzip`）由 722,368 bytes（未壓縮、沒有快取標頭）降為 88,041 bytes，公開網址量到相同的結果；不接受 gzip 的用戶端仍拿到原檔（299,228 bytes）。退回時把 `pre-asset-delivery-20261114` 改標回 `latest` 再以 `--no-build --force-recreate app` 重建，不需要還原資料庫。 |
| 2026-11-15 | 把 `origin/main`（到 #569）合進 `deploy/ntnu-integration`（合併提交 `f5e090b`，需求方指示）並部署，帶入 F-24 單場對話雷達圖：#568 AI 評分、#569 後端計算與重試 API（`GET /api/conversations/{id}/radar`、`POST .../radar/retry`）與前端計算狀態、雷達圖；另有 #565 只改文件。總結完成後自動計算雷達（失敗自動重試 1 次，仍失敗才標記 failed，不影響總結），學生端可手動重算。**沒有新的 migration**（相關的表在先前部署已建立，`_sqlx_migrations` 仍 27 筆）。合併無衝突；與 main 的差異維持在先前盤點的整合分支專屬內容（示範模式快速回覆、`profile.hintStaff`、本文件）。**這次沒有在部署樹跑 sbx-test**（以 main 上 #568、#569 的 CI 與 #571 分支預覽的人工驗證為準）。預覽驗證（不含正式環境）：個人練習結束後雷達自動算出並顯示；注入 AI 500 後前端顯示失敗訊息與重試按鈕，故障中重試再度失敗，排除後重試成功。部署前 `pg_dump` 備份 `~/ntnu-backups/socrates-pre-f24-radar-20261115-120626.sql.gz`（gzip 檢查通過）；回滾映像 `socrates-ntnu-app:pre-f24-radar-20261115`。先建好映像再只重建 `app`。驗證：`/health`、`/ready`（本機與公開網址）200，openapi 含兩個雷達路由；`users` 40、`conversations` 73、`messages` 685、`_sqlx_migrations` 27、`conversation_radar_results` 0，與部署前一致；新容器沒有 WARN、ERROR 日誌與 5xx。正式環境尚未有人實際跑過雷達，第一場正式對話結束後要看一次 `conversation_radar_results` 與日誌。退回時把 `pre-f24-radar-20261115` 改標回 `latest` 再以 `--no-build --force-recreate app` 重建，不需要還原資料庫。 |
| 2026-11-16 | 為 msi_cubi 建 Intel 版 cascade 映像（Whisper→NPU、Kokoro→iGPU），在 msi_cubi 以獨立專案 `socrates-intel-test` 與獨立 tag 驗證通過，測試容器已拆除；沒有動現行 NTNU，也沒有切換。細節見上節。公開網址搬遷後沿用 `raynb.tail5fc9fe.ts.net`（需求：使用者 2026-11-16 指示）。 |
| 待辦 | 管理者改掉初始密碼（`must_change_password` 仍為 true）。 |
| 待辦 | 維運負責人決定 NTNU 是否改為「部署前單獨執行 migrate」、以及是否採用每日加密備份與還原演練。 |
