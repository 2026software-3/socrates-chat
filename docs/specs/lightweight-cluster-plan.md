# 對話服務輕量化叢集計劃書（可加入團隊成員電腦作為節點）

> **狀態：草案（2026-11-03，第三版：範圍限定為對話服務），不構成規格決策。** 正式環境部署方式（S-09.1）仍未定案。本文規劃的叢集只服務**對話服務**，不含 CI、預覽環境或其他專案部分。是否可處理真實學生的對話，是需求方才能決定的事（第 2.1 節、第 10 節）。確認後依 [CONTRIBUTING.md](../../CONTRIBUTING.md#規格決策流程) 以 PR 更新 `decisions.md`，再開始實作。

## 1. 範圍

「對話服務」指學生與 AI 對話所經過的執行路徑：

| 元件 | 內容 | 位置 |
| --- | --- | --- |
| 後端 `app` | 對話 API、SSE 回覆串流、總結、登入與權限、AI 呼叫、用量限制（`chat.rs`、`reply.rs`、`summary.rs`、`ai.rs`） | 單一映像，同網域提供前端（S-08.1） |
| 語音即時路徑 | 瀏覽器 ⇄ Rust（WebSocket）⇄ cascade（VAD、斷句、打斷） | `cascade.rs`、`cascade-voice-spike.md` |
| 語音模型 | ASR（Whisper）、TTS（含轉接層） | `scripts/whisper-server.py`、`OPENAI_STT_BASE_URL`／`OPENAI_TTS_BASE_URL` |
| PostgreSQL | 對話、訊息、session | 沿用既有 |

不在範圍：CI runner、PR 預覽環境、前端獨立部署、題目／點數／成就等其他功能的獨立擴充。

目標（待需求方確認，見第 10 節）：讓成員電腦可以加入並分擔對話服務中**吃資源的部分**，主要是語音模型的 GPU 運算，以及容量不足時的對話處理。

非目標：

- 控制平面高可用。
- 取代 S-09.1 的正式環境決策。
- 引入 Supabase、Redis 等 AGENTS.md 未列出的基礎設施。Docker Swarm 內建於 Docker Engine，不需額外安裝元件，但仍是新的部署機制，需先經規格討論。

## 2. 設計前提（由程式碼與規格確認）

### 2.1 對話內容會經過節點，這是最大的決策點

成員電腦若執行對話服務的元件，就會在記憶體中處理學生的訊息或音訊：

| 元件 | 該節點上的人能讀到什麼 |
| --- | --- |
| `app` | 學生與 AI 的訊息原文、session、PostgreSQL 憑證、OpenAI 金鑰 |
| cascade／ASR | 學生語音（音訊只轉送、不保存，S-05.4），以及語音辨識出的文字 |
| TTS | AI 回覆文字 |

AGENTS.md 規定管理介面、API、報表、日誌中訊息一律顯示為 `message`，且系統管理者讀取資料庫原文需留存取紀錄。成員電腦的使用者與 root 可以直接讀取程序記憶體與容器內容，**不在這套存取紀錄機制內**。因此：

- 使用真實學生資料前，需求方必須明確同意「成員電腦處理學生對話或音訊」。目前沒有這項決策。
- 在此之前，叢集只能用合成資料（AGENTS.md：測試資料只用合成資料）。

### 2.2 後端的對話狀態有一部分只存在單一程序的記憶體

| 狀態 | 位置 | 多實例時的行為 |
| --- | --- | --- |
| 進行中回覆的已產生文字與正在收看的連線（`LiveReplies`） | `reply.rs` | 重連到其他實例會回 `409 reply_in_progress`，接不回原回覆 |
| 語音對話的一次性代號（`voice_sessions`） | `cascade.rs` | cascade 回呼內部 LLM 端點時，必須打到建立該語音場次的同一個實例，否則解析不到 |
| 每位使用者的 AI 用量（`ai_usage.rs`、`user_limit.rs`） | 記憶體 | 各實例各自計算，**實際上限變成「每實例的額度 x 實例數」**，AI 費用控管失效 |
| 登入頻率限制、語音 `speech_busy` | 記憶體 | 各實例各自計算 |
| API key 冷卻狀態（`key_pool.rs`） | 記憶體 | 各實例各自冷卻，影響較小 |
| `/admin/logs` 近期請求 | 記憶體 | 只看到單一實例 |

已經是多實例安全的部分：

- 同一場對話只允許一條回覆在產生，靠資料庫的 `generating_since` 原子 claim，不靠記憶體。
- 服務異常中斷留下的殘留 claim，會在 `ai_total_timeout x 2 + 10` 秒後視為過期（`config.rs`）。
- session 存在 PostgreSQL（S-01.1）。

結論：**`app` 在不改程式的前提下只能單實例**。多實例需先處理上表，這是另案（第 9 節階段 5）。語音模型側（cascade、ASR、TTS）則本來就是獨立的 HTTP／WebSocket 服務，由 `CASCADE_REALTIME_URL`、`OPENAI_STT_BASE_URL`、`OPENAI_TTS_BASE_URL` 指向，可以橫向擴充。

### 2.3 節點隨時會消失

成員電腦會休眠、關機、換網路。放在成員節點上的工作必須能降級：

- 語音即時功能預設關閉，缺設定時不啟用；前端連不上或 10 秒沒有回應，會視為無法使用並改用瀏覽器語音（`cascade-voice-spike.md`）。連線**中途**斷線時的前端行為，文件沒有記載，列為階段 2 驗證項目。
- 文字對話不依賴成員節點（見 3.1），節點消失不影響文字對話。

## 3. 建議架構：可信核心 ＋ 工作池

```
                      tailnet（既有 Tailscale）
 ┌───────────────────────────────────────────────────────────────┐
 │ 可信核心（固定、常駐機器，例如 NTNU 機器）                       │
 │   app（單實例）  PostgreSQL  OpenAI 金鑰  CASCADE_LLM_TOKEN      │
 │   Swarm manager                                                │
 │                                                               │
 │ 工作池（成員電腦，label pool=voice-worker，只收 worker 角色）    │
 │   cascade x N    ASR x N    TTS 轉接層＋模型 x N                │
 │   沒有資料庫憑證、沒有 OpenAI 金鑰                               │
 └───────────────────────────────────────────────────────────────┘
   app ── WS ──> cascade（Swarm service VIP）
   cascade ── Bearer CASCADE_LLM_TOKEN ──> app 的 /internal/cascade/v1/*
```

### 3.1 為什麼這樣切

- `app` 持有資料庫憑證、OpenAI 金鑰、對話原文與所有記憶體內狀態，留在可信、固定的機器上，單實例，繞開 2.2 的問題。
- 成員電腦只跑語音模型側。它們不持有資料庫憑證與 OpenAI 金鑰；唯一的憑證是 `CASCADE_LLM_TOKEN`，且該密鑰只能呼叫 `/internal/cascade/v1/*`（內部端點只認這個密鑰，不看 cookie）。外洩的影響範圍小於持有資料庫憑證。
- 文字對話完全不經過成員節點。工作池全部下線，影響的只有即時語音（降級為瀏覽器語音）。
- 成員節點仍會看到語音與回覆文字（2.1），所以真實學生資料仍受第 10 節決策管制。

### 3.2 為什麼選 Docker Swarm（沿用第二版結論，並限縮到工作池）

| 方案 | 評估 |
| --- | --- |
| 各機器直接跑 Compose，後端用 `CASCADE_REALTIME_URL` 指向某台 GPU 機器 | 最簡單；單台 GPU 時足夠。要在多台 GPU 之間分流、節點增減時，就得自己管清單與負載分配 |
| **Docker Swarm（建議）** | Docker 已是必備；service 有 VIP，多個副本間自動分流；`placement constraints` 可限定只上有 GPU 的節點；加入／退出各一行指令 |
| k3s | 功能更完整，但概念與維運成本較高；成員機器作業系統不一時 agent 較難裝 |

先做各機器 Compose 的最小版本（階段 1），確認 GPU 工作真的能在成員機器上跑，再決定是否需要 Swarm 的排程，避免為了一台 GPU 機器就上叢集。

Swarm 的已確認限制（官方文件）：

- `docker stack deploy` 忽略 Compose 的 `build`，stack 檔必須用預先建好並推到 registry 的映像。其他選項（`depends_on`、`profiles`）的行為本文未逐項查證。
- stack 的 secret 只支援 `file` 來源，不支援 `environment`。
- overlay 需要 `2377/tcp`、`7946/tcp+udp`、`4789/udp`；VXLAN 埠沒有身分驗證，只能對可信網路開放，絕不可開在邊界防火牆。
- 多個 manager 必須為奇數且過半存活；成員筆電不適合當 manager，因此只用單一 manager（不做 HA）。
- 加入節點用 join token，屬秘密、不進版控，官方建議至少每 6 個月輪替；輪替不會踢掉已加入的節點。
- 放置規則為 `deploy.placement.constraints`，找不到符合條件的節點時 task 停在 `Pending`。
- Docker daemon 可用 `node-generic-resources`（`daemon.json`）向 Swarm 宣告自訂資源，例如 GPU。

依設計推論、尚未實測（階段 1／2 驗證）：

- Swarm 沒有 taint，沒寫 constraint 的 service 可能被排到成員電腦。對策見 3.3。
- Docker Desktop／WSL2 節點的位址與 Swarm 埠能否從 tailnet 連到。
- service 如何請求 GPU，以及在 WSL2 的行為。

### 3.3 排程規則（偽代碼）

```
for each service in stack files:
    if service.holds(DB credentials or OpenAI keys or user message plaintext at rest):
        require constraint "node.role == manager"   # 留在可信核心
    else if service in {cascade, asr, tts}:
        require constraint "node.labels.pool == voice-worker"
        require constraint "node.labels.gpu == true" if service.needs_gpu
        require resources.limits (cpu, memory)

CI check: every service must declare a placement constraint
          app must declare replicas == 1
```

節點上線與退出：

```
join(node):
    tailscale up, tag the node (tag:socrates-worker)
    docker swarm join --token <worker token> \
        --advertise-addr <node tailnet IP> <manager tailnet IP>:2377
    on manager: docker node update --label-add pool=voice-worker <node>
    on manager: docker node update --label-add gpu=true <node>    # 有 GPU 才加

leave(node):
    docker node update --availability drain <node>
    on node:    docker swarm leave
    on manager: docker node rm <node>
    remove tailscale tag / device
```

### 3.4 網路與連線

- `app` 與 cascade 之間：`CASCADE_REALTIME_URL` 指向 cascade 的 Swarm service 位址；cascade 的 `session_nodes.llm.base_url` 指向 `app` 的 `/internal/cascade/v1`。後者要求 `app` 能被工作池經 tailnet 連到。現行部署 `app` 綁 `127.0.0.1:3000` 並經 Funnel 對外，**需另外提供只在 tailnet 內可達的監聽位址**，具體做法（額外埠、或讓 `app` 也加入 overlay）待階段 2 決定。
- 內部端點的存取控制：`check_origin`（`lib.rs`）對 `/internal/cascade/` 路徑跳過 CSRF 的 Origin 檢查，防護靠共用密鑰。若 Funnel 把整個 `app` 對公網開放，這些路徑是否同樣可從公網打到，需在階段 2 確認並視需要在代理層擋掉（尚未驗證）。
- `CASCADE_LLM_TOKEN` 輪替：同一個值要同步更新 `app` 與所有 cascade 副本；人員離開團隊或節點遺失時輪替。

## 4. 安全

| 項目 | 做法 |
| --- | --- |
| 節點間通訊 | 僅走 tailnet；Swarm 三個埠只在 tailnet 內可達 |
| 節點身分 | Tailscale tag ＋ ACL：工作池節點只能連 manager 與 registry，不能連成員其他裝置，也不能連 PostgreSQL（5432） |
| 憑證隔離 | 工作池不持有資料庫憑證與 OpenAI 金鑰；`CASCADE_LLM_TOKEN` 以 Swarm secret（`file` 來源）掛入，不寫進環境變數與版控 |
| join token | 屬秘密；人員離開、節點遺失或外洩時 `join-token --rotate`，固定每 6 個月輪替 |
| 成員節點信任等級 | 半信任：容器非 root，不掛 `hostPath` 與 Docker socket |
| 日誌 | 沿用 AGENTS.md：不記密碼、token、金鑰、聊天原文、語音逐字稿；容器日誌用 `local` driver 並限制大小 |
| 授權測試 | 不因部署方式放寬：未登入、角色不符、名單外帳號、存取他人資料等既有測試照常通過 |
| 本機保護 | service 設定 CPU／記憶體上限；成員可隨時 `drain` 自己的節點 |

## 5. 實作階段與驗收

每階段結束都有 go／no-go：驗收不過就停在該階段，回頭修正計劃。

### 階段 0：規格與決策（阻擋後續）

- 開規格 Issue（暫名 S-09.4「對話服務工作池」），掛在 S-09 底下。
- 驗收：第 10 節的待決事項至少 1、2、3、4 有答案。

### 階段 1：單台 GPU 成員機器（不上 Swarm）

- 一台成員機器以 Compose 跑 cascade＋ASR＋TTS，`app` 以 `CASCADE_REALTIME_URL` 指向它的 tailnet 位址（如 `cascade-voice-spike.md` 的做法，但跨機器）。
- 全部使用合成帳號與合成對話。
- 驗收：另一台機器的瀏覽器經 `app` 完成一場 `zh-TW` 即時語音對話；cascade 回呼 `app` 內部端點成功；工作池機器上沒有資料庫憑證與 OpenAI 金鑰。
- **go／no-go**：延遲相較於 `cascade-voice-spike.md` 的單機實測沒有不可接受的退化（門檻與負責人訂定）。不通過就不繼續。

### 階段 2：Swarm 工作池

- 1 個 manager（可信核心）＋ 2 個以上 GPU worker。
- 驗證項目：
  - 加入、`drain`、離開、`docker node rm`。
  - 把階段 1 的 Compose 轉成 stack 檔（改用 registry 映像），記錄哪些選項需要改寫。
  - cascade 副本間分流：兩場同時的語音對話是否分到不同副本；單一副本的並行場次上限（尚未測量，需先量）。
  - worker 關機時：進行中的語音場次前端行為、新場次是否自動改用其他副本或降級為瀏覽器語音。
  - 沒有 constraint 的 service 是否被排到 worker（驗證 3.3 的推論）。
  - 3.4 的兩項網路確認（tailnet 內監聽位址、`/internal/cascade/` 是否可從公網打到）。
  - Tailscale ACL 確實擋住 worker 連 PostgreSQL。
- 驗收：關閉任一 worker，文字對話零影響，語音對話降級可用；`app` 日誌與管理介面不含訊息原文。
- 若 Docker Desktop／WSL2 節點無法穩定加入，只收 Linux 節點，或退回階段 1 的 Compose 做法。

### 階段 3：容量與負載驗證

- 依 S-10.3 的負載測試方法（約 60 人同時使用）驗證單實例 `app` 是否足夠；文字對話的瓶頸多半在 AI 供應商延遲而非 `app` CPU，但這是推論，未測量。
- 驗收：量到 `app` 的 CPU、記憶體、資料庫連線在目標負載下的實際數字，再決定是否需要階段 5。

### 階段 4：維運文件

- 新增 `docs/` 內的節點加入／退出／撤銷手冊，格式比照 `docs/ntnu-ops.md`；`CASCADE_LLM_TOKEN` 與 join token 輪替步驟；manager 損毀的重建步驟。
- 驗收：另一位成員只依文件完成加入與退出。

### 階段 5（有條件，另案）：`app` 多實例

只在階段 3 證明單實例不夠時才做。前置工作都是程式與規格變更，需各自開 Issue：

1. 用量（`ai_usage`／`user_limit`）、登入頻率限制、`speech_busy` 改存 PostgreSQL 或其他共享儲存。
2. `LiveReplies` 的接回改成路由層黏著（依對話 ID 一致性雜湊到同一實例），或把已產生文字存到共享儲存。
3. `voice_sessions` 同理：cascade 回呼必須命中同一實例。
4. 路由層：Swarm routing mesh 沒有依 URL 路徑做黏著的能力，需要另加反向代理（nginx、Caddy、HAProxy 等；各自的一致性雜湊能力本文未查證）。
5. 重新檢視「app 實例不得放在成員電腦」這條原則：多實例若放在成員節點，這些節點就會持有資料庫憑證與 OpenAI 金鑰，與 3.1 衝突。建議多實例的 `app` 仍放在可信機器。

## 6. 風險

| 風險 | 影響 | 對策 |
| --- | --- | --- |
| 成員節點看得到語音與回覆文字，不受存取紀錄機制約束 | 個資與規格底線 | 第 2.1、10 節：需求方決策前只用合成資料 |
| `app` 單實例、單機 | 該機器故障，對話服務整體停擺 | 本計劃不處理 `app` 的可用性；屬 S-09.1；至少備份與重建手冊 |
| 工作池節點離線 | 即時語音中斷 | 降級為瀏覽器語音；階段 2 驗證中途斷線行為 |
| 內部端點被公網打到 | 密鑰猜測或濫用 | 階段 2 確認並在代理層擋掉；密鑰為高熵隨機值 |
| Swarm 沒有 taint，漏寫 constraint | 服務落到不該去的節點 | CI 檢查每個 service 都有 constraint |
| VXLAN 埠無身分驗證 | 流量偽造 | 只在 tailnet 內開放；ACL 限制 |
| Docker Desktop／WSL2 節點不穩 | 節點名單受限 | 階段 2 的驗證；必要時只收 Linux 節點 |
| 單一 GPU 的並行場次上限未知（6 GB 顯示卡） | 容量規劃失準 | 階段 2 先量測再決定副本數 |
| 語音方案本身仍屬暫定（部署與 GPU 規格待定） | 叢集投資可能白費 | 階段 1 為低成本驗證；規格定案前不擴大 |
| 本文未查證 Swarm 的長期維護方向 | 未來可能需要遷移 | 工作池為無狀態、可拋棄設計，遷移成本低 |
| 維運負責人不明 | 成為技術債 | 階段 0 指定負責人與備援（S-09.3 尚未決定人選） |

## 7. 與既有規格的關係

- 不改變 S-08.1（同網域部署）、S-08.3、S-09.3；不取代 S-09.1。
- 語音側沿用 `cascade-voice-spike.md` 與 S-05.5 的暫定決策；本計劃只提供執行位置，不決定正式採用。
- 與 S-10.3 的關係：階段 3 用其負載測試方法驗證容量。

## 8. 範圍外

- 正式環境的高可用與 `app` 的自動容錯。
- 文字對話所用 OpenAI 模型的選擇（S-03.4）與改用自架 LLM。後端雖可用 `OPENAI_BASE_URL` 指向相容服務，但規格決定 AI 使用 OpenAI（S-03.1），改變屬規格議題。
- CI、預覽環境、前端獨立部署。

## 9. 假設

以下為撰寫時的假設，若不成立需修正計劃：

- 叢集的主要動機是分擔語音模型的 GPU 運算，以及未來可能的容量需求；而不是要讓 `app` 本身在成員電腦上多實例執行。
- 工作池與 `app` 之間的延遲（tailnet 內）可被即時語音接受。

## 10. 待決事項（需需求方或團隊回答）

1. **是否允許成員電腦處理真實學生的對話或音訊？** 若否，叢集僅限合成資料，不能服務真實教室。若是，需補存取規範與告知學生的方式。
2. 叢集要解決的問題：語音 GPU 分流、對話容量、可用性，還是三者？優先順序？
3. 是否同意把 Docker Swarm 列為新增的部署機制？是否同意引入 registry（GHCR）？
4. 可信核心放在哪台機器？誰負責維運，備援是誰？（也是 S-09.3 尚未決定的人選。）
5. 成員可提供的作業系統與 GPU 清單，以及是否接受 Docker Desktop／WSL2 節點。
6. `/internal/cascade/` 是否需要與公開入口分開（獨立監聽位址或代理規則）。
7. 成員參與是否自願、可隨時退出，以及機器資源上限的共識。
