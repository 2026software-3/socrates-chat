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
- `CASCADE_REALTIME_URL`、`CASCADE_LLM_TOKEN` 都設了才啟用，缺任何一個，對話詳情的 `voice_realtime` 是 `false`，`/voice` 回 `503 voice_unavailable`。只設網址會讓學生按得到即時語音，但 cascade 呼叫內部 LLM 端點被拒絕而等不到回覆，所以兩者綁在一起。
- `CASCADE_LLM_TOKEN` 是部署方自己產生的共用密碼（例如 `openssl rand -hex 32`），不是 OpenAI 金鑰；同一個值要同時是後端的環境變數 `CASCADE_LLM_TOKEN`，以及啟動 cascade 的行程的同名環境變數（`launch.yaml` 的 LLM 節點用 `api_key_env` 指向它，檔案裡不寫密碼），cascade 以 `Authorization: Bearer` 送給 `/internal/cascade/v1/*`。內部端點只認這個密鑰，不看 cookie；不得進版控。
- cascade 沒有逐會話傳身分的管道，由 Rust 把一次性代號放進 `instructions`，成為 system message 開頭，再由內部 LLM 端點解析。瀏覽器不能送 `session.update`，也收不到 `session.*`。
- OpenAI 收到的是專案自己的提示與資料庫歷史，cascade 的 system prompt 與歷史都不會送出。
- TTS 前要有轉接層：cascade 不送 `max_new_tokens`，也不做繁轉簡，直連會踩中暴走條件。

## cascade 設定

cascade 的 `launch.yaml` 不在 repo（內含本機路徑與參考音檔），範本是 [`scripts/cascade-launch.example.yaml`](../../scripts/cascade-launch.example.yaml)：複製後改成自己的環境。範本的結構與實測用的設定相同，差別只有：路徑與參考音三處換成佔位，`dialog` 不設 `system_prompt`（見下表），沒有密鑰。必要的欄位：

| 欄位 | 值 | 說明 |
| --- | --- | --- |
| 環境變數 `CASCADE_LLM_TOKEN`（cascade 行程） | 與後端相同的共用密碼 | 不寫在檔案裡；與後端的值不同時，內部 LLM 端點回 401，學生等不到回覆 |
| `session_nodes.llm.base_url` | 後端的 `/internal/cascade/v1` | 後端的 `/models` 回報的名稱是 `socrates`，`model` 填同名 |
| `session_nodes.llm.api_key_env` | `CASCADE_LLM_TOKEN` | 環境變數的名稱，不是密碼本身 |
| `stages.vad.factory.silence_duration_ms` | `1000` | 預設 160 ms 時學生一停頓就被切成新的一句 |
| `session_nodes.asr` | `kind: openai_transcription`、`language: zh`、`response_format: json` | `base_url` 指向轉接層，轉給 `scripts/whisper-server.py` |
| `session_nodes.tts` | `kind: openai_speech`、`language: Chinese`、`reference_audio_path`、`reference_text` | `base_url` 指向轉接層；參考音要自己準備且逐字稿一致，只限本機評估 |
| `session_nodes.dialog` | 不設 `system_prompt` | 後端建立每場語音時以 `instructions` 覆蓋它，AI 的提示與歷史由後端提供，這裡寫了也不會送給 OpenAI；留著人設文字只會誤導 |
| `session_nodes.aligner` | `method: sentence` | 打斷時裁切到句子邊界，`heard_chars` 靠它的進度 |

其餘節點與連線（`inputs`、`session_publish`）照範本，不要改。後端這一端要設 `CASCADE_REALTIME_URL`（cascade 的 `/v1/realtime`）與 `CASCADE_LLM_TOKEN`。

轉接層是 [`docker/cascade/tts_adapter.py`](../../docker/cascade/tts_adapter.py)（本機評估是 `127.0.0.1:8103`）：它把 `/v1/audio/transcriptions` 轉給 Whisper 服務、把 `/v1/audio/speech` 轉給 TTS 服務，並補上 `max_new_tokens` 與繁轉簡。

## 用 docker compose 啟動整套 cascade

