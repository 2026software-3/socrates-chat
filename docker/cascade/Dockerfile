# 伺服器端語音對話（cascade）用的映像：TTS 與 cascade 本體共用，轉接層也用它執行。
# 只在 docker compose 的 cascade profile 啟用時才會建置，見 docs/specs/cascade-voice-spike.md。
# 模型權重與參考音不放進映像：HF 快取與參考音以 volume 掛入。
FROM nvidia/cuda:13.0.2-devel-ubuntu24.04

ENV DEBIAN_FRONTEND=noninteractive PIP_NO_CACHE_DIR=1 PIP_BREAK_SYSTEM_PACKAGES=1 PYTHONUNBUFFERED=1
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-pip python3-dev git ffmpeg sox libsox-dev libsndfile1 build-essential ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 版本固定在實測過的 0.2.0；opencc 供轉接層繁轉簡
RUN pip install sglang-omni==0.2.0 opencc-python-reimplemented
# Qwen3-TTS 需要官方 qwen-tts；它釘 Transformers 4.57.3、sox 會把 numpy 升過 numba 的上限，所以兩行都必須 --no-deps
RUN pip install --no-deps sox einops && pip install --no-deps qwen-tts==0.1.1

WORKDIR /app
COPY docker/cascade/tts_adapter.py docker/cascade/launch.yaml.tpl docker/cascade/entrypoint.sh /app/
RUN chmod +x /app/entrypoint.sh
ENTRYPOINT ["/app/entrypoint.sh"]
