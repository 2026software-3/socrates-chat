# NTNU 試用環境維運說明

本文件只描述在 NTNU 試用機器上的維運做法。部署設定檔 `docker-compose.ntnu.yml` 與 `scripts/deploy-ntnu.sh` 不進版控，只存在這台機器上。

**NTNU 自 2026-11-07 起視為正式環境**（[S-09.3](specs/S-09.3-ops-procedures.md)）：資料要保留，migration 只往前（已套用的 migration 檔不能改，改 schema 一律新增 migration），不再重建資料庫。這不代表正式環境的部署方式已定案（S-09.1 仍待決定）。

## 基本資訊

- Compose 專案名稱：`socrates-ntnu`（與開發用的 `socrates-chat` 分開）。
- 資料庫資料放在 Docker 命名 volume `socrates-ntnu_pgdata_v2`（2026-11-12 起，由 `docker-compose.ntnu.yml` 的 `db.volumes` 指定；見下一節）。舊 volume `socrates-ntnu_pgdata` 是 2026-11-10 的資料，只當退路。
- 設定檔：`docker-compose.yml` + `docker-compose.ntnu.yml`，所有 compose 指令都要帶上 `-f` 與 `-p socrates-ntnu`。
- 公開網址：`https://ray-nb.tail5fc9fe.ts.net`（Windows 端 Tailscale Funnel，見下一節）。這個值由 `docker-compose.ntnu.yml` 設定在容器的 `APP_BASE_URL`。
- `.env` 的 `APP_BASE_URL` 是 `http://localhost:3000`，但容器實際使用的是覆寫後的公開網址。CSRF 檢查要求 `Origin` 標頭等於容器的 `APP_BASE_URL`，所以用 curl 或腳本呼叫 API 時，`Origin` 要填公開網址，不能填 `.env` 的值。

## 資料庫位置（Docker 命名 volume）

2026-11-12 起，`db` 的資料放在 Docker 命名 volume `socrates-ntnu_pgdata_v2`。`docker-compose.ntnu.yml`（不進版控）的 `db` 以 `!override` 覆寫掉 `docker-compose.yml` 的 `pgdata` volume，並在檔案底部宣告它：

```yaml
    volumes: !override
      - pgdata_v2:/var/lib/postgresql/data
...
volumes:
  pgdata_v2:
```

- **不要再把資料庫放在 D 槽的 bind mount。** 2026-11-10 曾搬到 `D:\socrates-ntnu\pgdata`（來源 `/run/desktop/mnt/host/d/...`），但 2026-11-12 09:16 WSL／Docker Desktop 重啟後，Docker 看到的 `/run/desktop/mnt/host/d` 變成一個 **tmpfs**（記憶體檔案系統，不是真的 D 槽），`D:\socrates-ntnu\pgdata` 也不見了（從 WSL 與 Windows 都看是空的；資源回收筒與 D 槽也找不到 `PG_VERSION`，**消失原因不明**）。`db` 啟動時資料目錄是空的，Postgres 建了全新資料庫，網站變成只有示範帳號。bind mount 到「Docker Desktop 可能沒掛上的主機路徑」沒有失敗保護：沒掛上時不會報錯，而是悄悄在別處建空目錄。
- 命名 volume 放在 Docker 虛擬機的磁碟內，歷次重開機（2026-10-30 起到 2026-11-10）都正常；缺點是佔 C 槽或 Docker 的磁碟。要把資料放 D 槽，改用**備份**放 D 槽，不要放原始資料目錄。
- 備份、`pg_dump`、`docker exec ... psql` 的指令不變。volume 不是備份：Docker 或磁碟壞掉就沒了，備份仍要另外做（見「部署前檢查」）。
- 舊 volume `socrates-ntnu_pgdata` 是 2026-11-10 13:53 的資料（約 64.6 MB），目前沒有容器使用，只當退路，經試用負責人同意才刪。
- **每次 WSL／Docker Desktop 重啟後，除了 `/health`、`/ready`，還要數一下資料**：`docker exec socrates-ntnu-db-1 psql -U socrates -d socrates -At -c "select count(*) from users"`（2026-11-12 的數字是 40，且不應該突然變成只有示範帳號的 12）。這次事故就是 `/health`、`/ready` 都回 200，資料卻已經空了。

