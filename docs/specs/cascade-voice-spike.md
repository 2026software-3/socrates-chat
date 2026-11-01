# 伺服器端語音對話：接入 sglang-omni Streaming Cascade

記錄 2026-11-01 在 RTX 4050 6 GB、WSL2 上的實測與前端行為。需求方已先行接受音訊上傳（S-05.5，暫定）；功能預設關閉，部署方設定後才對 `zh-TW` 對話開放。

## 架構

```text
瀏覽器 ── WS /api/conversations/{id}/voice ── Rust ── WS ── cascade（VAD、斷句、打斷）
                                               ▲             │
             現有回覆流程（用量、提示、遮蔽、存檔）│             ├─ ASR：CPU whisper-small
                                               └─ 內部 LLM 端點 └─ TTS：轉接層 → Qwen3-TTS 0.6B（GPU）
```

- 瀏覽器只連 Rust，不直連 cascade 或 AI 服務。Rust 檢查登入、對話擁有者與 Origin，只放行音訊相關事件。
- `CASCADE_REALTIME_URL`、`CASCADE_LLM_TOKEN` 都設了才啟用；內部端點只認共用密鑰，不看 cookie。
- cascade 沒有逐會話傳身分的管道，由 Rust 把一次性代號放進 `instructions`，成為 system message 開頭，再由內部 LLM 端點解析。瀏覽器不能送 `session.update`，也收不到 `session.*`。
- OpenAI 收到的是專案自己的提示與資料庫歷史，cascade 的 system prompt 與歷史都不會送出。
- TTS 前要有轉接層：cascade 不送 `max_new_tokens`，也不做繁轉簡，直連會踩中暴走條件。

## 前端行為

`frontend/src/features/chat/voice/realtime/`：

| 模組 | 職責 |
| --- | --- |
| `pcm.ts` | 降到 16 kHz、PCM 與 base64 互轉 |
| `pcm-player.ts` | 24 kHz PCM 接續排程到 Web Audio，記錄實際唸出的時間 |
| `realtime-session.ts` | 一場連線：麥克風、WebSocket、播放與事件處理，擁有全部資源 |
| `voice-turn.ts` | 把語音回合接進對話串，adapter 不再 POST |
| `use-realtime-voice.ts` | 把連線接到 React |

- **何時啟用**：`GET /api/conversations/{id}` 的 `voice_realtime` 為真，而且對話語言是 `zh-TW`。按「即時」才開麥克風與連線；回覆還在產生或最後一則學生訊息尚未回覆時不能開始，要先處理那則訊息。瀏覽器讀不到 WebSocket 升級失敗的原因，連不上或 10 秒沒有回應都視為無法使用，改用瀏覽器語音並提示，這次頁面不再嘗試。
- **回合**：開口、斷句都由 cascade 決定，沒有 S-05.2 的 3 秒倒數；要結束時按「結束」。連線期間鎖住文字輸入，避免和語音回合交錯。
- **開口打斷開關**：沿用 S-05.5 的開關，預設關閉。關閉時，從回覆開始到唸完為止上傳靜音而不是停止上傳：cascade 的語音偵測只看收到的音訊，而且序號必須連續，停送會讓這一輪卡住。開啟時持續上傳，由 cascade 偵測開口並自動取消。
- **打斷**：收到 `speech_started`，或學生按「說話」時，立刻清空本機播放，並以 `conversation.item.truncate` 回報實際唸出的毫秒數。手動打斷另外送 `response.cancel`。之後遲到的音訊與文字因回覆已中止而丟棄。
- **唸完才算結束**：cascade 約領先播放 1 秒，文字更早，所以回到聆聽要等 `response.done` 而且本機播完。之後重新讀取對話，同步階段、建議收斂與是否已結束。
- **畫面文字**：顯示 cascade 收到的回覆文字；打斷時可能比學生聽到的多。重新整理後顯示資料庫的完整回覆。

## 實測結果

都使用合成語音；LLM 為假回覆。

| 項目 | 結果 |
| --- | --- |
| 完整鏈路 | 通過，含真的 Rust、WebSocket 代理與 cascade |
| GPU | 待機 4994 MiB，峰值 5512 / 6141 MiB |
| 說完到第一段音訊 | 約 2.9 秒：斷句 0.9、CPU 辨識 1.4、其餘 0.6（不含真實 OpenAI 延遲） |
| 打斷 | 再次開口後約 0.22 秒取消回覆 |
| 歷史裁切 | 38 字回覆打斷後保留聽到的 23 字；第一句沒唸完時整則不留。以句子為單位 |
| 繁體回覆經轉接層 | 測試中沒有暴走，樣本只有幾輪，不足以下結論 |

## 已知問題

- **音訊保存：** 應用程式只轉送不落地；S-05.4 的不保存還要查證 cascade 內部。
- **打斷與歷史：** cascade 回報學生聽到的字元數（`sglang.response.heard`，沒有聲音就被取消則算 0），後端存進 `messages.heard_chars`，`content` 留完整回覆供稽核。AI 的上下文、對話 API 與總結都只用聽到的部分，一個字都沒聽到的回覆不算存在。打斷的當下畫面仍可能比聽到的多，重新載入後才一致。階段判斷與用量仍照完整回覆計算。
- **回合失敗：** 回覆沒有存成或學生訊息存不進去時，後端送 `socrates.turn.failed`（不經過 cascade）；瀏覽器結束語音，這一輪標成可重試。語音回合在交易內拒絕疊在未回覆訊息上（`unanswered_message`）。學生訊息本身沒存到時，畫面上的這句話在重新載入後會消失。
- **併句：** 回覆一個字都沒被聽到時，cascade 把前後兩句學生發言併成一則。內部 LLM 端點依學生訊息數與開頭比對，只存新的部分；比對基準只在保存成功後更新，同一場的請求逐一處理。這依賴 cascade 目前的併句格式（以空格連接）。
- **容錯：** ASR 回 500 時 cascade 直接關閉整個會話，沒有重試。
- **啟動順序：** cascade 啟動時探測 LLM，Rust 要先就緒。
- **繁簡：** `whisper-server.py` 把中文轉成臺灣繁體（OpenCC `s2tw`）。轉換只是字詞對應，不校正辨識錯字；改用其他 ASR 服務時要各自處理。
- **容量：** 一台 6 GB 只能驗證可行性；多人同時使用未測，TTS 沒有全站併發上限。
- **聲音：** 參考音只供本機評估，不能作為正式聲音。
- **其他語言：** 只測了中文。英文、西文未測。

## 待需求方決定

是否正式以伺服器端語音取代瀏覽器端（S-05 與 S-08 邊界）、部署與 GPU 規格、正式聲音與模型。
