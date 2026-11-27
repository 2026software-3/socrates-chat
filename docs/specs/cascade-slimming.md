# cascade 瘦身實測

> 2026-11-26 至 27 的量測紀錄，不是規格決策。結論是「哪些改法省資源、有沒有讓斷句與打斷變慢」，採用與否屬部署取捨。標示「未驗證」的項目沒有量過。

## 結論

| 改法 | 結果 | 處置 |
| --- | --- | --- |
| **B**：`vad`、`turn_detection` 併進 cascade 程序 | 閒置記憶體約減半，斷句與打斷行為不變，延遲沒有變慢 | **已寫進 `docker/cascade/launch.yaml`，預設生效** |
| **C**：Whisper 改跑 NVIDIA GPU | 辨識延遲降為約 1/5，字錯率沒變差；GPU 顯存多約 420 MiB，容器記憶體沒有省 | 可選，疊 `docker-compose.whisper-gpu.yml` |
| **A**：每個後端一個 cascade 容器，共用 tts、whisper、adapter | 兩個後端各一場同時進行，互不干擾；重啟其中一個不影響另一個 | 可選，疊 `docker-compose.cascade-2.yml`（含 E：記憶體上限 2g） |
| D：`MALLOC_ARENA_MAX=2` | 匿名記憶體沒有變化（1209 → 1202 MB） | 不採用 |
| F：Kokoro 的 allocator 設定 | 沒做。要重啟正在服務的 TTS 容器，預期只省 40 到 100 MiB | 未驗證 |
| G、H、I：換 Kokoro 精度或量化、Whisper 換 base | 沒做。會改變語音品質，Kokoro ONNX 還要新增依賴 | 需要規格決策（見 [cascade-voice-spike.md](cascade-voice-spike.md) 的「正式聲音與模型」、[whisper-base-stt-perf.md](whisper-base-stt-perf.md)） |

## 量測環境與方法

- 筆電：RTX 4050 Laptop（6 GB 顯存）、Docker Desktop 的 WSL VM 16 GB。映像 `socrates-cascade:latest`（sglang-omni 0.2.0 加 `patch_dialog.py`）。
- 假 LLM 取代後端的內部端點，把 OpenAI 的延遲隔開；學生的聲音由同一個 TTS 合成。工具與用法在 [`scripts/cascade-bench/`](../../scripts/cascade-bench/README.md)。
- 每個變體都用「剛啟動後等 30 秒」的同一個時間點量閒置。延遲比較用**成對交錯**（兩個 cascade 同時待命、輪流跑），並用事件互相相減的間隔，不用絕對時間（理由見該 README）。
- 記憶體看容器內各程序的匿名記憶體（`RssAnon`）。`docker stats` 的數字含可回收的檔案快取，會比匿名記憶體高、而且會波動。
- TTS 與 Whisper 是筆電上正在服務 staging 的那組，測試期間共用。

## 記憶體與顯存

| 設定 | 閒置容器 | 程序數 | 匿名記憶體合計 | 兩場同時的容器尖峰 |
| --- | --- | --- | --- | --- |
| 基準（現行） | 2723 MB | 5 | 2325 MB | 2894 MB |
| B | 1289 MB | 3 | 1209 MB | 1446 MB |
| B 加 D | 1599 MB | 3 | 1202 MB | 1733 MB |

每個 stage worker 約 0.6 GB 的來源是各自載入一份 torch、transformers、sglang，而 vad 與 turn_detection 真正的模型只有幾十 MB。B 讓它們共用 cascade 程序已載入的那一份。

顯存（總共 6141 MiB）：只有 TTS 時約 960 到 1160 MiB；加 GPU 版 Whisper 後約 1584 MiB；兩個 cascade 各一場同時進行的尖峰 1616 MiB。

## 延遲（成對量測，中位數）

| 情境 | B 前（基準） | B | B 加 C（Whisper 在 GPU） |
| --- | --- | --- | --- |
| 單場：speech_stopped 到首段音訊 | 1.97 s | 1.91 s | 0.92 s |
| 單場：辨識階段（含 Smart Turn 等待） | 1.24 s | 1.22 s | 0.51 s |
| 兩場同時說完：speech_stopped 到首段音訊 | 3.18 s | 3.10 s | 1.08 s |