### 資料庫被重新初始化時的還原（2026-11-12 的做法）

1. 先確認真的是空的：`users`、`conversations` 筆數，`pg_postmaster_start_time()` 是不是剛剛，`_sqlx_migrations` 是不是從頭套用。
2. 不要再重啟 `db`。用最新的 `pg_dump` 備份（`~/ntnu-backup-YYYY-MM-DD.sql`）還原；備份是 `--no-owner` 的純文字，含 `_sqlx_migrations`，還原進**空的**資料庫即可：
   - `docker stop socrates-ntnu-app-1`；移除壞掉的 `db` 容器（`docker compose ... rm -sf db`，不加 `-v`）。
   - 把 `docker-compose.ntnu.yml` 的 `db.volumes` 改成**新的**命名 volume（例如 `pgdata_v3`），`up -d --no-deps db`，等 healthy，確認 `information_schema.tables` 為 0。
   - `docker exec -i socrates-ntnu-db-1 psql -U socrates -d socrates -v ON_ERROR_STOP=1 -q < ~/ntnu-backup-YYYY-MM-DD.sql`。
   - `up -d --no-deps --no-build app`（啟動時會套用備份之後新增的 migration），核對 `users`、`conversations`、`audit_logs`、`_sqlx_migrations` 與 `/health`、`/ready`。
3. 還原不回來的是「備份之後到事故之間」的資料；所以備份頻率決定損失，部署前與每次重啟後都要先有一份。

## 對外入口（Windows 端 Tailscale Funnel）

2026-11-08 起，對外入口由 **Windows 的 Tailscale** 提供，WSL 裡不再有 Tailscale 節點（舊節點 `raynb` 已離線，舊網址 `raynb.tail5fc9fe.ts.net` 失效）。

- 路徑：網際網路 → Funnel（443）→ Windows 的 Tailscale → `127.0.0.1:3000` → `app` 容器。`~/.wslconfig` 設 `networkingMode=mirrored`，Windows 的 `127.0.0.1` 與 WSL 共用，所以不需要 portproxy。`app` 只綁 `127.0.0.1:3000`，不對區網開放。
- 網域是「Tailscale 機器名稱」加 tailnet 後綴：機器名稱 `Ray_NB` → `ray-nb.tail5fc9fe.ts.net`。**機器名稱一改，網域就跟著變。**
- 在 WSL 裡操作 Windows 的 Tailscale，用完整路徑：

  ```bash
  TS="/mnt/c/Program Files/Tailscale/tailscale.exe"
  "$TS" funnel --bg 3000        # 公開 443，轉到 127.0.0.1:3000（會公開到網際網路）
  "$TS" serve status            # 應該只有一條：https://ray-nb.tail5fc9fe.ts.net (Funnel on) → http://127.0.0.1:3000
  ```

- 從這台機器連自己的 Funnel 網址會被拒絕（`Connection refused`），要驗證外部可達，得用另一台裝置在行動網路下開網址。

### 改 Tailscale 機器名稱或網域時

serve／funnel 設定是綁在舊網域上的，改名後不會跟著搬。依序處理：

1. 到 Tailscale 確認新的網域（`"$TS" status --json` 的 `Self.DNSName`）。
2. 把 `docker-compose.ntnu.yml` 的 `APP_BASE_URL` 改成新網址，重建 `app`（`up -d --no-deps --no-build --force-recreate app`）。不改的話 CSRF 的 `Origin` 檢查會擋掉登入與所有寫入。
3. 重設 funnel：`"$TS" serve reset`，再 `"$TS" funnel --bg 3000`。CLI 沒辦法依主機名單獨刪舊條目，所以只在設定裡沒有別的服務時才用 `reset`（先看 `serve status`）；會中斷幾秒。確認 `serve status` 不再出現舊網域。
4. 到 Google Cloud Console，把 OAuth 的重新導向 URI 與授權來源換成新網域，否則 Google 登入會失敗。
5. 通知使用者新網址。

