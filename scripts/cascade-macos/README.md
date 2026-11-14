# macOS（Apple Silicon）原生語音 cascade

在 Mac 上不靠 NVIDIA、不靠 Docker 跑即時語音：與 NVIDIA 部署（`docker/cascade/`、compose 的 `cascade` profile）使用**同一套** sglang-omni Streaming Cascade、同一份 `launch.yaml`、同一個轉接層與 Kokoro 端點契約，差別只在推論裝置。後端（Rust）與前端完全不變，仍是 `CASCADE_REALTIME_URL` ＋ `CASCADE_LLM_TOKEN`。

NVIDIA／Windows 部署不受影響：`docker/cascade/kokoro_server.py` 只多了可省略的環境變數（`KOKORO_DEVICE`、`KOKORO_HOST`、`KOKORO_BACKEND`、`KOKORO_LEAD_SILENCE_MS`），沒設定時就是原本的行為（PyTorch、`cuda`、`0.0.0.0`、不裁靜音）。Mac 專用的 MLX 語音辨識服務放在這個目錄（`whisper_mlx_server.py`），不影響 `scripts/whisper-server.py`。

## 架構

```text
瀏覽器 ──WebSocket── 後端（Rust） ──WebSocket── cascade（sgl-omni，:8110）
                        ▲                          │ VAD／斷句／打斷
                        │ /internal/cascade/v1     ├─ ASR  ─→ 轉接層（:8103）─→ Whisper small（MLX，:8101）
                        └──────────────────────────┤ LLM  ─→ 後端內部端點（不變）
                                                   └─ TTS  ─→ 轉接層（:8103，繁轉簡）─→ Kokoro-82M（MLX，:8100）
```

四個服務全部是 macOS 原生行程，只綁 `127.0.0.1`。Docker 容器（Linux VM）用不到 Metal，所以 cascade 不放進容器；資料庫與後端可以照舊用 Docker 或 `cargo run`。

## 需求

- Apple Silicon（M1 以上）、macOS，`uv`（`pip install uv` 或 `brew install uv`）、`git`。
- 安裝不使用 Homebrew、不需要管理者權限，也不會裝任何 CUDA 套件。
- 磁碟：兩個虛擬環境與模型約數 GB（未精確量測）。

## 使用

```bash
scripts/cascade-macos/install.sh        # 建立兩個 venv、編譯 sglang、預先下載模型（加 --skip-models 可略過下載）
export CASCADE_LLM_TOKEN=$(openssl rand -hex 32)   # 與後端的 CASCADE_LLM_TOKEN 必須相同；不要寫進任何檔案
scripts/cascade-macos/start.sh          # 依序啟動並等待就緒
scripts/cascade-macos/status.sh         # 全部健康回傳 0
scripts/cascade-macos/stop.sh           # 反向順序停止，只動它自己啟動的行程
```

後端（與 NVIDIA 版相同的兩個變數）：

```bash
CASCADE_REALTIME_URL=ws://127.0.0.1:8110/v1/realtime
CASCADE_LLM_TOKEN=<與 start.sh 相同>
```

後端若在 Docker 內，容器連到主機要改用 `host.docker.internal`；**這條路徑沒有驗證過**（本機驗證時後端是假的 LLM 端點）。cascade 呼叫後端內部端點的位址由 `APP_URL` 指定（預設 `http://127.0.0.1:3000`，即 compose 對主機公開的埠）。

### 環境變數

| 變數 | 預設 | 說明 |
| --- | --- | --- |
| `CASCADE_LLM_TOKEN` | 必填 | 與後端相同的共用密鑰，只傳給 cascade 行程，其他服務不繼承 |
| `APP_URL` | `http://127.0.0.1:3000` | cascade 呼叫內部 LLM 端點的後端位址 |
| `WHISPER_BACKEND` | `mlx` | `mlx`（mlx-whisper，GPU）／`faster-whisper`（CPU int8，與 NVIDIA 部署同一支 `scripts/whisper-server.py`） |
| `KOKORO_BACKEND` | `mlx` | `mlx`（mlx-audio，GPU）／`torch`（PyTorch，裝置看 `KOKORO_DEVICE`） |
| `KOKORO_DEVICE` | `mps` | 只在 `KOKORO_BACKEND=torch` 時有用：`mps`／`cpu`／`auto`；指定的裝置不可用時啟動失敗，不會默默退回 CPU |
| `KOKORO_LEAD_SILENCE_MS` | `50` | 每句語音開頭最多保留幾毫秒靜音（Kokoro 原本約 400 ms）；設成空字串就不裁 |
| `WHISPER_MODEL` | `small` | 與 compose 相同；`mlx` 時對應 `mlx-community/whisper-<名稱>-mlx`，其他命名用 `WHISPER_MLX_MODEL` 指定完整名稱 |
| `START_TIMEOUT` | `240` | 每個服務等待就緒的秒數（第一次載入模型、編譯 Metal 較久） |
| `CASCADE_MACOS_HOME` | `~/.cache/socrates-cascade-macos` | venv、sglang 原始碼、執行目錄與記錄檔 |
| `CASCADE_VENV`／`VOICE_VENV` | 上述目錄下的 `venv-cascade`／`venv-voice` | 想沿用既有環境時指定 |