基準與 B 是同一批成對測試（單場 n=20、兩場 n=30），B 與 C 是另一批成對測試（B 的對照值在該批為 1.92 s 與 2.94 s）。全部 0 次失敗。兩場同時時，原本 CPU 上的 Whisper 會排隊，改到 GPU 後排隊幾乎消失。這些數字不含真正的 LLM，實際首字延遲主要由 LLM 決定。

## 斷句與打斷

| 項目 | 基準 | B | B 加 D | B 加 C |
| --- | --- | --- | --- | --- |
| 句中停頓 0.8 秒是否切成兩句 | 否 | 否 | 否 | 否 |
| 停頓 1.2、1.6 秒 | 切成兩句 | 切成兩句 | 切成兩句 | 切成兩句 |
| 打斷：後端偵測到插話 | 0.31 s | 0.21 s | 0.21 s | 0.31 s |
| 打斷：偵測到後取消回覆 | 0.01 s | 0.01 s | 0.01 s | 0.01 s |
| 取消前多送的音訊包 | 0 | 0 | 中位 0，偶發 1 包 | 中位 0，偶發 1 包 |

斷句門檻（停頓約 1 秒）在所有變體都相同。偵測延遲的差異（0.21 與 0.31）在基準本身就出現過兩個值，和變更無關。`speech_stopped` 的事件時間會因測試音訊對齊 100 ms 幀的位置落在兩個值（約 1.08 與 1.28 秒），同一個值之內基準與變體的差異不超過 10 ms。

## Whisper：CPU 與 GPU

12 個含哲學術語的繁體句子（Kokoro 合成），同一批音訊送兩個服務：

| 項目 | CPU int8（現行） | GPU int8_float16 |
| --- | --- | --- |
| 字錯率 | 4.8% | 4.4% |
| 整句全對 | 7/12 | 7/12 |
| 術語命中 | 5/9 | 5/9 |
| 單一請求延遲（中位） | 1.28 s | 0.27 s |
| 兩個請求同時（總時間） | 2.60 s | 0.65 s |
| 容器記憶體 | 0.83 GiB（閒置） | 1.11 GiB（推論後） |

兩邊辨錯的字幾乎相同（「產婆術」「德爾菲」等術語都會錯），GPU 沒有比較差，但也沒有解決術語問題。樣本只有 12 句合成語音，不能代表真人錄音；採用前仍照 [whisper-base-stt-perf.md](whisper-base-stt-perf.md) 的建議用真實錄音評估。

## 兩個 cascade 容器（A、E）

| 驗證項目 | 結果 |
| --- | --- |
| 各接不同後端、同時各一場（5 輪） | 各 0 次失敗；speech_stopped 到首段音訊約 1.1 s |
| 各自的後端只收到自己的回呼 | 各 5 次，互不混 |
| 重啟其中一個時，另一個的 5 輪場次 | 0 次失敗，延遲不變 |
| 其中一個掛 `mem_limit: 2g` | 尖峰約 1.8 GB（含檔案快取），沒有 OOM |
| 掛 2g 上限的那個容器持續 10 分鐘負載（13 個循環、65 輪，含單場、兩場同時、打斷） | 0 次異常；容器記憶體 1319 到 1443 MB；沒有 OOM、沒有重啟 |

兩個容器共用同一個 TTS（Kokoro 用單一鎖，一次合成一句）與同一張 GPU，所以其中一個後端的負載會拖慢另一個。同一台機器同時服務正式與 staging 時，這是營運上的取捨。

## Intel 映像（msi_cubi，Whisper 在 NPU、Kokoro 在 iGPU）

2026-11-27 凌晨、沒有學生場次時，在 msi_cubi 用 prod 同一個映像（`socrates-cascade-core:intel-20261116`）與同一組 TTS／Whisper／adapter，另起測試 cascade，回呼指向假 LLM，基準與 B 各兩輪交錯（基準、B、基準、B）。測完容器已清掉，prod 容器沒有重啟。