### 驗證 TRUSTED_PROXIES（來源 IP）

登入限流以來源 IP 為鍵；`TRUSTED_PROXIES: 172.16.0.0/12`（`docker-compose.ntnu.yml`）讓後端信任 Docker bridge 閘道送來的 `X-Forwarded-For`。改變入口後要重驗：

1. 用手機在行動網路（關 Wi-Fi）先查自己的 IPv6，馬上開公開網址，故意輸入錯誤密碼登入一次。
2. 查 `login_failures`：

   ```bash
   docker exec socrates-ntnu-db-1 psql -U socrates -d socrates -c "select left(login_key,8), ip, failures, last_failed_at from login_failures order by last_failed_at"
   ```

3. `ip` 應該是手機的公網位址，不是 `172.x`（Docker 閘道）或 `100.x`（Tailscale）。IPv6 來源會合併成 `/64` 網段，所以只會看到前四段，例如 `2a09:bac3:5808:323::/64`。行動網路的位址會變動，查 IP 與登入要在同一分鐘內做。
4. 測完刪掉測試紀錄（`login_key` 若是真實帳號輸入也要刪，避免殘留失敗次數）：`delete from login_failures where login_key like '<前 8 碼>%'`。

## 反向代理與停機說明頁（S-16，選用）

`docker-compose.yml` 有一個 `proxy` 服務（profile `proxy`，nginx，設定在 `docker/proxy/`）。啟用後路徑變成：網際網路 → Funnel（443）→ Windows 的 Tailscale → `127.0.0.1:3080` → `proxy` 容器 → `app:3000`。後端連不上（容器停掉、重建中、WSL 剛開機）時，代理回一頁固定的三語說明（HTTP 503，「系統維護中，請稍後再試」，不含時間與原因）；後端自己的回應（包含維護模式的 503）原樣轉發。這一層**預設沒有啟用**；是否上線由維護者決定。

- `proxy` 只綁 `127.0.0.1:3080`（`PROXY_PORT` 可改）。`app` 仍然綁 `127.0.0.1:3000`，所以**切換與回退都只是改 funnel 指向哪個埠**，兩條路可以同時存在。
- 代理刻意不 `depends_on` `app`，也設了 `restart: unless-stopped`：`app` 停著時代理仍在。`curl http://127.0.0.1:3080/proxy-health` 回 200 `ok` 只表示代理活著，與後端無關。
- 代理的存取日誌只有來源位址、方法、路徑（不含查詢字串）、狀態與耗時；不記 Cookie、Referer、User-Agent。

### 上線前驗證

1. 在任何有 Docker 的機器上跑 `scripts/verify-proxy.sh`（用假後端，不碰 `socrates-ntnu`）：檢查轉發、`X-Forwarded-For`、後端自己的 503 原樣轉發、SSE、WebSocket 升級、大請求，以及後端停掉時的停機頁與回來後的自動恢復。
2. 在 NTNU 機器上，**先不動 funnel**，只啟動代理，直接打代理的埠確認它轉得到真的後端：

   ```bash
   docker compose -f docker-compose.yml -f docker-compose.ntnu.yml -p socrates-ntnu --profile app --profile proxy up -d --no-deps proxy
   curl -s http://127.0.0.1:3080/proxy-health            # ok
   curl -s http://127.0.0.1:3080/ready                   # {"status":"ok"}
   ```

3. 確認 `docker-compose.ntnu.yml` 的 `TRUSTED_PROXIES` 包含 compose 網路（目前 `172.16.0.0/12`）。代理的對端是 compose 網路內的位址，沒包含的話所有人在登入限流眼裡都是同一個來源。
4. SSE 與語音要用真的後端確認：`verify-proxy.sh` 用的是假後端。切換後立刻開一場對話（回覆要逐段出現）並試一次語音；有問題就照下面〈回退〉。

### 切換

```bash
TS="/mnt/c/Program Files/Tailscale/tailscale.exe"
"$TS" funnel --bg 3080         # 公開 443 改轉到 127.0.0.1:3080（代理）
"$TS" serve status             # 應該只有一條：https://ray-nb.tail5fc9fe.ts.net (Funnel on) → http://127.0.0.1:3080
```

