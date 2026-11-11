# NTNU 試用環境維運說明

本文件只描述在 NTNU 試用機器上的維運做法。部署設定檔 `docker-compose.ntnu.yml` 與 `scripts/deploy-ntnu.sh` 不進版控，只存在這台機器上。

**NTNU 自 2026-11-07 起視為正式環境**（[S-09.3](specs/S-09.3-ops-procedures.md)）：資料要保留，migration 只往前（已套用的 migration 檔不能改，改 schema 一律新增 migration），不再重建資料庫。這不代表正式環境的部署方式已定案（S-09.1 仍待決定）。

## 基本資訊

- Compose 專案名稱：`socrates-ntnu`（與開發用的 `socrates-chat` 分開）。
- 資料庫資料放在 **D 槽**：`D:\socrates-ntnu\pgdata`（WSL 內是 `/mnt/d/socrates-ntnu/pgdata`），由 `docker-compose.ntnu.yml` 的 `db.volumes` 以 bind mount 掛入，不再使用 volume `socrates-ntnu_pgdata`（見下一節）。
- 設定檔：`docker-compose.yml` + `docker-compose.ntnu.yml`，所有 compose 指令都要帶上 `-f` 與 `-p socrates-ntnu`。
- 公開網址：`https://ray-nb.tail5fc9fe.ts.net`（Windows 端 Tailscale Funnel，見下一節）。這個值由 `docker-compose.ntnu.yml` 設定在容器的 `APP_BASE_URL`。
- `.env` 的 `APP_BASE_URL` 是 `http://localhost:3000`，但容器實際使用的是覆寫後的公開網址。CSRF 檢查要求 `Origin` 標頭等於容器的 `APP_BASE_URL`，所以用 curl 或腳本呼叫 API 時，`Origin` 要填公開網址，不能填 `.env` 的值。

## 資料庫位置（D 槽）

2026-11-10 起，`db` 的資料目錄是 D 槽的 `D:\socrates-ntnu\pgdata`。`docker-compose.ntnu.yml`（不進版控）的 `db` 以 `!override` 覆寫掉 `docker-compose.yml` 的 `pgdata` volume：

```yaml
    volumes: !override
      - /run/desktop/mnt/host/d/socrates-ntnu/pgdata:/var/lib/postgresql/data
```

- **來源要寫 `/run/desktop/mnt/host/d/...`，不要寫 `/mnt/d/...`。** 後者是 WSL 路徑，Docker Desktop 要等 `docker-desktop-bind-mounts` 的掛載點建好才掛得上；WSL 重開機後 engine 先自動重啟容器，掛載點還沒好就失敗，容器不會重試（`app` 的 `.apikeylist` 曾因此 `Exited (127)`）。`/run/desktop/mnt/host/d` 是 Docker Desktop 自己的 D 槽掛載點，不受這個時序影響。
- 目錄擁有者必須是 `999:999`、權限 `700`（Postgres 的要求）；Docker Desktop 的 D 槽掛載會保留這兩項。不要在 Windows 檔案總管或 WSL 裡直接改 `pgdata` 內的檔案。
- 備份、`pg_dump`、`docker exec ... psql` 的指令不變。`D:\socrates-ntnu\pgdata` 是 Postgres 的原始資料目錄，**不是備份**：D 槽壞掉或誤刪就沒了，備份仍要另外做。
- 舊的 volume `socrates-ntnu_pgdata` 保留了搬移當時（2026-11-10 13:53）的完整資料，目前沒有容器使用，只當退路。確認 D 槽版本穩定運作一段時間後，經試用負責人同意再刪（`docker volume rm socrates-ntnu_pgdata`）。退回舊 volume 的做法：把 `docker-compose.ntnu.yml` 的 `db.volumes` 覆寫拿掉，重建 `db`；但搬移之後寫入的資料只在 D 槽，退回就會遺失，所以退回前要先 `pg_dump`。

### 把資料庫搬到別的位置（2026-11-10 的做法，可重複）

1. 依「部署前檢查」做 `pg_dump` 備份。
2. `docker stop socrates-ntnu-app-1`，再 `docker stop -t 60 socrates-ntnu-db-1`（讓 Postgres 乾淨關閉）。
3. 複製資料，保留擁有者與權限，並核對檔案數量一致：

   ```bash
   docker run --rm -v socrates-ntnu_pgdata:/from:ro -v /run/desktop/mnt/host/d/socrates-ntnu:/to alpine \
     sh -c 'mkdir -p /to/pgdata && cp -a /from/. /to/pgdata/ && echo copied'
   ```

