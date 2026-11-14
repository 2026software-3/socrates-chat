#!/usr/bin/env bash
# 共用設定與函式（被 install.sh／start.sh／stop.sh／status.sh source）。相容 macOS 內建的 bash 3.2。

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# 所有狀態（虛擬環境、sglang 原始碼、執行目錄、記錄檔）都放在這裡，不進版本控制、不放進 repo。
CASCADE_MACOS_HOME="${CASCADE_MACOS_HOME:-$HOME/.cache/socrates-cascade-macos}"
CASCADE_VENV="${CASCADE_VENV:-$CASCADE_MACOS_HOME/venv-cascade}"   # sglang-omni／轉接層
VOICE_VENV="${VOICE_VENV:-$CASCADE_MACOS_HOME/venv-voice}"         # Whisper／Kokoro
RUN_DIR="$CASCADE_MACOS_HOME/run"

# 固定埠（與 docker compose 版相同）；全部只綁 127.0.0.1。
TTS_PORT=8100
WHISPER_PORT=8101
ADAPTER_PORT=8103
CASCADE_PORT=8110

# 啟動順序；停止時反向。
SERVICES="whisper tts adapter cascade"

log() { printf '[cascade-macos] %s\n' "$*"; }
die() { printf '[cascade-macos][error] %s\n' "$*" >&2; exit 1; }

require_apple_silicon() {
  [ "$(uname -s)" = "Darwin" ] || die "只支援 macOS（目前是 $(uname -s)）"
  [ "$(uname -m)" = "arm64" ] || die "需要 Apple Silicon（arm64），目前是 $(uname -m)"
}

pid_file() { printf '%s/%s.pid' "$RUN_DIR" "$1"; }
log_file() { printf '%s/%s.log' "$RUN_DIR" "$1"; }

is_alive() { [ -n "${1:-}" ] && kill -0 "$1" 2>/dev/null; }

service_pid() {
  local f; f="$(pid_file "$1")"
  [ -f "$f" ] && cat "$f" || true
}

service_url() {
  case "$1" in
    whisper) printf 'http://127.0.0.1:%s/openapi.json' "$WHISPER_PORT" ;;
    tts) printf 'http://127.0.0.1:%s/health' "$TTS_PORT" ;;
    adapter) printf 'http://127.0.0.1:%s/v1/models' "$ADAPTER_PORT" ;;
    cascade) printf 'http://127.0.0.1:%s/health' "$CASCADE_PORT" ;;
    *) die "未知的服務：$1" ;;
  esac
}

service_healthy() { curl -fsS -m 3 -o /dev/null "$(service_url "$1")" 2>/dev/null; }

# 結束行程與它的所有子行程（sgl-omni 會產生多個子行程）。
kill_tree() {
  local pid="$1" signal="${2:-TERM}" child
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    kill_tree "$child" "$signal"
  done
  kill "-$signal" "$pid" 2>/dev/null || true
}

stop_service() {
  local name="$1" pid
  pid="$(service_pid "$name")"
  if is_alive "$pid"; then
    kill_tree "$pid" TERM
    local i=0
    while is_alive "$pid" && [ "$i" -lt 20 ]; do sleep 0.5; i=$((i + 1)); done
    if is_alive "$pid"; then
      log "$name 沒有在 10 秒內結束，強制結束"
      kill_tree "$pid" KILL
    fi
  fi
  rm -f "$(pid_file "$name")"
}