切換後馬上重跑上面〈驗證 TRUSTED_PROXIES（來源 IP）〉的步驟：`login_failures.ip` 要是手機的公網位址，不是 `172.x`。再用 `docker stop socrates-ntnu-app-1` 短暫停掉 app（先告知使用者，會中斷服務）確認公開網址出現停機頁，之後 `docker start socrates-ntnu-app-1`。

### 回退

出任何問題（回覆不逐段、語音連不上、來源 IP 不對），直接把 funnel 改回後端，不需要改容器：

```bash
"$TS" funnel --bg 3000
"$TS" serve status             # → http://127.0.0.1:3000
```

要完全移除代理：`docker compose -f docker-compose.yml -f docker-compose.ntnu.yml -p socrates-ntnu --profile proxy rm -sf proxy`。

### 已知限制

- 代理只在**後端連不上或逾時**（nginx 自己產生的 502、504）時回停機頁；後端活著但回錯誤（包含它自己回的 502、504）會原樣轉發。
- WSL 或 Docker Desktop 重啟後，`proxy` 與 `app` 都要回來；`app` 的 bind mount 失效時（見〈重啟後的恢復〉）代理仍會顯示停機頁，直到 `app` 重建。
- 一般請求最多等 600 秒、語音 WebSocket 3600 秒；AI 回覆的產生時間上限遠小於此（`AI_TOTAL_TIMEOUT_SECS` 預設 60 秒）。

## 部署

```bash
SKIP_PULL=1 ./scripts/deploy-ntnu.sh > deploy.log 2>&1; echo "exit=$?"
```

- 不要把腳本接在管線後面（例如 `| tail`），管線會遮蔽失敗的退出碼。
- `exit=0` 且 `curl http://127.0.0.1:3000/health` 與 `curl http://127.0.0.1:3000/ready` 都回 200 才算部署成功；`exit=1` 時，`deploy.log` 最後會有 app 最近 30 行日誌。`/health` 只表示程序存活；`/ready` 以 `SELECT 1` 檢查資料庫（最多等 3 秒），正常回 200 `{"status":"ok"}`，不通回 503 `database_unavailable`。尚未重新部署包含 #473 的映像之前，舊映像沒有 `/ready` 路由，對它發請求會拿到前端首頁 HTML 的 200（SPA fallback），所以要確認回應本文是 JSON `{"status":"ok"}`，不能只看狀態碼。migration 是否全部套用仍要另外用 `_sqlx_migrations` 確認。
- 日誌出現 `failed to run migrations` 時，表示資料庫的 migration 紀錄與程式不一致。先比對 `_sqlx_migrations` 的 checksum，不要直接 `down -v`。
- `docker-compose.ntnu.yml` 或 `docker-compose.yml` 中 `db` 的設定有變動時，下一次部署會重建 `db` 容器（資料在 volume 裡不會丟，但會中斷幾秒）。

### 部署前檢查（含 migration 的版本）

1. **備份**（只讀，不影響執行中的資料庫）。備份含真實使用者資料：不進版控、權限限本人、用完要刪除（`shred -u`）。每日加密備份與保存方式見 [S-09.3 第 3 節](specs/S-09.3-ops-procedures.md)（是否適用 NTNU 待決定）。

   ```bash
   (umask 077; docker exec socrates-ntnu-db-1 pg_dump -U socrates -d socrates --no-owner > ~/ntnu-backup-$(date +%F).sql)
   ```

2. **migration 有變動時，先在還原的備份上演練**：另起臨時 PostgreSQL（不要用 `socrates-ntnu` 的 db，也不要共用它的 volume），還原備份，用新版後端加上 `RUN_MIGRATIONS=true` 啟動（`LISTEN_ADDR` 改到別的連接埠），確認沒有 `VersionMismatch`，並檢查資料搬移結果。用完以 `docker rm -fv` 移除，連同 volume。
   - `sqlx::migrate!()` 在編譯時才嵌入 migration 檔；新增或還原 migration 後，要 touch `backend/src/main.rs` 強制重編，否則會跑到舊的 migration 內容。