4. 改 `docker-compose.ntnu.yml` 的 `db.volumes`，`docker compose -f docker-compose.yml -f docker-compose.ntnu.yml --profile app --profile cascade up -d --no-deps --no-build --force-recreate db`，等 `db` healthy，比對 `users`、`conversations`、`_sqlx_migrations` 的筆數。
5. `docker start socrates-ntnu-app-1`，確認 `/health` 與 `/ready` 回 200。全程約 1 分鐘的中斷。

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
- 如果 `app` 正常、`cascade-1` 仍重啟，日誌是 `找不到 /refs/reference.wav`：同樣是 Docker Desktop 的 bind mount 失效（檔案本身在 `CASCADE_REFS_DIR`，通常沒掉）。重建 `cascade`（要帶 `--profile cascade`）：

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

- NTNU 是正式環境。不要對 `socrates-ntnu` 執行 `down -v`，除非試用負責人明確同意刪除資料。資料庫現在在 D 槽的 bind mount，`down -v` 不會刪它，但會刪掉保留當退路的舊 volume `socrates-ntnu_pgdata`。要刪資料庫本身得刪 `D:\socrates-ntnu\pgdata`，同樣要先取得試用負責人同意。
- 刪除前要先告知試用負責人，並留下 `_sqlx_migrations` 與 `users` 的摘要以便核對。

## 管理者帳號與密碼

啟動時，`.env` 的 `ADMIN_EMAILS` 指定的信箱會建立管理者帳號，初始密碼來自 `ADMIN_INITIAL_PASSWORD`，且首次登入必須改密碼。

- `ADMIN_INITIAL_PASSWORD` 已於 2026-10-27 從 `.env` 移除。移除後不影響既有帳號；啟動時也不會重設既有帳號的密碼。
- 重建資料庫後，管理者帳號是以初始密碼建立的（`must_change_password = true`），**尚未改密碼**。需要由管理者登入後改密碼，見下一節。

### 更改自己的密碼（已登入）

- 登入後，網頁會要求先更改密碼。
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

NTNU 目前可以開 `DEMO_MODE=true`（寫在未納入版控的 `docker-compose.ntnu.yml`）供發表使用。注意它是**公開網址**：

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
| 2026-11-08 | WSL 重開機後 `app`（`Exited (127)`）與 `cascade-1`（找不到 `/refs/reference.wav`）都因 bind mount 失效起不來，重建兩者後恢復（`/ready` 回 200，cascade 與 cascade-tts 皆 healthy）。 |
| 2026-11-08 | 以手機行動網路經 Funnel 故意登入失敗，`login_failures` 記到手機的真實 IPv6 `/64` 網段，確認 `TRUSTED_PROXIES` 在新入口下正常；測試紀錄已刪除。 |
| 2026-11-10 | 資料庫由 volume `socrates-ntnu_pgdata` 搬到 D 槽 `D:\socrates-ntnu\pgdata`（bind mount，來源用 `/run/desktop/mnt/host/d/...`）。搬前 `pg_dump` 備份；搬後 `users` 40、`conversations` 66、`_sqlx_migrations` 14 筆與搬前一致，`/health`、`/ready` 回 200，公開網址 `/ready` 回 200。中斷約 1 分鐘。舊 volume 保留當退路。 |
| 2026-11-10 | 評估 app 放雲端（DB 留 D 槽、cascade 留本機）：Docker cloud sandbox 的 UDP 不通、位在美西、24 小時過期，經 Tailscale 只能走 DERP，往返 170–200 ms；Cloudflare Containers 約 9.6–21 美元／月且仍需把 DB 接回本機。結論：NTNU 維持本機部署，見「雲端部署評估」。 |
| 2026-11-10 | 補充評估「後端改寫成 Worker 加 Workers Paid」：約 5 美元／月，但後端約 15,440 行、74 個端點、180 處 SQLx 查詢要重寫，資料庫仍在本機、可用性沒改善，且需先改規格。結論：不採用。 |
| 2026-11-11 | 部署 PR #509（`perf/chat-merge-queries`，cherry-pick `7c8eb51`、`c64fb21`、`ba1728c`）：送出訊息與保存 AI 回覆合併查詢，每則文字訊息的資料庫往返 13 → 10 次（教室對話），沒有 migration。部署前在 deploy 樹跑 sbx-test：750 通過、0 失敗；`pg_dump` 備份 `~/ntnu-backup-2026-11-11.sql`。只重建 `app`，`/health`、`/ready`（本機與公開網址）皆正常，`users` 40、`conversations` 70、`messages` 631、`_sqlx_migrations` 14 筆與部署前一致。舊映像留為 `socrates-ntnu-app:pre-chat-merge-20261111`，退回時把它改標回 `latest` 再以 `--no-build --force-recreate app` 重建。 |
| 待辦 | 管理者改掉初始密碼（`must_change_password` 仍為 true）。 |
| 待辦 | 部署含 `20261107000000` 的版本前，依「部署前檢查」備份並演練；合併 `origin/main` 前先處理 NTNU 分支上尚未進 main 的 commit。 |
| 待辦 | 維運負責人決定 NTNU 是否改為「部署前單獨執行 migrate」、以及是否採用每日加密備份與還原演練。 |
