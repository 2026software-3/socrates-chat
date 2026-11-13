#!/usr/bin/env bash
# 在 macOS（Apple Silicon）上原生啟動語音 cascade：Whisper → Kokoro（MPS）→ 轉接層 → cascade。
# 必須先設 CASCADE_LLM_TOKEN（與後端的同一個值，不要寫進任何檔案）。見 README.md。
#
# 環境變數（皆可省略）：
#   APP_URL              後端位址，cascade 用它呼叫內部 LLM 端點（預設 http://127.0.0.1:3000）
#   KOKORO_DEVICE        mps（預設）｜cpu｜auto
#   WHISPER_MODEL        預設 small（與 docker-compose 相同）
#   CASCADE_MACOS_HOME   狀態目錄（預設 ~/.cache/socrates-cascade-macos）
#   START_TIMEOUT        每個服務等待就緒的秒數（預設 240；第一次要編譯 Metal／載入模型較久）
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

require_apple_silicon
[ -n "${CASCADE_LLM_TOKEN:-}" ] || die "請先設定 CASCADE_LLM_TOKEN（與後端的 CASCADE_LLM_TOKEN 相同；例如 export CASCADE_LLM_TOKEN=\$(openssl rand -hex 32)）"
for venv in "$CASCADE_VENV" "$VOICE_VENV"; do
  [ -x "$venv/bin/python" ] || die "找不到虛擬環境 ${venv}；先執行 scripts/cascade-macos/install.sh"
done

APP_URL="${APP_URL:-http://127.0.0.1:3000}"
START_TIMEOUT="${START_TIMEOUT:-240}"
KOKORO_DEVICE="${KOKORO_DEVICE:-mps}"
WHISPER_MODEL="${WHISPER_MODEL:-small}"

mkdir -p "$RUN_DIR"
chmod 700 "$RUN_DIR"

for s in $SERVICES; do
  if is_alive "$(service_pid "$s")"; then
    die "$s 已在執行；先執行 scripts/cascade-macos/stop.sh"
  fi
done
for port in $TTS_PORT $WHISPER_PORT $ADAPTER_PORT $CASCADE_PORT; do
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    die "埠 $port 已被占用（可能是其他行程或 docker compose 的 cascade profile）"
  fi
done

python3 "$REPO_ROOT/scripts/cascade-macos/render_launch.py" \
  "$REPO_ROOT/docker/cascade/launch.yaml" "$RUN_DIR/launch.yaml" \
  --app-url "$APP_URL" --adapter-url "http://127.0.0.1:$ADAPTER_PORT" --model-path "$RUN_DIR"

if ! curl -fsS -m 3 -o /dev/null "$APP_URL/health" 2>/dev/null; then
  log "警告：連不上後端 $APP_URL/health；cascade 會啟動，但學生說話時它呼叫 LLM 會失敗"
fi

launch() { # launch NAME 指令...（繼承環境；CASCADE_LLM_TOKEN 只給 cascade）
  local name="$1"; shift
  : > "$(log_file "$name")"
  nohup "$@" >> "$(log_file "$name")" 2>&1 &
  echo $! > "$(pid_file "$name")"
}

wait_healthy() {
  local name="$1" waited=0
  while ! service_healthy "$name"; do
    if ! is_alive "$(service_pid "$name")"; then
      log "$name 啟動後結束了；記錄檔最後幾行："
      tail -n 20 "$(log_file "$name")" >&2 || true
      "$(dirname "${BASH_SOURCE[0]}")/stop.sh" >/dev/null
      die "$name 啟動失敗（完整記錄：$(log_file "$name")）"
    fi
    if [ "$waited" -ge "$START_TIMEOUT" ]; then
      "$(dirname "${BASH_SOURCE[0]}")/stop.sh" >/dev/null
      die "$name 在 ${START_TIMEOUT} 秒內沒有就緒（完整記錄：$(log_file "$name")）"
    fi
    sleep 1
    waited=$((waited + 1))
  done
  log "$name 就緒（${waited}s）"
}

# DYLD_LIBRARY_PATH 若指到 Homebrew 的 ffmpeg，會與 PyAV 內建的函式庫衝突；cascade 不需要它。
unset DYLD_LIBRARY_PATH
export PYTORCH_ENABLE_MPS_FALLBACK=1
SAFE_ENV="env -u CASCADE_LLM_TOKEN"

log "啟動 whisper（${WHISPER_MODEL}，CPU int8）"
launch whisper $SAFE_ENV WHISPER_MODEL="$WHISPER_MODEL" WHISPER_DEVICE=cpu WHISPER_COMPUTE=int8 \
  WHISPER_HOST=127.0.0.1 WHISPER_PORT="$WHISPER_PORT" PYTHONPATH="$REPO_ROOT/scripts" \
  "$VOICE_VENV/bin/python" "$REPO_ROOT/scripts/whisper-server.py"
wait_healthy whisper

log "啟動 tts（Kokoro，${KOKORO_DEVICE}）"
launch tts $SAFE_ENV KOKORO_DEVICE="$KOKORO_DEVICE" KOKORO_HOST=127.0.0.1 \
  "$VOICE_VENV/bin/python" "$REPO_ROOT/docker/cascade/kokoro_server.py"
wait_healthy tts

log "啟動 adapter"
launch adapter $SAFE_ENV ADAPTER_HOST=127.0.0.1 \
  TTS_UPSTREAM="http://127.0.0.1:$TTS_PORT/v1/audio/speech" \
  WHISPER_UPSTREAM="http://127.0.0.1:$WHISPER_PORT/v1/audio/transcriptions" \
  "$CASCADE_VENV/bin/python" "$REPO_ROOT/docker/cascade/tts_adapter.py"
wait_healthy adapter

log "啟動 cascade（sglang-omni Streaming Cascade）"
CASCADE_LLM_TOKEN="$CASCADE_LLM_TOKEN" launch cascade \
  "$CASCADE_VENV/bin/sgl-omni" serve --config "$RUN_DIR/launch.yaml" \
  --host 127.0.0.1 --port "$CASCADE_PORT" --enable-realtime
wait_healthy cascade

log "全部就緒。後端的 CASCADE_REALTIME_URL 設成 ws://127.0.0.1:$CASCADE_PORT/v1/realtime（後端在 Docker 內時見 README）"
log "記錄檔：$RUN_DIR/*.log（不含對話內容）；停止：scripts/cascade-macos/stop.sh"
