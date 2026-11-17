# 摘要論證樹

## 網頁使用

完成後端 migration 後啟動既有前後端。學生打開自己的已結束對話，在 AI 學習總結下方點「總結論證樹」。首次產生並保存，再次打開直接讀取；視窗內可展開論點、來源引文與 JSON。失敗可重試。

服務端沿用 OPENAI_MODEL、OPENAI_BASE_URL、金鑰清單與推理強度，服務必須支援 Responses API 及 strict JSON Schema。CLI 的 ANNOTATION_* 不覆寫服務端模型。

API 為 GET／POST /api/conversations/{id}/argument-tree，前端不送逐字稿。權限、保存與失敗處理見 [摘要論證樹規格](../../../docs/specs/summary-argument-tree.md)。此按需摘要樹未取代 S-04.4 多對多論證圖。

## 檢查

在 backend/ 執行：

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked main_argument_tree --lib
cargo test --locked --test argument_tree --test argument_tree_provider
```

整合測試需要 PostgreSQL 與 DATABASE_URL。全部為合成資料，不含私人 T01 評測。

## CLI 離線重播

不需 API 金鑰，在 backend/ 執行：

```bash
OUT="$(mktemp -d)"
cargo run --locked --example try_main_argument_tree -- --replay \
  eval/main_argument_tree/fixtures/synthetic_transcript.json \
  eval/main_argument_tree/fixtures/synthetic_outline.json \
  "$OUT/tree.json" "$OUT/tree.html"
```

線上 CLI：

```bash
export ANNOTATION_MODEL='可用且支援 Responses 的模型 ID'
export OPENAI_API_KEYS_FILE='/path/to/.apikeylist'
OUT="$(mktemp -d)"
cargo run --locked --example try_main_argument_tree -- \
  /path/to/transcript.json "$OUT/outline.json" "$OUT/tree.json" "$OUT/tree.html"
```

目錄需先存在，輸出檔案不得已存在，CLI 不覆寫既有檔案。CLI 讀取 process 環境，不自動載入 .env。可設定 ANNOTATION_BASE_URL、ANNOTATION_TIMEOUT_SECS、ANNOTATION_REASONING_EFFORT；推理強度未設定時繼承 OPENAI_REASONING_EFFORT，空字串停用繼承，支援值依模型而定。

## 輸入、遮蔽與驗證

- 接受 topic_id/motion/rounds，以及對話 API 的 title/messages 扁平物件；也接受教師逐字稿 conversation_id 作識別碼，但網頁 API 只允許學生自己的對話。
- 按伺服器 seq 順序處理，student/user 連續發言以換行合併，不跨 ai/assistant，不去重。第一輪可無問題；末尾未回答 AI 不增加輪次。round/source_round 是分析輪次，不等於 turn_count。
- 原有 rounds 不合併。source_message_indices 保存 messages 的 1-based 位置，JSON 直接位於每個 round 下（Rust 內部存於 extra），不傳模型。
- 先解析 JSON，再遮蔽 motion、問答與 metadata 字串，最後才序列化；結構識別碼不替換，應使用不含個資的 opaque ID。沿用 email、身分證、學號、電話規則，不能辨識所有姓名或自然語言個資。
- 模型與引文驗證使用相同遮蔽後文字，只送題目、問答與 question_type。後者是 AI 追問策略，不是節點種類。
- 驗證根、父節點、循環、輪次、逐字引文與種類／關係配對。一般語意重複與支持／反駁是否恰當仍須人工評估。
- 提示版本 main-argument-tree-precision-v2.1.1；單次模型呼叫，不自動追加模型修訂。