`docker-compose.yml` 的 `cascade` profile 包含四個服務：`cascade-tts`（Qwen3-TTS 0.6B，使用 GPU）、`cascade-whisper`（CPU，small）、`cascade-adapter`（轉接層）、`cascade`（本體；LLM 回呼 `http://app:3000/internal/cascade/v1`）。**預設不啟用**，只在指定 profile 時才會建置與啟動，沒有 profile 的 `docker compose up` 與 CI 不受影響。

| 項目 | 設定 |
| --- | --- |
| 啟用 | `COMPOSE_PROFILES=cascade`，或 `docker compose --profile app --profile cascade up -d --build` |
| 後端環境變數 | `CASCADE_REALTIME_URL=ws://cascade:8110/v1/realtime`、`CASCADE_LLM_TOKEN`（與 cascade 容器同一個值，compose 會從 `.env` 帶入） |
| 參考音 | `CASCADE_REFS_DIR`（預設 `./cascade-refs`）下放 `reference.wav`，逐字稿設在 `CASCADE_REFERENCE_TEXT`；參考音不進版控 |
| 模型快取 | 預設存在 `cascade_hf` volume（首次啟動下載）；`CASCADE_HF_CACHE=~/.cache/huggingface` 可重用主機已下載的模型 |
| 需求 | NVIDIA GPU（TTS 約 5 GB 顯存）與 NVIDIA Container Toolkit；服務之間走 compose 內部網路，不對主機開 port |

映像固定 `sglang-omni==0.2.0`（實測版本），CUDA 13 開發版基底。cascade 與 TTS 沒有 GPU 時無法啟動，所以不要在沒有 GPU 的環境啟用這個 profile。

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
- **打斷與歷史：** cascade 的 `sglang.response.heard` 只到整句，後端用 `sglang.response.progress` 與瀏覽器回報的播放位置在句內按比例推算，還沒有進度點時以每秒 3.5 字估算，少於 4 字算沒聽到；沒有聲音就被取消則算 0。結果存進 `messages.heard_chars`，`content` 留完整回覆供稽核。AI 的上下文、對話 API 與總結都只用聽到的部分，一個字都沒聽到的回覆不算存在。打斷的當下畫面仍可能比聽到的多，重新載入後才一致。階段判斷與用量仍照完整回覆計算。
- **回合失敗：** 回覆沒有存成或學生訊息存不進去時，後端送 `socrates.turn.failed`（不經過 cascade）；瀏覽器結束語音，這一輪標成可重試。語音回合在交易內拒絕疊在未回覆訊息上（`unanswered_message`）。學生訊息本身沒存到時，畫面上的這句話在重新載入後會消失。
- **併句：** 回覆一個字都沒被聽到時，cascade 把前後兩句學生發言併成一則。內部 LLM 端點依學生訊息數與開頭比對：回覆完全沒被聽到就把接著說的話併回上一則學生訊息，回合數不變；學生聽到一部分時則是新的一則。比對基準只在保存成功後更新，同一場的請求逐一處理。這依賴 cascade 目前的併句格式（以空格連接）。畫面上兩段在語音進行中仍分開顯示，重新載入後才併成一則。
- **斷句：** `launch.yaml` 的 VAD 靜音門檻設 1000 ms；原本 160 ms 時學生一停頓就被切成新的一句。
- **幻聽：** `whisper-server.py` 丟掉 `no_speech_prob` ≥ 0.6 或 `avg_logprob` < -1 的片段，並刪除影片片尾套語。門檻取自 Whisper 的慣例，尚未用真人語料校準。
- **容錯：** ASR 回 500 時 cascade 直接關閉整個會話，沒有重試。
- **啟動順序：** cascade 啟動時探測 LLM，Rust 要先就緒。
- **繁簡：** `whisper-server.py` 把中文轉成臺灣繁體（OpenCC `s2tw`）。轉換只是字詞對應，不校正辨識錯字；改用其他 ASR 服務時要各自處理。
- **容量：** 一台 6 GB 只能驗證可行性；多人同時使用未測，TTS 沒有全站併發上限。
- **聲音：** 參考音只供本機評估，不能作為正式聲音。
- **其他語言：** 只測了中文。英文、西文未測。

## 待需求方決定

是否正式以伺服器端語音取代瀏覽器端（S-05 與 S-08 邊界）、部署與 GPU 規格、正式聲音與模型。