記錄檔在 `$CASCADE_MACOS_HOME/run/*.log`，不含對話內容。`start.sh` 由 `docker/cascade/launch.yaml` 轉出 `run/launch.yaml`（`render_launch.py` 只換服務位址與 `model_path`，找不到預期內容就失敗），所以不需要另外維護一份設定。

## 安裝內容與版本

| 環境 | 內容 |
| --- | --- |
| `venv-cascade` | `sglang-omni==0.2.0`（與 NVIDIA 映像同版本）、`sglang` v0.5.23 以 `all_mps` 從原始碼安裝、`opencc`；套用 `docker/cascade/patch_dialog.py` |
| `venv-voice` | `mlx-whisper==0.4.3`、`mlx-audio==0.6.5`（預設後端）；`faster-whisper`、`kokoro==0.9.4`、`misaki[zh]==0.9.4`（退路，也提供音訊解碼與 Silero VAD）；`av>=14,<16`、`fastapi`、`uvicorn`、`opencc` |

為什麼要從原始碼裝 sglang：`sglang-omni` 在 macOS arm64 刻意不依賴 `sglang`，Mac 的 PyPI 最高只有 0.5.10 且缺 `platforms` 模組，直接 `pip install sglang-omni` 之後 `sgl-omni` 載入就失敗。這是上游 `install.sh` 的做法（上游的 Apple Silicon 支援標為 Experimental，且只文件化 Qwen3-ASR；Streaming Cascade 在 Mac 上沒有上游的支援聲明）。我們沒有用上游的 `install.sh`，因為它會執行 `brew install`。

## 實測結果（2026-11-13，MacBook Pro M1、8 GB）

條件：合成語音（macOS `say`）、假的 LLM（固定 0.8 s 後以每字 20 ms 串流，**不是真實 LLM 延遲**）、單一機器、其他桌面程式同時開著、swap 已用 3–4 GB。數字不是效能保證，也不能和 Windows 的紀錄直接比。「原本」＝第一版（faster-whisper CPU int8＋Kokoro PyTorch MPS、不裁靜音，現在等於 `WHISPER_BACKEND=faster-whisper KOKORO_BACKEND=torch KOKORO_LEAD_SILENCE_MS=`）；「現在」＝預設值。兩者用同一個測試程式、同一串回覆。

假 LLM 每一輪回不同的三句話（16 句輪替）。第一版的量測每次都回同樣三句，會低估 PyTorch MPS 的延遲（見下方「TTS」）。

### 一輪對話（WebSocket 客戶端模擬後端與瀏覽器，4.9 s 的語音，暖機後 10 輪中位數）

| 階段 | 原本 | 現在 |
| --- | --- | --- |
| 說完 → VAD 判定（`silence_duration_ms: 1000`，沒有改） | 0.96 s | 0.97 s |
| 語音辨識 | 1.58 s | 0.57 s |
| 假 LLM 首字 | 0.82 s | 0.82 s |
| LLM 首字 → 第一段音訊（含第一句串流完的時間） | 1.67 s | 1.07 s |
| **說完 → 第一段音訊送達** | 4.96 s（4.33–5.93） | 3.40 s（2.97–4.19） |
| **說完 → 聽得到第一個字**（扣掉音訊開頭的靜音） | 5.34 s | 3.46 s |
| 啟動後第一輪（說完 → 第一段音訊） | 6.72 s（另兩次啟動 4.45、5.35 s） | 3.20 s（另一次 3.07 s） |
| 打斷：`response.cancel` → `response.done(cancelled)` | 10–32 ms（4 次） | 10–36 ms（4 次） |
| 四個服務的實體記憶體（`footprint`） | 4.6 GB（Whisper 0.4、Kokoro 2.3、cascade 1.9） | 3.4 GB（Whisper 0.9、Kokoro 0.5、cascade 1.9） |

