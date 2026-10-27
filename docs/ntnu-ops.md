# NTNU 試用環境維運說明

本文件只描述在 NTNU 試用機器上的維運做法。部署設定檔 `docker-compose.ntnu.yml` 與 `scripts/deploy-ntnu.sh` 不進版控，只存在這台機器上。

## 基本資訊

- Compose 專案名稱：`socrates-ntnu`（與開發用的 `socrates-chat` 分開）。
- 設定檔：`docker-compose.yml` + `docker-compose.ntnu.yml`，所有 compose 指令都要帶上 `-f` 與 `-p socrates-ntnu`。
- 公開網址：`https://raynb.tail5fc9fe.ts.net`（Tailscale Funnel）。這個值由 `docker-compose.ntnu.yml` 設定在容器的 `APP_BASE_URL`。
- `.env` 的 `APP_BASE_URL` 是 `http://localhost:3000`，但容器實際使用的是覆寫後的公開網址。CSRF 檢查要求 `Origin` 標頭等於容器的 `APP_BASE_URL`，所以用 curl 或腳本呼叫 API 時，`Origin` 要填公開網址，不能填 `.env` 的值。

## 部署

```bash
SKIP_PULL=1 ./scripts/deploy-ntnu.sh > deploy.log 2>&1; echo "exit=$?"
```

- 不要把腳本接在管線後面（例如 `| tail`），管線會遮蔽失敗的退出碼。
- `exit=0` 且 `/ready` 回 200 才算部署成功；`exit=1` 時，`deploy.log` 最後會有 app 最近 30 行日誌。
- 日誌出現 `failed to run migrations` 時，表示資料庫的 migration 紀錄與程式不一致。先比對 `_sqlx_migrations` 的 checksum，不要直接 `down -v`。

## 刪除資料庫的限制

- 不要對 `socrates-ntnu` 執行 `down -v`，除非已確認要刪除試用資料。它會刪掉 `socrates-ntnu_pgdata`。
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

## 不要用 seed-demo.sh 操作 NTNU

`scripts/seed-demo.sh` 是給本機開發與試用用的，不要在 NTNU 上執行：

- 它會把示範帳號（`teacher@example.com`、`student1`～`student8@example.com`）重設成固定密碼。
- 它會刪除並重建示範教室、活動與對話。
- 它的預設資料庫指令 `DEMO_PSQL` 是 `docker compose exec -T db ...`，沒有指定專案時會對目前目錄的 compose 專案執行，也就是開發用的 `socrates-chat`。

## 狀態紀錄

| 日期 | 事件 |
| --- | --- |
| 2026-10-27 | 重建 `socrates-ntnu` 資料庫並重新部署（`/ready` 回 200，8 個 migration 全部成功）。以 `.env` 的 `ADMIN_EMAILS` 建立管理者帳號，登入測試通過。 |
| 2026-10-27 | 從 `.env` 移除 `ADMIN_INITIAL_PASSWORD`。 |
| 待辦 | 管理者改掉初始密碼（`must_change_password` 仍為 true）。 |
