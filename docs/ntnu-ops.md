# NTNU 試用環境維運說明

本文件只描述在 NTNU 試用機器上的維運做法。部署設定檔 `docker-compose.ntnu.yml` 與 `scripts/deploy-ntnu.sh` 不進版控，只存在這台機器上。

**NTNU 自 2026-11-07 起視為正式環境**（[S-09.3](specs/S-09.3-ops-procedures.md)）：資料要保留，migration 只往前（已套用的 migration 檔不能改，改 schema 一律新增 migration），不再重建資料庫。這不代表正式環境的部署方式已定案（S-09.1 仍待決定）。

## 基本資訊

- Compose 專案名稱：`socrates-ntnu`（與開發用的 `socrates-chat` 分開）。
- 設定檔：`docker-compose.yml` + `docker-compose.ntnu.yml`，所有 compose 指令都要帶上 `-f` 與 `-p socrates-ntnu`。
- 公開網址：`https://ray-nb.tail5fc9fe.ts.net`（Windows 端 Tailscale Funnel，見下一節）。這個值由 `docker-compose.ntnu.yml` 設定在容器的 `APP_BASE_URL`。
- `.env` 的 `APP_BASE_URL` 是 `http://localhost:3000`，但容器實際使用的是覆寫後的公開網址。CSRF 檢查要求 `Origin` 標頭等於容器的 `APP_BASE_URL`，所以用 curl 或腳本呼叫 API 時，`Origin` 要填公開網址，不能填 `.env` 的值。

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

## 刪除資料庫的限制

- NTNU 是正式環境。不要對 `socrates-ntnu` 執行 `down -v`，除非試用負責人明確同意刪除資料。它會刪掉 `socrates-ntnu_pgdata`。
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
| 待辦 | 管理者改掉初始密碼（`must_change_password` 仍為 true）。 |
| 待辦 | 部署含 `20261107000000` 的版本前，依「部署前檢查」備份並演練；合併 `origin/main` 前先處理 NTNU 分支上尚未進 main 的 commit。 |
| 待辦 | 維運負責人決定 NTNU 是否改為「部署前單獨執行 migrate」、以及是否採用每日加密備份與還原演練。 |