模擬多人（每人一條 WebSocket：想 3–8 s → 說 4.9 s → 聽完整則回覆 → 重複；每個人數跑 120 s）：

| 人數 | 原本：中位數／p90／最長 | 現在：中位數／p90／最長 |
| --- | --- | --- |
| 1（5 輪） | 4.94／6.02／6.02 s | 3.69／4.61／4.61 s |
| 4（18、20 輪） | 4.63／5.40／5.70 s | 3.62／4.44／4.74 s |
| 8（35、38 輪） | 5.06／9.57／12.61 s | 4.18／7.38／9.98 s |

所有回覆都是 `completed`、沒有錯誤。8 人時的長尾來自 TTS 排隊（該階段 TTS 回應時間中位數 0.98 s、p90 3.06 s；辨識 0.88／1.39 s）：一則回覆有三句，後面的句子和別人的第一句排同一條先進先出的隊。16 句輪替幾輪後 PyTorch MPS 會遇到算過的句長，所以這張表對「原本」偏樂觀。

### 語音辨識（24 句合成語音，兩種音色，1–11 s；經 HTTP，每句 3 次取中位數）

| 後端 | 延遲中位數 | 4.9 s 語音 | 31 s 語音 | 同時 4／8 個請求全部完成 | 錯字（共 454 字） |
| --- | --- | --- | --- | --- | --- |
| faster-whisper small，CPU int8，beam 5（原本） | 1.23 s | 1.39 s | 4.33 s | 5.44／10.70 s | 35（其中一句 32 字整句被過濾掉；其餘 3） |
| whisper.cpp v1.9.5 small，Metal，beam 5 | 0.97 s | 1.08 s | 2.99 s | 4.17／8.42 s | 51（4.9 s 那句回空字串；2 句每次結果不同） |
| whisper.cpp 再把 `audio_ctx` 依語音長度縮短 | 0.43 s | 0.56 s | 3.39 s | 沒測 | 111；靜音與雜訊會幻想出內容，並花 3–15 s |
| **mlx-whisper 0.4.3 small（現在）** | 0.48 s | 0.56 s | 1.86 s | 2.27／4.55 s | 5 |

- mlx-whisper 只有貪婪解碼（沒有 beam search）。在這組音檔上，它與 faster-whisper beam 5 的錯字幾乎是同一批（「知識」聽成「知事」「柏拉圖」聽成「波拉圖」）；**合成音色、樣本小，不能據此說真人語音的準確度沒有差**。
- 整理文字的規則沒有變：`whisper_mlx_server.py` 用的是同一份 `whisper_text.py`／`whisper_echo.py`（臺灣繁體、`no_speech_prob<0.6`、`avg_logprob>=-1.0`、片尾套語、提示詞洩漏），推論前同樣先過 Silero VAD；靜音與雜訊（2 s、6 s）都回空字串。有契約測試（`test_whisper_mlx_server.py`）。
- whisper.cpp 沒有採用：這次量到只比 CPU 快約 2 成（先前單句量到 1.8 倍，沒有重現），`no_speech_prob` 偏高（另一次檢查中，兩句真的語音是 0.67、0.68，會被現有門檻丟掉），2 s 的靜音會幻想出一句話，而且要從原始碼編譯。
- 既有行為、兩個後端都一樣：片段的 `no_speech_prob` 一超過 0.6 就整段丟掉，即使 `avg_logprob` 很高：31 s 的音檔兩個後端都只回最後一句（mlx-whisper 量到前兩段是 0.67、`avg_logprob` −0.14），faster-whisper 還把一句 7 s 的語音整句丟掉（同一句 mlx-whisper 是 0.57，剛好留下）。這是既有的過濾規則，這次沒有改；真人語音會不會這樣沒有量。

### TTS（Kokoro-82M `zm_yunjian`，16 句長短不同的句子，每句前先閒置 3 s）

| 後端 | 沒唸過的句子：中位數／最長 | 同一句再唸一次：中位數 | 常駐記憶體（`footprint`） |
| --- | --- | --- | --- |
| PyTorch MPS（原本） | 1.42／2.30 s | 0.74 s | 2.3–2.5 GB |
| PyTorch CPU | 0.81／1.52 s | 0.83 s | 沒量 |
| **mlx-audio 0.6.5（現在）** | 0.60／1.06 s | 0.59 s | 0.5 GB（載入時的峰值 3.5 GB） |
| kokoro-mlx 0.1.4（行程內量測） | 0.80／1.06 s | 0.65 s | 沒量（MLX 快取到 4.9 GB） |

