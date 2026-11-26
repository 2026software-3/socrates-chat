# cascade 量測工具

量 cascade 的記憶體、首段音訊延遲、斷句與打斷，用來比較設定變更前後（結果見 [`docs/specs/cascade-slimming.md`](../../docs/specs/cascade-slimming.md)）。
假 LLM 取代後端的內部端點，把 OpenAI 的延遲隔開，只量 cascade 這一段。測試資料全是合成語音，不含學生資料。

## 檔案

| 檔案 | 作用 |
| --- | --- |
| `stub_llm.py` | 假的 `/internal/cascade/v1`：回六句固定的串流文字，密鑰不對回 401，`/hits` 回收到的呼叫次數。環境變數 `CASCADE_LLM_TOKEN`、`STUB_TAG` |
| `scn.py` | 客戶端。用 adapter 的 TTS 合成「學生的聲音」，經 cascade 的 realtime WebSocket 跑情境，每輪新開一個場次，只印時間與計數（`RES {...}`） |
| `summarize.py` | 彙整 `scn.py` 的輸出 |

## 情境（`SCEN`）

- `turn`：一句話，量辨識與首段音訊。
- `pause`：一句話中間停頓 `PAUSE` 秒（預設 0.8），看會不會被切成兩句。目前門檻在 1.0 到 1.2 秒之間：0.8 秒不切，1.2 秒以上切成兩段。
- `interrupt`：AI 回覆途中學生插話，量偵測與取消延遲。

其他環境變數：`ROUNDS`（輪數）、`OFFSET`（開始前等幾秒，用來錯開兩個客戶端）、`CASCADE_URL`、`ADAPTER_URL`、`TAG`。

## 用法

在 cascade 所在的 compose 網路內跑（容器名稱依你的專案調整）。先起假 LLM 和一個測試用 cascade，把回呼指向假 LLM：

```bash
NET=<compose 網路>; IMG=socrates-cascade:latest
docker run -d --name bench-stub --network $NET -e CASCADE_LLM_TOKEN=bench --entrypoint python3 $IMG -c "$(cat scripts/cascade-bench/stub_llm.py)"
docker run -d --name bench-cascade --network $NET \
  -e CASCADE_LLM_TOKEN=bench -e CASCADE_LLM_BASE_URL=http://bench-stub:3000/internal/cascade/v1 \
  -v <cascade_cache>:/root/.cache -v <cascade_hf>:/root/.cache/huggingface $IMG cascade
```

再跑情境（腳本從標準輸入餵進去，容器內已有 httpx、numpy、websockets）：

```bash
docker run --rm -i --network $NET -e SCEN=turn -e ROUNDS=5 -e TAG=base \
  -e CASCADE_URL=ws://bench-cascade:8110/v1/realtime \
  -e ADAPTER_URL=http://cascade-adapter:8103/v1/audio/speech \
  --entrypoint python3 $IMG - < scripts/cascade-bench/scn.py | grep ^RES > turn.txt
python3 scripts/cascade-bench/summarize.py base=turn.txt
```

記憶體用 `docker stats --no-stream bench-cascade`，各程序的匿名記憶體讀容器內 `/proc/*/status` 的 `RssAnon`。

## 注意

- 比較兩個設定要**成對交錯**：兩個 cascade 同時待命，輪流各跑一段，再比 `summarize.py` 的間隔。分開時段跑的單次數字容易被當時的 TTS／Whisper 狀態影響（實測遇過看似慢 0.4 秒、成對測就消失的假象）。
- 測試語音是合成的，停頓與音量比真人規則，斷句的絕對門檻不能直接套到真人，只適合比較前後差異。
- 測試會對共用的 TTS 與 Whisper 加負載。在正在服務使用者的機器上跑時，先確認沒有場次。
