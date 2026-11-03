# 輕量化叢集計劃書（可加入團隊成員電腦作為節點）

> **狀態：草案（2026-11-03，第二版：主方案改為 Docker Swarm），不構成規格決策。** 正式環境部署方式（S-09.1）仍未定案；本文只規劃**非正式環境**的共用運算資源，並列出需要需求方或團隊確認的事項。確認後依 [CONTRIBUTING.md](../../CONTRIBUTING.md#規格決策流程) 以 PR 更新 `decisions.md`，再開始實作。

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
- 不動現有 NTNU 試用環境：它維持獨立的 Docker Compose，不加入 Swarm。
- 不引入 Supabase、Redis 等 AGENTS.md 未列出的基礎設施。Swarm 內建於 Docker Engine，不需額外安裝元件，但仍是新的部署機制，需先經規格討論（第 9 節）。

## 2. 設計前提

### 2.1 成員電腦不適合當正式環境

- 會休眠、關機、換網路；家用或校園 NAT 後方沒有固定位址。
- 機器的使用者與 root 可讀取該節點上所有容器的環境變數與掛載內容（包含 Swarm 派送到該節點的 secret）。正式環境的資料庫、OpenAI 金鑰與真實個資（S-02）不得落在這類機器上。
- AGENTS.md 要求測試資料只用合成資料；預覽環境符合此限制，正式環境不符合。

### 2.2 後端目前不能直接橫向擴展

後端有存在程序記憶體的狀態（規格已載明）：

| 狀態 | 影響 | 出處 |
| --- | --- | --- |
| 進行中的 AI 回覆 | 斷線重連只能接回同一程序；其他程序回 `409 reply_in_progress` | S-08.1 |
| 登入頻率限制、失敗計數的部分 | 多實例各自計算 | S-08.2、`technical-details.md` |
| 語音 `speech_busy` 限制 | 多實例各自計算 | S-05.5 |
| `/admin/logs` 近期請求 | 只含當前程序 | S-09.3 |

結論：叢集的價值**不是**把同一個服務開多份來擴容，而是**把彼此獨立的環境與工作排到不同機器**。每個環境內的 `app` 維持單一副本（`replicas: 1`）。Swarm 的 routing mesh 會把請求分散到服務的各個 task，副本數設為 1 即可避免此問題；日後若要多副本，需先處理上表（另案）。

### 2.3 節點隨時會消失

所有放到成員節點的工作負載必須可丟棄、可重排程：預覽環境可重建、CI job 可重跑、GPU 服務可降級（語音功能預設關閉，缺設定時不啟用）。需要持久狀態的東西只放在固定的 manager 節點，或留在 Swarm 之外。

## 3. 方案選擇

| 方案 | 優點 | 缺點 |
| --- | --- | --- |
| A. Tailscale ＋ 各機器 Docker Compose ＋ 自架 runner（無叢集） | 最少新元件；與現行部署一致 | 沒有排程；預覽環境要自己管埠與網址；節點增減靠人工 |
| **B. Docker Swarm ＋ Tailscale（建議）** | 成員電腦本來就要裝 Docker；stack 檔與現有 Compose 檔格式相近；沒有額外二進位檔；加入／退出各一行指令；有排程、placement constraint、secret | 見 3.1 |
| C. k3s ＋ Tailscale | 有 namespace、NetworkPolicy、內建排程標籤與污點機制 | 新增 Kubernetes 概念與維運負擔；成員機器作業系統不一時 agent 難裝（k3s 以 Linux 為主） |

建議 B，原因：需求是「輕量」且「加入他人電腦」，團隊與 repo 已經以 Docker Compose 為基礎；Swarm 的概念（manager、worker、service、stack）遠少於 Kubernetes。C 保留為備案：若預覽環境之間需要網路層隔離（NetworkPolicy）或需要內建的網址路由，再改用 C。

### 3.1 Swarm 的限制（官方文件確認的部分）

- `docker stack deploy` 會忽略 Compose 的 `build`（輸出 `Ignoring unsupported options: build`）。現有 `docker-compose.yml` 的 `app` 用 `build: .`，因此 **stack 要用預先建好並推到 registry 的映像**，需另寫一份 stack 檔，不能直接重用 `docker-compose.yml`。`profiles`、`depends_on` 等其他選項在 stack 模式下的行為，本文未逐項查證，列為階段 1 檢查項目。
- 頂層 `secrets` 的 `environment` 來源**不支援** `docker stack deploy`，只能用 `file` 來源（現有 `.apikeylist` 即是 `file`，不受影響）。
- 放置規則用 `deploy.placement.constraints`，例如 `node.labels.pool==dev-laptop`；找不到符合條件的節點時 task 停在 `Pending`。
- overlay 網路需要的埠：`2377/tcp`（manager 控制）、`7946/tcp+udp`（節點探索）、`4789/udp`（VXLAN 資料）。官方特別說明 VXLAN **沒有身分驗證**，只能對可信網路開放，絕不可開在邊界防火牆；網路不完全可信時建議用加密 overlay（`--opt encrypted`，需要 IP protocol 50 ESP 通過）。
- manager 若多個，必須為奇數（建議 3 或 5），且需過半 manager 存活才能運作；成員筆電不適合當 manager。
- 各節點必須用對方可連到的位址加入：`docker swarm init --advertise-addr <位址>`，多網卡或 NAT 時必須明確指定。
- join token 是加入叢集的秘密，不得進版控；官方建議至少每 6 個月 `docker swarm join-token --rotate`，輪替不會踢掉已加入的節點。

以下兩點是**依設計推論、尚未實測**，列為階段 1 的驗證項目：

- Swarm **沒有 taint**。k3s 可用污點讓 Pod 預設不上成員電腦；Swarm 沒有同等機制，任何沒寫 constraint 的 service 都可能被排到成員電腦。對策見 4.1。
- Docker Desktop（Windows、macOS）的引擎跑在虛擬機內，節點位址與 `2377`、`7946`、`4789` 能否從 tailnet 正確連到，需實測；WSL2 同理。若不行，該節點只能走方案 A。

## 4. 架構

```
                      tailnet（既有 Tailscale）
   ┌────────────────────────────────────────────────────────┐
   │                                                        │
   │  manager 節點（固定、常駐，例如 NTNU 機器）              │
   │   - Swarm manager（單一，不做 HA）                      │
   │   - 預覽環境的 PostgreSQL 可放這裡（合成資料）           │
   │   - NTNU 試用環境仍是獨立 Compose，不加入 Swarm         │
   │                                                        │
   │  worker 節點（成員電腦，label pool=dev-laptop）          │
   │   - 預覽 stack、GPU 服務                                 │
   │   - label gpu=true（有 GPU 者）                          │
   └────────────────────────────────────────────────────────┘
        成員以 tailnet 位址存取預覽環境；不使用 Funnel，不對公網開放
        CI runner 直接裝在 worker 主機上，不放進 Swarm（見 4.3）
```

### 4.1 排程規則（偽代碼）

Swarm 沒有 taint，因此用「兩層 constraint」模擬：

```
for each stack file under deploy/:
    if service.class == "production":
        refuse                                   # 本叢集不承載正式環境
    if service.class in {"preview", "gpu"}:
        require constraint "node.labels.pool == dev-laptop"
        require constraint "node.labels.gpu == true" if service.needs_gpu
        require resources.limits (cpu, memory)   # 避免吃光成員本機資源
        require replicas == 1 for app            # 見 2.2
    if service.class == "infra":                 # 例如預覽共用的資料庫
        require constraint "node.role == manager"

CI check: reject any stack file whose service has no placement constraint
```

「每個 service 都必須有 constraint」由 CI 檢查，而不是靠人記得，因為漏寫會讓服務默默落到某位成員的電腦。

節點上線與退出：

```
join(node):
    tailscale up, tag the node (tag:socrates-node)
    docker swarm join --token <worker token> \
        --advertise-addr <node tailnet IP> <manager tailnet IP>:2377
    # 在 manager 上設定 label（Swarm 的 label 由 manager 管理，可事後更改）
    docker node update --label-add pool=dev-laptop <node>
    docker node update --label-add gpu=true <node>   # 有 GPU 才加
    verify: docker node ls shows Ready / Active

leave(node):
    docker node update --availability drain <node>   # task 被轉移，不再接新 task
    on the node: docker swarm leave
    on manager:  docker node rm <node>
    remove tailscale tag / device
```

### 4.2 預覽環境

- 每個 PR 一個 stack（`docker stack deploy -c deploy/preview.yml pr-<number>`）：stack 名稱會成為網路、volume、service 名稱的前綴，達到基本隔離。一個 `app`（沿用現有單一映像，前後端同網域，S-08.1）加一個 `db`。
- 映像由 CI 在 PR 時建置並推到 registry（候選：GHCR）；worker 從 registry 拉取。
- 資料庫 volume 是節點本機的：task 被重排到別的節點時資料庫會是空的。預覽資料全為合成資料，重建並重新 seed 即可（可沿用 `scripts/seed-demo.sh` 的做法）。PR 關閉時 `docker stack rm`。
- 每個預覽環境的 `APP_BASE_URL` 必須設為該環境自己的網址：後端 CSRF 檢查要求 `Origin` 等於 `APP_BASE_URL`（見 `docs/ntnu-ops.md`），沿用 `.env` 的值會讓寫入請求被拒。
- **網址路由**：Swarm 的 routing mesh 以「發佈埠」為單位，沒有內建依主機名稱分流。每個預覽用獨立埠最簡單（例如依 PR 編號配埠）；若要用網址分流，需額外加反向代理（Traefik、Caddy 等），那是新增元件，列入第 9 節。
- 預覽環境的 migration：沿用 `RUN_MIGRATIONS=true`（規格僅把它定位為本機／試用便利設定，S-09.3，正式環境不得使用）。
- **AI 金鑰不得進成員節點。** 預覽環境需使用 mock AI 提供者，或具額度上限的專用金鑰。具體做法待決定（S-03.1 已把 AI 呼叫包成單一介面，可實作 mock；是否新增屬規格議題）。

### 4.3 CI runner

- 使用 GitHub 自架 runner，註冊時以 `--labels` 加自訂標籤，workflow 以 `runs-on: [self-hosted, <label>]` 指定；`--ephemeral` 讓 runner 只接一個 job 後即註銷。
- **runner 直接安裝在成員主機上，不放進 Swarm。** runner 執行 job 時需要 Docker（例如 `services: postgres`）；把它放進 Swarm 容器就得掛載 Docker socket，等同交出該節點的 root。
- **前置條件：確認 repo 為私有，或 workflow 不讓 fork 的 PR 觸發自架 runner。** 公開 repo 上，外部貢獻者的 PR 可在成員電腦上執行任意程式碼。repo 目前的可見性與 fork PR 設定未在本次調查中確認。
- 現有 `ci.yml` 的 job 全部使用 `ubuntu-latest`。是否轉移哪些 job、並存或取代，需在階段 3 實測編譯時間後決定，不預設全部搬移。

### 4.4 GPU 工作

- 有 GPU 的成員節點標 `gpu=true`；cascade、Whisper 服務（`scripts/whisper-server.py`、`scripts/cascade-launch.example.yaml`）以 constraint 排上去。
- Docker daemon 支援用 `node-generic-resources`（`daemon.json`）向 Swarm 宣告自訂資源，例如 `NVIDIA-GPU=<UUID>`。service 如何請求這項資源，以及它在 WSL2／Docker Desktop 的實際行為，**本文未查證**，列為階段 4 的 PoC 項目。
- 若 Swarm 無法穩定提供 GPU，退回方案 A：該機器以 Compose 直接跑，後端以 `CASCADE_REALTIME_URL` 指向該機器的 tailnet 位址。
- 這類服務在規格中仍是暫定（`cascade-voice-spike.md`：部署與 GPU 規格待定），本計劃只提供執行位置，不決定正式採用。

## 5. 網路與安全

| 項目 | 做法 |
| --- | --- |
| 節點間通訊 | 僅走 tailnet；`2377`、`7946`、`4789` 只在 tailnet 內可達，不對公網開放（VXLAN 無身分驗證） |
| overlay 加密 | tailnet 本身已加密；是否仍啟用 `--opt encrypted`（需 ESP 通過、有額外開銷）於階段 1 評估 |
| 節點身分 | Tailscale tag（`tag:socrates-node`）＋ ACL：節點只能連 manager 與 registry，不能連成員其他裝置；Tailscale 加入用的 key 設到期 |
| join token | 屬秘密，不進版控、不貼 Issue 或聊天；人員離開團隊、節點遺失或 token 外洩時立即 `join-token --rotate`；固定每 6 個月輪替 |
| 成員節點信任等級 | 視為半信任：不放正式金鑰與真實資料；容器以非 root 執行，不掛 `hostPath` 與 Docker socket；Swarm 沒有 NetworkPolicy，預覽 stack 之間靠各自獨立的 overlay 網路隔離 |
| 日誌 | 沿用 AGENTS.md 規範：不記密碼、token、金鑰、聊天原文；容器日誌用 `local` driver 並限制大小（比照現有 compose 的 20m x 5） |
| 本機保護 | service 設定 CPU／記憶體上限；成員可隨時 `drain` 自己的節點收回電腦 |

授權檢查與資安測試證據的要求（AGENTS.md）不因環境不同而放寬：預覽環境仍須通過既有的授權測試。

## 6. 實作階段與驗收

每階段結束都有 go／no-go：任一階段驗收不過，就停在該階段並回頭修正計劃，不硬推進。

### 階段 0：規格確認（阻擋後續）

- 開規格 Issue（暫名 S-09.4「共用運算叢集」），掛在 S-09 底下，並在 `decisions.md` 記錄需求方確認的範圍。
- 驗收：第 9 節的待決事項至少 1、2、3、5 有答案。

### 階段 1：最小 PoC

- 1 個 manager ＋ 1 個成員 worker（優先 Linux 原生機器，其次 WSL2、Docker Desktop），透過 Tailscale 連線。
- 驗證項目：
  - 加入、`drain`、離開、`docker node rm`。
  - 把現有 compose 轉成 stack 檔（改用 registry 映像），確認 `depends_on`、`profiles`、healthcheck 等選項的實際行為，並記錄哪些需要改寫。
  - worker 關機後，task 是被重排到 manager 還是停在 `Pending`；重新連線後是否恢復。
  - 沒有 constraint 的 service 是否真的會被排到 worker（驗證 4.1 的推論）。
  - Tailscale ACL 確實擋住節點存取不該連的裝置。
- **go／no-go**：若成員的主要作業系統（Windows、macOS）無法穩定當 worker，且多數成員用這些系統，改走方案 A，不繼續階段 2。若發現預覽環境需要 NetworkPolicy 或依主機名稱分流且不想額外加代理，評估改用方案 C。

### 階段 2：預覽環境

- CI 在 PR 開啟時建置映像（初期僅 linux/amd64）、推到 registry、`docker stack deploy`；PR 關閉時 `docker stack rm`。
- 加入 4.1 的「每個 service 必須有 constraint」CI 檢查。
- 驗收：一個 PR 對應一個可用網址（或埠）；以合成資料完成登入與一次完整對話；PR 關閉後 stack 與 volume 清除；預覽環境的寫入請求（CSRF）通過。

### 階段 3：自架 runner

- 在 worker 主機上註冊 ephemeral runner；選一個 job（建議後端測試）遷移。
- 驗收：同一 commit 在自架與託管 runner 上結果一致；記錄兩者耗時；fork PR 不會觸發自架 runner。

### 階段 4：GPU 節點

- PoC GPU 暴露方式；cascade 或 Whisper 在 GPU 節點上通過 `cascade-voice-spike.md` 既有的實測項目。
- 驗收：與該文件記錄的單機結果相比，延遲沒有顯著退化（門檻在階段 4 開始時與負責人訂定）。

### 階段 5：維運文件

- 新增 `docs/` 內的節點加入／退出／撤銷手冊，與 `docs/ntnu-ops.md` 同格式；列出 manager 損毀時的重建步驟（Swarm 狀態的備份與還原演練一次）。
- 驗收：另一位成員只依文件完成加入與退出，無需口頭補充。

## 7. 主要風險

| 風險 | 影響 | 對策 |
| --- | --- | --- |
| 成員用 Docker Desktop 或 WSL2，節點位址與 Swarm 埠不一定能從 tailnet 連到（未驗證） | 節點名單受限 | 階段 1 的 go／no-go；必要時只收 Linux 節點 |
| Swarm 沒有 taint，漏寫 constraint 的服務會落到成員電腦 | 非預期資源占用，或不該上的服務上了不可信節點 | 4.1 的 CI 檢查；階段 1 實測確認行為 |
| 單一 manager 故障 | 無法管理叢集（既有 task 是否續跑需實測） | 接受；定期備份 Swarm 狀態與重建手冊，不做 HA（HA 需 3 個以上穩定 manager，不適合筆電） |
| VXLAN 埠無身分驗證，若誤對公網或不可信網路開放 | 流量被偽造或竊聽 | 只在 tailnet 內開放；ACL 限制；評估加密 overlay |
| CPU 架構不一（x86_64 與 Apple Silicon） | 映像需多架構建置，Rust 編譯時間倍增 | 初期限定 linux/amd64 節點 |
| 節點資源被預覽或 CI 吃滿，影響成員本機工作 | 成員不願加入 | 資源上限、隨時 `drain`、預設只開放使用者同意的時段 |
| 公開 repo 的 fork PR 在成員電腦上執行任意程式 | 成員機器被入侵 | 階段 3 前置條件；不通過就不啟用 runner |
| 預覽環境誤用真實金鑰或真實資料 | 違反資安與個資底線 | 預覽 stack 不掛載真實 secret；CI 檢查 |
| 本文未查證 Swarm 的長期維護方向 | 未來可能需要遷移 | 預覽環境為可拋棄設計，遷移成本低；選型時由團隊自行確認 |
| 維運負責人不明 | 叢集無人管，成為技術債 | 階段 0 指定負責人與備援（S-09.3 尚未決定人選，此處同樣需要） |

## 8. 與既有規格的關係

- 不改變 S-08.1（同網域部署）、S-08.3（環境變數與 `sqlx migrate`）、S-09.3（正式環境不自動 migrate）。
- 不取代 S-09.1 的正式環境決策；NTNU 試用環境維持現狀。
- 與 S-10.3（約 60 人同時使用）無關：預覽環境不做負載驗收。

## 9. 待決事項（未定案，需需求方或團隊回答）

1. 叢集用途範圍：預覽環境、CI runner、GPU 工作三者是否都要做？優先順序？
2. 是否同意把 Docker Swarm 列為新增的部署機制？是否同意為預覽環境引入 registry（GHCR）與（若要依網址分流）反向代理？
3. manager 節點放在哪台機器？誰負責維運，備援是誰？
4. 成員可提供的作業系統與 GPU 清單，以及是否接受 Docker Desktop／WSL2 節點。
5. repo 目前是公開或私有？fork PR 是否會觸發 workflow？（決定階段 3 能否進行。）
6. 預覽環境的 AI 呼叫用 mock 還是限額金鑰？由誰核可。
7. 預覽環境的存取範圍：僅 tailnet 內，或需要給教師／需求方試用（若需要，需另案評估公開暴露與個資風險）。
8. 成員參與是否自願、可隨時退出，以及機器資源上限的共識。

## 10. 範圍外

- 正式環境部署與 HA。
- 單一環境內後端多副本（需先處理第 2.2 節的程序記憶體狀態）。
- 自動擴縮容、集中式日誌或監控平台。
