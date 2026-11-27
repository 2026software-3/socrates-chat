# 後端測試的偶發失敗寫法檢查

`backend/tests/**/*.rs` 裡幾種「機器一慢就偶發失敗」的寫法，由 `lint.mjs` 在 CI 擋下（只用 Node 內建模組，幾秒內跑完）。

```bash
node scripts/test-flake-lint/lint.mjs                      # 檢查
node scripts/test-flake-lint/lint.mjs --update-baseline    # 重新產生基準檔（只在修掉違規後用）
node --test scripts/test-flake-lint/lint.test.mjs          # 這支檢查本身的測試
```

## 規則

| 規則 | 擋什麼 | 該怎麼寫 |
| --- | --- | --- |
| `thread-sleep` | `thread::sleep(...)` | 注入時鐘；或把時間拉長到不受排程影響；或輪詢實際狀態（有上限、逾時要失敗） |
| `long-sleep` | `sleep(Duration::from_millis(N))`，N ≥ 100，或 `from_secs` | 等明確的事件：`wait_until_ai_called`、`wait_until_background_idle`、`wait_until_blocked_on_lock`，或輪詢實際結果。100ms 以下的睡眠視為輪詢間隔，不擋 |
| `elapsed` | `.elapsed()` | 驗證行為而不是時間；真的要量時間，上限要留足餘裕 |
| `pool-acquire-count` | `before_acquire(`（數連線池取用次數） | 背景任務也會取用連線池，計數會被污染；重複量測取最小值，或改驗行為 |

只擋「寫法」，擋不到的偶發問題（例如共用狀態、埠衝突）要靠 code review。

## `flaky-ok`

確定這行沒有問題時，在**該行尾端或緊接的上一行**加註解，**理由必填**：

```rust
// flaky-ok: 假伺服器故意晚送錯誤，不是測試在等時間
tokio::time::sleep(Duration::from_millis(300)).await;
```

只覆蓋那一行。理由要說明為什麼機器變慢也不會誤報失敗（例如「只檢查下限」「餘裕以數量級計」）。

## 基準檔

`baseline.txt` 放歷史違規（`檔案<TAB>規則<TAB>該行內容`，不含行號，所以搬動行號不影響）。規則是：

- 新增的違規一律失敗，不能往基準檔加。
- 修掉基準檔裡的項目後，CI 會要求把它移除（`--update-baseline`），所以基準檔只會越來越少。

目前基準檔是空的。