3. **不要修改已套用的 migration 檔。** 要修正 schema，新增一個 migration。

## 重啟後的恢復

`db` 與 `app` 都設 `restart: unless-stopped`（`docker-compose.ntnu.yml` 的 `db` 必須保留這一行）。WSL 或 Docker Desktop 重啟後，先用 `docker ps -a --filter name=socrates-ntnu` 確認 `db`、`app`、四個 cascade 容器都在，且 `/health` 與 `/ready` 都回 200（`/ready` 的本文要是 JSON `{"status":"ok"}`，見上）。

- `app` 若是 `Exited (127)`，用 `docker inspect socrates-ntnu-app-1 --format '{{.State.Error}}'` 看原因。訊息是 `failed to fulfil mount request ... docker-desktop-bind-mounts ... no such file or directory` 時，表示容器指向的 bind mount 路徑已失效，`docker start` 沒用，要重建容器（不重建映像、不動其他服務）：

  ```bash
  docker compose -f docker-compose.yml -f docker-compose.ntnu.yml --profile app up -d --no-deps --no-build --force-recreate app
  ```

- `app` 停著時，`socrates-ntnu-cascade-1` 會不斷重啟（它啟動時要探測 `http://app:3000/internal/cascade/v1/models`，連不到就結束）。這是正常現象，`app` 回來後會自己恢復，不用動 cascade。
- 如果 `app` 正常、`cascade-1` 仍重啟，先看 `docker logs socrates-ntnu-cascade-1`。`cascade` 要等 `cascade-tts`（Kokoro）healthy 才會起來；Kokoro 啟動時會先載入並預熱模型（首次還要下載權重），這段期間 healthcheck 未過屬正常，約兩分鐘內不用處理。日誌若是 Docker Desktop 的 bind mount 失效（例如 `CASCADE_HF_CACHE` 指向的主機目錄），重建 `cascade`（要帶 `--profile cascade`）：

  ```bash
  docker compose -f docker-compose.yml -f docker-compose.ntnu.yml --profile app --profile cascade up -d --no-deps --no-build --force-recreate cascade
  ```

- 對外入口也要確認：`"/mnt/c/Program Files/Tailscale/tailscale.exe" serve status` 應該有 `https://ray-nb.tail5fc9fe.ts.net (Funnel on)`。Windows 或 Tailscale 重啟後設定通常會保留，沒有的話重跑 `funnel --bg 3000`。
- WSL 重開機後，已開著的終端機不一定有 `docker` 群組；`docker` 出現 `permission denied ... docker.sock` 時，開新的終端機。

## 客訴通知信（SMTP）

客訴表單（`/complaint`）與管理者的「客訴」頁（`/admin/complaints`）不需要設定就能用；沒設定 SMTP 時客訴照常存資料庫，寄信狀態顯示「未寄信（尚未設定 SMTP）」。要讓管理者收到通知信，在 `.env` 加上（變數說明見 `.env.example`）：

```
SMTP_HOST=...
SMTP_FROM=Socrates <noreply@example.edu>
COMPLAINT_TO=admin1@example.edu,admin2@example.edu
SMTP_USERNAME=...
SMTP_PASSWORD=...
```

- `SMTP_HOST`、`SMTP_FROM`、`COMPLAINT_TO` 缺任何一項就視為沒啟用；格式錯誤時後端啟動會失敗並指出是哪個變數（不印出位址或密碼）。
- 預設用 STARTTLS（埠 587）；Gmail 這類服務要用應用程式密碼。`SMTP_SECURITY=tls` 走埠 465；`none` 只給本機測試。
- 改 `.env` 後要重建後端容器才會生效。這是設定變更，不含 migration，但新版本本身含一個 migration（`create_complaints`），部署照「部署前檢查」。
- 寄信失敗不影響提交者；管理者在「客訴」頁看到「寄信失敗」、一直停在「寄送中」（寄送途中後端重啟）或「未寄信」（設定 SMTP 之前送出的）時，可按「重寄通知信」；已寄出的沒有這個按鈕。注意：「寄送中」其實仍在寄的時候按重寄可能寄出兩封。日誌只會有客訴編號與失敗類別（`build`／`timeout`／`transport`），要查原因請直接測 SMTP 連線。
- 信件不含學號、電子郵件與對話原文；描述的原文只在信件內文與管理者的「客訴」頁。

