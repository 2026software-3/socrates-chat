#!/bin/sh
# 用法：entrypoint.sh tts | adapter | cascade（由 docker-compose.yml 的 cascade profile 指定）
set -eu

case "${1:-}" in
  tts)
    exec /opt/kokoro/bin/python /app/kokoro_server.py
    ;;
  adapter)
    export ADAPTER_HOST=0.0.0.0
    export TTS_UPSTREAM="${TTS_UPSTREAM:-http://cascade-tts:8100/v1/audio/speech}"
    export WHISPER_UPSTREAM="${WHISPER_UPSTREAM:-http://cascade-whisper:8101/v1/audio/transcriptions}"
    exec python3 /app/tts_adapter.py
    ;;
  cascade)
    : "${CASCADE_LLM_TOKEN:?CASCADE_LLM_TOKEN 未設定}"
    python3 /app/patch_dialog.py
    exec sgl-omni serve --config /app/launch.yaml --host 0.0.0.0 --port 8110 --enable-realtime
    ;;
  *)
    echo "用法：entrypoint.sh tts|adapter|cascade" >&2
    exit 2
    ;;
esac
