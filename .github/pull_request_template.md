## 對應 Issue

Closes #

## 改動摘要

-

## 明確不包含

-

## 測試

- [ ] 先寫會失敗的測試，再做最小實作
- [ ] 新增／修改的測試全部通過（請寫出執行的指令）

```bash

```

## 前端與設計（沒有改 `frontend/` 畫面可刪除本段）

- Figma 節點：<!-- 例：https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat?node-id=21-241，節點見 docs/design/figma-map.md -->

截圖取自 `frontend/tests/e2e/__screenshots__/`（`npm run test:e2e:linux` 產生）：

| 375 | 768 | 1440 |
| --- | --- | --- |
| <!-- 截圖 --> | <!-- 截圖 --> | <!-- 截圖 --> |

- axe 結果：<!-- 例：serious／critical 0 件（npm run test:e2e:linux -- tests/e2e/student-home.spec.ts）；minor／moderate 有的話列出 -->

- [ ] 版面、文字、狀態與 Figma 節點一致；動畫與材質依 HTML 原型（`DESIGN.md`「設計依據分工」）
- [ ] 只用 `tokens.css` 的 token、`type-*` 文字樣式與 `components/vedute` 元件；沒有原始色碼或任意值，沒有手改 `components/ui/`
- [ ] 載入狀態用 skeleton；錯誤與空資料狀態都有畫面
- [ ] Playwright 點擊測試涵蓋主要操作，axe 沒有 serious、critical 違規；基準截圖有意改變時已說明
- [ ] 三種語言的翻譯都已加上

## 規格

- [ ] 這個 PR 沒有改變任何規則；**或**已同步更新 `docs/specs/`（`docs/specs/` 是規則的唯一來源）
- [ ] 若這是規格決策 PR：已更新 `docs/specs/decisions.md`（必要時另寫詳細規格），並在上方以 `Closes #` 關聯規格 Issue

## 資安與個資

- [ ] 沒有加入金鑰、憑證、`.env` 或真實學生資料（測試只用合成資料）
- [ ] 日誌與錯誤回應不含對話原文、token、電子郵件等敏感內容
- [ ] 權限相關改動有對應的授權測試

## Commit

- [ ] 所有 commit 符合 [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