## 刪除資料庫的限制

- NTNU 是正式環境。不要對 `socrates-ntnu` 執行 `down -v`，除非試用負責人明確同意刪除資料：它會刪掉所有命名 volume，包含現在的資料庫 `socrates-ntnu_pgdata_v2` 與保留當退路的舊 volume `socrates-ntnu_pgdata`。
- 刪除前要先告知試用負責人，並留下 `_sqlx_migrations` 與 `users` 的摘要以便核對。

## 管理者帳號與密碼

啟動時，`.env` 的 `ADMIN_EMAILS` 指定的信箱會建立管理者帳號，初始密碼來自 `ADMIN_INITIAL_PASSWORD`，且首次登入必須改密碼。

- `ADMIN_INITIAL_PASSWORD` 已於 2026-10-27 從 `.env` 移除。移除後不影響既有帳號；啟動時也不會重設既有帳號的密碼。
- 重建資料庫後，管理者帳號是以初始密碼建立的（`must_change_password = true`），**尚未改密碼**。需要由管理者登入後改密碼，見下一節。

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

NTNU 需要發表時可以開 `DEMO_MODE=true`（寫在未納入版控的 `docker-compose.ntnu.yml`），**2026-11-12 起已關閉**。注意它是**公開網址**：

- 示範教師與學生的密碼寫在原始碼裡、登入頁也會列出來，任何人都能用它們登入，看到所有示範資料。示範帳號不含管理者。
- 發表完成後把 `DEMO_MODE` 拿掉並重新部署：端點會回 404、登入頁不再列出帳號，但既有的示範帳號仍帶著公開密碼。要停用就把它們停用或刪除（管理者介面），或乾脆重建資料庫（先告知試用負責人）。
- `scripts/seed-demo.sh` 會刪除並重建 `[demo]` 開頭的示範教室、活動與對話，不會碰其他資料。它的預設資料庫指令 `DEMO_PSQL` 是對目前目錄的 compose 專案（開發用的 `socrates-chat`）執行，對 NTNU 必須明確指定：

```bash
DEMO_PSQL='docker compose -p socrates-ntnu -f docker-compose.yml -f docker-compose.ntnu.yml exec -T db psql -U socrates -d socrates' ./scripts/seed-demo.sh
```

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
| 現況：本機部署加 Tailscale Funnel | 免費 | 低（同機） | 是 | **維持** |
| 本機部署加 Cloudflare Tunnel | 免費，加網域約 10 美元／年 | 低（同機；入口走最近的 Cloudflare 機房，未實測） | 是 | 想要穩定網址或 WAF 時採用；Funnel 與 Tunnel 可並行，確認新網址正常再關 Funnel |
| Docker cloud sandbox 當正式環境 | 按秒計費（單價文件未載明） | 高，170–200 ms | 是 | 不採用 |
| Cloudflare Containers，DB 經 Tunnel 回本機 | 約 9.6–21 美元／月 | 未知，要實測 | 是 | 暫不採用 |
| 後端改寫成 Worker 加 Workers Paid，資料庫經 Hyperdrive 回本機 | 約 5 美元／月（加網域約 10 美元／年），另加數週的重寫成本 | 未知，要實測 | 是 | 不採用；要先走規格決策 |
| 全搬雲端（app 與託管 PostgreSQL） | 另計 | 低 | 否 | 要等 S-09 決定 DB 方案，再評估 |

重新評估的條件：S-09 決定正式環境的 DB 方案，或需要不依賴這台電腦的可用性。

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
| 待辦 | 管理者改掉初始密碼（`must_change_password` 仍為 true）。 |
| 待辦 | 部署含 `20261107000000` 的版本前，依「部署前檢查」備份並演練；合併 `origin/main` 前先處理 NTNU 分支上尚未進 main 的 commit。 |
| 待辦 | 維運負責人決定 NTNU 是否改為「部署前單獨執行 migrate」、以及是否採用每日加密備份與還原演練。 |
