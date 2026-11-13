# macOS（Apple Silicon）原生語音 cascade

在 Mac 上不靠 NVIDIA、不靠 Docker 跑即時語音：與 NVIDIA 部署（`docker/cascade/`、compose 的 `cascade` profile）使用**同一套** sglang-omni Streaming Cascade、同一份 `launch.yaml`、同一個轉接層與 Kokoro 端點契約，差別只在推論裝置。後端（Rust）與前端完全不變，仍是 `CASCADE_REALTIME_URL` ＋ `CASCADE_LLM_TOKEN`。

NVIDIA／Windows 部署不受影響：`docker/cascade/` 只多了 `KOKORO_DEVICE`、`KOKORO_HOST` 兩個可省略的環境變數，預設值（`cuda`、`0.0.0.0`）就是原本的行為。

## 架構

```text
瀏覽器 ──WebSocket── 後端（Rust） ──WebSocket── cascade（sgl-omni，:8110）
                        ▲                          │ VAD／斷句／打斷
                        │ /internal/cascade/v1     ├─ ASR  ─→ 轉接層（:8103）─→ Whisper small（CPU int8，:8101）
                        └──────────────────────────┤ LLM  ─→ 後端內部端點（不變）
                                                   └─ TTS  ─→ 轉接層（:8103，繁轉簡）─→ Kokoro-82M（PyTorch MPS，:8100）
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
| `KOKORO_DEVICE` | `mps` | `mps`／`cpu`／`auto`；指定的裝置不可用時啟動失敗，不會默默退回 CPU |
| `WHISPER_MODEL` | `small` | 與 compose 相同 |
| `START_TIMEOUT` | `240` | 每個服務等待就緒的秒數（第一次載入模型、編譯 Metal 較久） |
| `CASCADE_MACOS_HOME` | `~/.cache/socrates-cascade-macos` | venv、sglang 原始碼、執行目錄與記錄檔 |
| `CASCADE_VENV`／`VOICE_VENV` | 上述目錄下的 `venv-cascade`／`venv-voice` | 想沿用既有環境時指定 |

記錄檔在 `$CASCADE_MACOS_HOME/run/*.log`，不含對話內容。`start.sh` 由 `docker/cascade/launch.yaml` 轉出 `run/launch.yaml`（`render_launch.py` 只換服務位址與 `model_path`，找不到預期內容就失敗），所以不需要另外維護一份設定。

## 安裝內容與版本

| 環境 | 內容 |
| --- | --- |
| `venv-cascade` | `sglang-omni==0.2.0`（與 NVIDIA 映像同版本）、`sglang` v0.5.23 以 `all_mps` 從原始碼安裝、`opencc`；套用 `docker/cascade/patch_dialog.py` |
| `venv-voice` | `faster-whisper`、`kokoro==0.9.4`、`misaki[zh]==0.9.4`、`av>=14,<16`、`fastapi`、`uvicorn`、`opencc` |

為什麼要從原始碼裝 sglang：`sglang-omni` 在 macOS arm64 刻意不依賴 `sglang`，Mac 的 PyPI 最高只有 0.5.10 且缺 `platforms` 模組，直接 `pip install sglang-omni` 之後 `sgl-omni` 載入就失敗。這是上游 `install.sh` 的做法（上游的 Apple Silicon 支援標為 Experimental，且只文件化 Qwen3-ASR；Streaming Cascade 在 Mac 上沒有上游的支援聲明）。我們沒有用上游的 `install.sh`，因為它會執行 `brew install`。

## 實測結果（2026-11-13，MacBook Pro M1、8 GB；合成語音、假 LLM）

用 `start.sh` 啟動後，以 WebSocket 客戶端模擬後端與瀏覽器（送 `session.update`、依即時速度送 16 kHz 音訊、`response.cancel`）：

- 四個服務依序就緒；cascade 七個階段 healthy，placement 為 0 GPU。
- 一輪完整對話：VAD 約在說完後 0.95 s 判定結束（設定 1000 ms）、Whisper 約 1.5 s、Kokoro 第一段約 0.9 s；從說完到第一段語音 4.2–4.8 s（含假 LLM 固定的 0.8 s，**不是真實 LLM 延遲**）。回覆為 24 kHz PCM，約依即時速度送出，結束狀態 `completed`。
- 手動打斷：送出 `response.cancel` 後約 13 ms 收到 `response.done`（`cancelled`），僅收到約 2 秒音訊。
- 數字是單一機器、單次情境、其他桌面程式同時開著；不是效能保證。與 Windows 的紀錄（Qwen TTS、假 LLM）不能直接比較。
- 單一元件：Whisper small CPU int8 對 4.9 s 語音 1.36 s；Kokoro MPS 的 RTF 約 0.12–0.15；Whisper 與 TTS 各自序列化，多人同時使用會排隊（Whisper 4 路同時送 4.9 s 語音整體約 5.5 s）。

### 記憶體

8 GB 的機器上實測時，系統 swap 已用到 3.4／4 GB（桌面程式也開著），所以**這台沒有可靠的記憶體峰值**，也不建議把 8 GB 當作可用規格。16／24／32 GB 沒有量測，沒有數字可寫。已知單一元件的量級：Kokoro MPS 的 `torch.mps` allocated 約 542 MiB（driver 約 1282 MiB）。

## 尚未驗證／已知限制

- 沒有接真的後端與瀏覽器：`heard_chars` 的保存、打斷後的歷史、連續發言合併、`socrates.turn.failed`、後端在 Docker 內的連線，都只在 NVIDIA 部署驗證過，Mac 上未測。
- 自動打斷（學生插話）、ASR／TTS 失敗、token 不符、多人併發在 cascade 上都未測。
- 沒有設定併發上限、佇列長度、最長回覆時間等參數：目前只有「一位學生同時一場語音」（後端）與各服務自己的序列化。
- 長時間運作、降頻、重開機後自動啟動（launchd）未處理；`start.sh` 不會自動重啟掛掉的服務。
- 啟動 Whisper 與 Kokoro 預設用 CPU int8 與 MPS；沒有用過 MLX（實測速度與 MPS 沒有可測得的差異）。
- ASR 另一個選項 whisper.cpp（Metal）實測單句約快 1.8 倍（4.9 s 語音 0.76 s 對 1.36 s），但**沒有整合**：它輸出簡體、也沒有 `s2tw`、`no_speech_prob`／`avg_logprob`、片尾套語與 prompt 洩漏過濾，採用前要在轉接層補回。
- `install.sh` 在全新的 `CASCADE_MACOS_HOME` 從零跑過一次（2026-11-13，約 10 分鐘，網路順暢時；Hugging Face 快取已有模型，所以沒有包含下載模型的時間），之後用這個新環境跑 `start.sh`、端到端與打斷都通過。只驗證過一次，沒有在乾淨的第二台機器上驗證。
- **啟動後第一輪對話較慢**：全新環境的第一輪從說完到第一段語音是 8.5 s，同一次啟動的第二輪是 4.4 s（之前的環境第一輪也有 6.0 s）。推測是第一次推論的冷啟動，沒有拆開驗證；正式使用前可以先自己跑一輪暖機。