- **PyTorch MPS 遇到沒算過的句長要重新編譯計算圖**，而實際對話的句子幾乎都是新的；這種情況下它比 CPU 還慢。連續重複同一句時 MPS 是 0.41 s（第一版的數字），閒置 2–10 s 後再唸同一句是 0.46–0.91 s。MLX 沒有這兩個現象，所以先前「MLX 與 MPS 沒有差」只在重複同一句時成立。
- 三者產生的音訊長度相同。把合成的語音再辨識回文字（16 句、261 字）：MLX 錯 4 字、PyTorch MPS 錯 5 字，錯的是同樣幾個字。**這不是音質評分，沒有人聽過。**
- Kokoro 每句音訊開頭有約 0.39 s 靜音、結尾約 0.98 s。`KOKORO_LEAD_SILENCE_MS=50` 把開頭裁到 50 ms，學生早約 0.34 s 聽到第一個字；結尾（句與句之間的停頓）沒有動。裁掉後 cascade 回報的 `audio_ms` 仍等於實際送出的音訊長度。
- 沒有採用「句內先切第一個子句」：第一段音訊可早 0.27 s（中位數），但每句多 0.25 s 運算，而且每次合成都帶頭尾靜音，逗號處會多出約 1.4 s 的停頓。
- cascade 對中文只在句號、問號等處切句（空白分詞對中文無效），所以第一段音訊一定等 LLM 把第一句串流完；第一句越長等越久。

### 沒有效果或沒有做的

- `turn_detection: semantic_vad`（以 `session.update` 指定 medium、high）：VAD 判定時間仍是 0.96 s，沒有變短。
- `silence_duration_ms`：預設維持 1000（太短會切斷學生說話，這是產品行為）。量測上它一比一換成延遲：以 `session.update` 指定 500 時，VAD 判定 0.43 s、說完到第一段音訊 2.40 s（3 輪）。
- Core ML（whisper.cpp 的 encoder）：沒測，需要另外轉模型並重新編譯。
- faster-whisper（退路）的解碼參數：beam 5 → 1 的延遲中位數 1.21 → 1.07 s，這組音檔的錯字相同；關掉時間戳只再快 0.01–0.06 s；執行緒從預設改成 8 反而變慢（1.60 s）。差距小，`scripts/whisper-server.py` 沒有改。

## 尚未驗證／已知限制

- 沒有接真的後端與瀏覽器：`heard_chars` 的保存、打斷後的歷史、連續發言合併、`socrates.turn.failed`、後端在 Docker 內的連線，都只在 NVIDIA 部署驗證過，Mac 上未測。以客戶端模擬確認過的只有：`response.created`、`sglang.response.progress`（`text_end` 與原本相同、`audio_ms` 等於送出的音訊長度）、`response.done`（`completed`／`cancelled`）、打斷後依 `conversation.item.truncate` 的播放位置回 `sglang.response.heard`（0.8 s → 0 字、6 s → 第一句結尾）。
- 真人語音的辨識準確度（尤其貪婪解碼對 beam 5）、真實 LLM 的延遲、教室環境的雜訊都沒有量。
- mlx-whisper、mlx-audio 是新的依賴，只裝在 Mac 的 `venv-voice`；版本是實測過的那一組，沒有試其他版本。
- 自動打斷（學生插話）、ASR／TTS 失敗、token 不符在 cascade 上都未測。
- 沒有設定併發上限、佇列長度、最長回覆時間等參數：目前只有「一位學生同時一場語音」（後端）與各服務自己的序列化；TTS 佇列是先進先出，沒有讓每則回覆的第一句優先。
- 8 GB 的機器 swap 一直在用，記憶體數字只能看相對大小；16／24／32 GB 沒有量測。
- 長時間運作、降頻、重開機後自動啟動（launchd）未處理；`start.sh` 不會自動重啟掛掉的服務。
- 更新後的 `install.sh` 只在既有環境重跑過一次（成功；補裝兩個 MLX 套件共 14 個套件，無版本衝突），模型先前就在快取裡；沒有在全新的 `CASCADE_MACOS_HOME` 或第二台機器上從零跑過。
