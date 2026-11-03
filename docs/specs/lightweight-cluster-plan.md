# 輕量化叢集計劃書（可加入團隊成員電腦作為節點）

> **狀態：草案（2026-11-03），不構成規格決策。** 正式環境部署方式（S-09.1）仍未定案；本文只規劃**非正式環境**的共用運算資源，並列出需要需求方或團隊確認的事項。確認後依 [CONTRIBUTING.md](../../CONTRIBUTING.md#規格決策流程) 以 PR 更新 `decisions.md`，再開始實作。

## 1. 背景與目標

目前部署狀態：

- 開發與 CI 用 Docker Compose（`docker-compose.yml`，S-09.1）。
- 試用環境為單一台 NTNU 機器，以 Docker Compose 部署，經 Tailscale Funnel 對外（`docs/ntnu-ops.md`）。
- 伺服器端語音（cascade、Whisper）已在成員的 RTX 4050 筆電（WSL2）上實測，需要 GPU 的工作目前沒有共用的執行位置。
- 後端 CI 需要編譯 Rust、跑 PostgreSQL 整合測試與覆蓋率，耗時長。

目標：讓團隊成員的電腦能**以低成本、可隨時加入與退出**的方式成為共用節點，提供：

1. 每個分支或 PR 一套獨立的預覽環境。
2. 自架 CI runner。
3. GPU／重運算工作（cascade、Whisper、`backend/eval` 的 AI 行為評測）。

非目標：

- **不承載正式環境，不放真實學生資料。**
- 不追求控制平面高可用。
- 不引入 Supabase、Redis 等 AGENTS.md 未列出的基礎設施（叢集本身屬新增基礎設施，需先經規格討論，見第 9 節）。

## 2. 設計前提（先說明會限制方案的事實）

### 2.1 成員電腦不適合當正式環境

- 會休眠、關機、換網路；家用或校園 NAT 後方沒有固定位址。
- 機器的使用者與 root 可讀取該節點上所有 Pod 的環境變數與掛載內容。正式環境的資料庫、OpenAI 金鑰與真實個資（S-02）不得落在這類機器上。
- AGENTS.md 要求測試資料只用合成資料；預覽環境符合此限制，正式環境不符合。

### 2.2 後端目前不能直接橫向擴展

後端有存在程序記憶體的狀態（規格已載明）：

| 狀態 | 影響 | 出處 |
| --- | --- | --- |
| 進行中的 AI 回覆 | 斷線重連只能接回同一程序；其他程序回 `409 reply_in_progress` | S-08.1 |
| 登入頻率限制、失敗計數的部分 | 多實例各自計算 | S-08.2、`technical-details.md` |
| 語音 `speech_busy` 限制 | 多實例各自計算 | S-05.5 |
| `/admin/logs` 近期請求 | 只含當前程序 | S-09.3 |

結論：叢集的價值**不是**把同一個服務開多份來擴容，而是**把彼此獨立的環境與工作排到不同機器**。每個環境（預覽、試用）內的 `app` 維持單一副本。若日後要讓單一環境多副本，需先決定如何處理上表（sticky routing 或把狀態移到 PostgreSQL），這屬於另一個規格議題，不在本計劃內。

### 2.3 節點隨時會消失

所有放到成員節點的工作負載必須可丟棄、可重排程：預覽環境可重建、CI job 可重跑、GPU 服務有備援或可降級（語音功能預設關閉，缺設定時不啟用）。需要持久狀態的東西只放在固定的 server 節點。

## 3. 方案選擇

| 方案 | 優點 | 缺點 |
| --- | --- | --- |
| A. Tailscale ＋ 各機器 Docker Compose ＋ 自架 runner（無叢集） | 最少新元件；與現行部署一致 | 沒有排程；預覽環境要自己管埠與網址；節點增減靠人工 |
| B. k3s（單 server ＋ 多 agent）＋ Tailscale | 排程、標籤與污點、namespace 隔離、宣告式部署；加入節點只要一行指令 | 新增 Kubernetes 概念與維運負擔；成員機器作業系統不一（見 3.1） |
| C. Docker Swarm、Nomad 等 | 比 Kubernetes 輕 | 本文未逐項查證其在 Windows／macOS／WSL2 節點上的行為，不列入建議 |

**建議：B（k3s ＋ Tailscale）**，因為需求明確是「加入他人電腦當 server」並自動排程工作。但 B 的價值取決於預覽環境與排程是否真的需要；若實際需求只有 CI runner 與 GPU 服務，A 更划算。因此第 6 節的階段 1 先以最小範圍驗證，再決定是否繼續。

選 k3s 的依據（官方文件）：

- 單一二進位檔；agent 最低需求 1 core、512 MB RAM，建議 2 core、1 GB；server 加一個簡單工作負載實測約 1.6 GB 記憶體。
- 預設資料儲存為 SQLite（單 server）；SQLite 不能用於多 server。要 HA 需 3 個以上 server 搭配 embedded etcd，或 2 個以上 server 搭配外部資料庫（PostgreSQL 在支援清單內）。本計劃採單 server，不做 HA。
- agent 以 `K3S_URL` 與 `K3S_TOKEN` 加入；每台機器 hostname 必須唯一，否則設定 `K3S_NODE_NAME`。
- `--node-label`、`--node-taint` 只在註冊時生效，事後要改得用 `kubectl`。
- 跨網路（NAT 後方）可透過 Tailscale 整合：`k3s agent --vpn-auth="name=tailscale,joinKey=<key>" --server https://<server 的 tailnet IP>:6443 --node-external-ip=<agent 的 tailnet IP>`，並在 Tailscale 管理介面核准節點與子網路。

## 4. 架構

```
                      tailnet（既有 Tailscale）
   ┌────────────────────────────────────────────────────────┐
   │                                                        │
   │  server 節點（固定、常駐，例如 NTNU 機器）               │
   │   - k3s server（SQLite）                                │
   │   - 映像 registry 憑證、叢集 token                      │
   │   - 預覽環境專用 PostgreSQL（合成資料）                  │
   │                                                        │
   │  agent 節點（成員電腦，pool=dev-laptop）                 │
   │   - taint: dedicated=dev-laptop:NoSchedule              │
   │   - label: socrates.dev/gpu=true（有 GPU 者）           │
   │   - 預覽 Pod、CI runner Pod、GPU 服務 Pod               │
   └────────────────────────────────────────────────────────┘
        成員以 tailnet 位址存取預覽環境；不使用 Funnel，不對公網開放
```

### 4.1 排程規則（偽代碼）

```
for each workload:
    if workload.class == "production":
        refuse                                  # 本叢集不承載正式環境
    if workload.class in {"preview", "ci", "gpu"}:
        tolerate(taint "dedicated=dev-laptop")  # 明確同意才會排到成員電腦
        require(label "socrates.dev/gpu=true") if workload.needs_gpu
    set resource requests and limits            # 避免吃光成員本機資源
    set restart policy so that node loss => reschedule or fail fast
```

節點上線／退出：

```
join(node):
    install tailscale, tag the node (tag:socrates-node)
    install k3s agent with K3S_URL + K3S_TOKEN + labels + taint
    admin verifies: kubectl get node shows Ready

leave(node):
    kubectl cordon <node>
    kubectl drain <node> --ignore-daemonsets
    uninstall k3s agent
    kubectl delete node <node>
    remove tailscale tag / device
```

### 4.2 預覽環境

- 每個 PR 一個 namespace：一個 `app` Pod（沿用現有單一映像，前後端同網域，S-08.1）加一個資料庫。
- 資料庫放在 server 節點，或每個預覽各用一個 Pod 搭配 ephemeral volume；只含合成資料（可沿用 `scripts/seed-demo.sh` 的做法）。PR 關閉即刪除整個 namespace。
- 每個預覽環境的 `APP_BASE_URL` 必須設為該環境自己的網址：後端 CSRF 檢查要求 `Origin` 等於 `APP_BASE_URL`（見 `docs/ntnu-ops.md`），沿用 `.env` 的值會讓預覽環境的寫入請求被拒。
- 預覽環境的 migration：沿用 `RUN_MIGRATIONS=true`（規格僅把它定位為本機／試用便利設定，S-09.3，正式環境不得使用）。
- **AI 金鑰不得進成員節點。** 預覽環境需使用 mock AI 提供者，或具額度上限的專用金鑰。具體做法待決定（S-03.1 已把 AI 呼叫包成單一介面，可實作 mock；是否需要新增是規格議題）。

### 4.3 CI runner

- 使用 GitHub 自架 runner，註冊時以 `--labels` 加自訂標籤，workflow 以 `runs-on: [self-hosted, <label>]` 指定；`--ephemeral` 讓 runner 只接一個 job 後即註銷，降低髒環境與憑證殘留風險。
- **前置條件：確認 repo 為私有，或 workflow 不讓 fork 的 PR 觸發自架 runner。** 公開 repo 上，外部貢獻者的 PR 可在成員電腦上執行任意程式碼。repo 目前的可見性與 fork PR 設定未在本次調查中確認。
- 現有 `ci.yml` 的 job 全部使用 `ubuntu-latest`。是否轉移哪些 job、轉移後與 GitHub 託管 runner 並存或取代，需在階段 3 實測編譯時間後決定，不預設全部搬移。
- 執行 job 的 Pod 不得掛載 `hostPath` 與 Docker socket；PostgreSQL 服務以 sidecar 或同 Pod 容器提供。

### 4.4 GPU 工作

- 有 GPU 的成員節點標 `socrates.dev/gpu=true`；cascade、Whisper 服務（`scripts/whisper-server.py`、`scripts/cascade-launch.example.yaml`）以 nodeSelector 排上去。
- GPU 在 Kubernetes 的暴露方式（device plugin、WSL2 的 GPU 穿透）**本文未查證**，列為階段 4 的 PoC 項目；若 WSL2 節點無法穩定提供 GPU，退回方案 A（該機器以 Compose 直接跑，後端以 `CASCADE_REALTIME_URL` 指向 tailnet 位址）。
- 這類服務在規格中仍是暫定（`cascade-voice-spike.md`：部署與 GPU 規格待定），本計劃只提供執行位置，不決定正式採用。

## 5. 網路與安全

| 項目 | 做法 |
| --- | --- |
| 節點間通訊 | 僅走 tailnet；k3s API（6443）不對公網開放 |
| 節點身分 | Tailscale tag（`tag:socrates-node`）＋ ACL：節點只能連 server 與 registry，不能連成員其他裝置；加入用的 join key 設到期且只用於 tag 節點 |
| 叢集 token | `node-token` 屬秘密，不進版控、不貼 Issue 或聊天；人員離開團隊或 token 外洩時輪替並移除對應節點 |
| 成員節點信任等級 | 視為半信任：不放正式金鑰、不放真實資料；Pod 以非 root 執行，不掛 `hostPath`，用 NetworkPolicy 限制預覽 namespace 之間互通 |
| 日誌 | 沿用 AGENTS.md 規範：不記密碼、token、金鑰、聊天原文；Pod 日誌留在節點本機，輪替上限比照 compose 的 `local` driver（20m x 5） |
| 本機保護 | 設定 CPU／記憶體上限，並提供暫停指令（`cordon` ＋ 停止 agent），讓成員隨時收回電腦；具體 kubelet 保留資源參數待 PoC 驗證 |

授權檢查與資安測試證據的要求（AGENTS.md）不因環境不同而放寬：預覽環境仍須通過既有的授權測試。

## 6. 實作階段與驗收

每階段結束都有 go／no-go：任一階段驗收不過，就停在該階段並回頭修正計劃，不硬推進。

### 階段 0：規格確認（阻擋後續）

- 開規格 Issue（暫名 S-09.4「共用運算叢集」），掛在 S-09 底下，並在 `decisions.md` 記錄需求方確認的範圍。
- 驗收：第 9 節的待決事項至少 1、2、3、5 有答案。

### 階段 1：最小 PoC

- 1 個 server ＋ 1 個成員 agent（優先 Linux 原生機器，其次 WSL2），透過 Tailscale 連線。
- 部署一個最簡單的 Pod 到 agent，並驗證：加入、`cordon`／`drain`、關機後 Pod 被重排程、重新連線後恢復 Ready。
- 驗收：`kubectl get nodes` 顯示 Ready；節點斷線 5 分鐘後工作被重排到其他節點或明確標為 Pending（兩者擇一，並記錄實測行為）；Tailscale ACL 確實擋住節點存取不該連的裝置。
- **go／no-go**：若成員的主要作業系統（Windows、macOS）無法穩定當 agent，改走方案 A，不繼續階段 2 的 k3s 部分。

### 階段 2：預覽環境

- 以 manifests 或 Helm chart（擇一，Helm 屬新工具，需列入第 9 節）定義單一預覽環境。
- CI 在 PR 開啟時建置映像（多架構需求見 7）、推到 registry、建立 namespace；PR 關閉時清理。
- 驗收：一個 PR 對應一個可用網址；以合成資料完成登入與一次完整對話；PR 關閉後 namespace 與資料庫全部清除；預覽環境的寫入請求（CSRF）通過。

### 階段 3：自架 runner

- 在 agent 上註冊 ephemeral runner；選一個 job（建議後端測試）遷移。
- 驗收：同一 commit 在自架與託管 runner 上結果一致；記錄兩者耗時；fork PR 不會觸發自架 runner。

### 階段 4：GPU 節點

- PoC GPU 暴露方式；cascade 或 Whisper 在 GPU 節點上通過 `cascade-voice-spike.md` 既有的實測項目。
- 驗收：與該文件記錄的單機結果相比，延遲沒有顯著退化（門檻在階段 4 開始時與負責人訂定）。

### 階段 5：維運文件

- 新增 `docs/` 內的節點加入／退出／撤銷手冊，與 `docs/ntnu-ops.md` 同格式；列出 server 節點損毀時的重建步驟（SQLite 備份與還原演練一次）。
- 驗收：另一位成員只依文件完成加入與退出，無需口頭補充。

## 7. 主要風險

| 風險 | 影響 | 對策 |
| --- | --- | --- |
| 成員機器作業系統不一（k3s 以 Linux 為主；Windows、macOS 需 WSL2 或 VM，本文未驗證其穩定度） | 節點名單受限、維運成本高 | 階段 1 的 go／no-go；必要時只收 Linux／WSL2 節點 |
| CPU 架構不一（x86_64 與 Apple Silicon） | 映像需多架構建置，Rust 編譯時間倍增 | 初期限定 linux/amd64 節點 |
| server 節點單點故障 | 叢集控制面停擺，但已在節點上的 Pod 仍會續跑一段時間 | 接受；以 SQLite 定期備份與重建手冊緩解，不做 HA |
| 節點資源被預覽或 CI 吃滿，影響成員本機工作 | 成員不願加入 | 資源上限、一鍵暫停、預設只在使用者設定的時段開放 |
| 公開 repo 的 fork PR 在成員電腦上執行任意程式 | 成員機器被入侵 | 階段 3 前置條件；不通過就不啟用 runner |
| 預覽環境誤用真實金鑰或真實資料 | 違反資安與個資底線 | 預覽 namespace 不掛載真實 secret；CI 檢查 |
| 維運負責人不明 | 叢集無人管，成為技術債 | 階段 0 指定負責人與備援（S-09.3 尚未決定人選，此處同樣需要） |

## 8. 與既有規格的關係

- 不改變 S-08.1（同網域部署）、S-08.3（環境變數與 `sqlx migrate`）、S-09.3（正式環境不自動 migrate）。
- 不取代 S-09.1 的正式環境決策；若最後正式環境也選 Kubernetes，本叢集的 manifests 可作為起點，但資料、金鑰與節點信任模型需另案設計。
- 與 S-10.3（約 60 人同時使用）無關：預覽環境不做負載驗收；負載測試仍針對正式環境候選方案。

## 9. 待決事項（未定案，需需求方或團隊回答）

1. 叢集用途範圍：預覽環境、CI runner、GPU 工作三者是否都要做？優先順序？
2. 是否同意把 k3s 列為新增基礎設施（AGENTS.md：新增主要依賴需先經規格討論）？Helm 是否一併同意？
3. server 節點放在哪台機器？誰負責維運，備援是誰？
4. 成員可提供的作業系統與 GPU 清單，以及是否接受 WSL2 節點。
5. repo 目前是公開或私有？fork PR 是否會觸發 workflow？（決定階段 3 能否進行。）
6. 預覽環境的 AI 呼叫用 mock 還是限額金鑰？由誰核可。
7. 預覽環境的存取範圍：僅 tailnet 內，或需要給教師／需求方試用（若需要，需另案評估公開暴露與個資風險）。
8. 成員參與是否自願、可隨時退出，以及機器資源上限的共識。

## 10. 範圍外

- 正式環境部署與 HA。
- 單一環境內後端多副本（需先處理第 2.2 節的程序記憶體狀態）。
- 自動擴縮容、服務網格、集中式日誌或監控平台。
