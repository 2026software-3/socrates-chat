# Whisper base 語音辨識效能紀錄

試用自架 Whisper base 取代 OpenAI 語音辨識的量測紀錄（2026-10-26）。**這是量測結果，不是規格決策**：具體採用哪個辨識模型仍在 `open-decisions.md`，未定案。

## 環境

- GPU：NVIDIA GeForce RTX 4050 Laptop（6 GB），WSL2；無任何負載時已佔 221 MiB（顯示用，下表「增量」已扣除）
- faster-whisper 1.2.1（CTranslate2 4.8.4），模型 `base`（約 74M 參數），beam_size 5，指定語言
- 測試音檔為合成語音（edge-tts 產生，不含真實學生資料）：中、英各一段 4 秒與一段約 55 秒
- 後端接法：`OPENAI_STT_BASE_URL=http://127.0.0.1:8000/v1`、`OPENAI_STT_MODEL=whisper-base`，服務見 `scripts/whisper-server.py`

## GPU 記憶體

| 設定 | 載入後增量 | 辨識時尖峰增量（長音檔 55 秒） | 載入時間 | 主機 RAM |
| --- | --- | --- | --- | --- |
| GPU float16 | 約 273 MiB | 約 387 MiB | 1.2 s | 約 820 MiB |
| GPU int8_float16 | 約 209 MiB | 約 291 MiB | 0.7 s | 約 820 MiB |
| CPU int8 | 0 | 0 | 0.7 s | 約 440 MiB |

經 HTTP 服務、8 路並行時整體尖峰約 640 MiB（含 221 MiB 底噪），顯存幾乎不隨並行數成長。

## 速度（暖機後，秒；RTF = 耗時／音檔長度，越小越快）

| 音檔 | GPU float16 | GPU int8_float16 | CPU int8 |
| --- | --- | --- | --- |
| 中文 4.3 s | 0.14 s（RTF 0.033） | 0.14 s | 0.40 s（RTF 0.093） |
| 中文 57.6 s | 1.05 s（RTF 0.018） | 1.08 s | 2.17 s（RTF 0.038） |
| 英文 4.0 s | 0.13 s | 0.13 s | 0.37 s |
| 英文 54.8 s | 0.80 s | 0.87 s | 1.50 s |

經 HTTP 服務（webm/opus 上傳）：57 秒音檔單一請求約 1.3 s。並行請求在單張 GPU 上排隊執行：

| 並行數 | 57 秒音檔，整體耗時 | 4 秒音檔，整體耗時 |
| --- | --- | --- |
| 1 | 1.3 s | — |
| 4 | 5.2 s | — |
| 8 | 11.7 s | 1.3 s |

## 辨識品質（僅供參考，樣本很小）

- 英文兩段逐字正確。
- 中文短句正確；長句有專有詞錯誤：「未經檢視」被辨成「為金箭式」（float16）、「為經檢室」（int8_float16）。哲學術語密度高的課堂內容可能需要更大的模型（small／medium），或在提示詞（`initial_prompt`）帶入術語；要正式採用前應用真實錄音評估。
- int8_float16 省約 25% 顯存、速度相同，但同一段長句的錯誤略不同，沒有明顯變好或變差。

## 結論

- Whisper base 在這張 6 GB 顯卡上只佔約 0.3–0.4 GB，GPU 與對話服務（資源上限見 `docker-compose.ntnu.yml`）不衝突；CPU int8 也夠快（長音檔 RTF 約 0.04），沒有 GPU 時可行。
- 瓶頸不在顯存而在序列化：並行請求排隊，8 人同時送 1 分鐘長錄音要等約 12 秒；一般短句（數秒）幾乎無感。
- 服務以 `WHISPER_PERF_LOG` 可逐請求記錄耗時與 GPU 使用量（不含辨識文字），可用來收集真實使用時的數據。
