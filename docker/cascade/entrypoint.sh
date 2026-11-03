#!/bin/sh
# 用法：entrypoint.sh tts | adapter | cascade（由 docker-compose.yml 的 cascade profile 指定）
set -eu

case "${1:-}" in
  tts)
    exec sgl-omni serve --model-path Qwen/Qwen3-TTS-12Hz-0.6B-Base --host 0.0.0.0 --port 8100 \
      --tts_engine.engine.max_running_requests 2 --tts_engine.engine.max_queued_requests 8 \
      --tts_engine.engine.mem_fraction_static "${CASCADE_TTS_MEM_FRACTION:-0.8}" --tts_engine.engine.cuda_graph_max_bs 2 \
      --vocoder.factory.incremental_codec_cuda_graph false --vocoder.factory.codec_state_slots 4 \
      --vocoder.factory.initial_cuda_graph false --vocoder.factory.followup_cuda_graph false
    ;;
  adapter)
    export ADAPTER_HOST=0.0.0.0
    export TTS_UPSTREAM="${TTS_UPSTREAM:-http://cascade-tts:8100/v1/audio/speech}"
    export WHISPER_UPSTREAM="${WHISPER_UPSTREAM:-http://cascade-whisper:8101/v1/audio/transcriptions}"
    exec python3 /app/tts_adapter.py
    ;;
  cascade)
    : "${CASCADE_LLM_TOKEN:?CASCADE_LLM_TOKEN 未設定}"
    : "${CASCADE_REFERENCE_TEXT:?CASCADE_REFERENCE_TEXT 未設定（要和 /refs/reference.wav 的逐字稿一致）}"
    [ -f /refs/reference.wav ] || { echo "找不到 /refs/reference.wav：請把參考音掛到 /refs" >&2; exit 1; }
    python3 - <<'PY'
import os
text = os.environ["CASCADE_REFERENCE_TEXT"].replace("\\", "\\\\").replace('"', '\\"')
src = open("/app/launch.yaml.tpl", encoding="utf-8").read()
open("/tmp/launch.yaml", "w", encoding="utf-8").write(src.replace("${CASCADE_REFERENCE_TEXT}", text))
PY
    exec sgl-omni serve --config /tmp/launch.yaml --host 0.0.0.0 --port 8110 --enable-realtime
    ;;
  *)
    echo "用法：entrypoint.sh tts|adapter|cascade" >&2
    exit 2
    ;;
esac