| 項目 | 基準（現行 prod 設定） | B |
| --- | --- | --- |
| 閒置容器記憶體（兩輪） | 2.35、2.35 GiB | 1.25、1.25 GiB |
| 匿名記憶體合計／大程序數 | 2318 MB／4 | 1202 到 1203 MB／2 |
| 兩場同時後的容器記憶體 | 2.51 到 2.53 GiB | 1.39 GiB |
| 單場：speech_stopped 到首段音訊（n=20） | 1.29 s | 1.26 s |
| 兩場同時：同上（n=20，兩輪各 10） | 1.35 s（1.44、1.32） | 1.45 s（1.43、1.53） |
| 句中停頓 0.8 秒／1.2 秒的轉錄次數 | 1／2 | 1／2 |
| 打斷偵測、取消 | 0.22 s、0.01 s | 0.17 s、0.01 s |
| 取消前多送的音訊包 | 中位 0 | 中位 0，p90 1 包 |

單場、斷句、打斷與 NVIDIA 的結果一致，記憶體同樣約減半。兩場同時時 B 的中位數高約 0.1 秒，但兩輪重複各自的差異（基準 1.32 到 1.44、B 1.43 到 1.53）互相重疊，這個樣本不足以判定是退步還是雜訊；Intel 上瓶頸是共用的 Kokoro（單一鎖）。

注意：這個映像比 `CASCADE_LLM_BASE_URL` 的支援還舊，`launch.yaml` 把回呼寫死為 `http://app:3000/internal/cascade/v1`，測試時在容器內改寫。當時 prod 的 Intel cascade 映像（`intel-20261116`）也是這個舊版，重建後才吃得到現行的 `entrypoint.sh` 與 B，見下節「部署紀錄」。

## 部署紀錄

| 時間 | 環境 | 做法 | 結果 |
| --- | --- | --- | --- |
| 2026-11-27 | NTNU 正式（msi_cubi） | 以 intel 分支現狀加 main 的 `launch.yaml` 建 `socrates-cascade-core:intel-20261127`；`.env` 的 `CASCADE_INTEL_TAG` 改新值，只重建 `cascade` 容器（`--no-deps`）。tts、whisper、adapter 沿用舊映像，已加同名新 tag 別名 | cascade 容器記憶體約 2.4 → 1.26 GiB，`/health` 與 `/ready` 為 200 |
| 2026-11-27 | staging 語音節點（RTX 4050 筆電） | 以現行 `socrates-cascade:latest` 加 main 的 `launch.yaml` 建薄映像，`latest` 指向它，只重建 `cascade` 容器；舊映像保留為 `socrates-cascade:pre-b-20261127` | 容器 healthy，記憶體約 1.6 GiB，B 生效 |

退路：正式環境把 `.env` 的 tag 改回 `intel-20261116` 再重建 `cascade`；筆電把 `socrates-cascade:latest` 指回 `pre-b-20261127` 再重建 `cascade`。

## 限制與未驗證

- Intel 映像的量測只用舊映像（2026-11-16 建）加容器內改寫設定，沒有用重建後的映像；兩場同時的延遲差異沒有定論（見上節）。
- msi_cubi 的 staging 只有 app、db、proxy，沒有 cascade，所以 Intel cascade 無法經 staging 驗證，改在 msi_cubi 另起隔離的測試容器（沒有學生場次的時段）。
- 上線後沒有做真人從瀏覽器走語音（含打斷）的端到端驗證。
- 測試語音是合成的，停頓與音量比真人規則；斷句門檻的絕對值不能直接套到真人，這份紀錄只用來比較變更前後。
- soak 只跑了 10 分鐘，沒有做 1 小時以上的長時間測試。
- 沒有量真正的 LLM 延遲，也沒有量記憶體隨場次數成長以外的長期變化。
- 這份紀錄的前提是 sglang-omni 0.2.0；升版後要重量。
