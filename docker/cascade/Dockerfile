# 伺服器端語音對話（cascade）用的映像：TTS 與 cascade 本體共用，轉接層也用它執行。
# 只在 docker compose 的 cascade profile 啟用時才會建置，見 docs/specs/cascade-voice-spike.md。
# 模型權重不放進映像：HF 快取以 volume 掛入。
FROM nvidia/cuda:13.0.2-devel-ubuntu24.04

ENV DEBIAN_FRONTEND=noninteractive PIP_NO_CACHE_DIR=1 PIP_BREAK_SYSTEM_PACKAGES=1 PYTHONUNBUFFERED=1
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-pip python3-dev git ffmpeg sox libsox-dev libsndfile1 build-essential ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 版本固定在實測過的 0.2.0；opencc 供轉接層繁轉簡
RUN pip install sglang-omni==0.2.0 opencc-python-reimplemented
RUN apt-get update && apt-get install -y --no-install-recommends python3-venv \
    && rm -rf /var/lib/apt/lists/*
# Kokoro 的依賴放進獨立虛擬環境，避免更改 cascade 的套件版本；重用映像已有的 CUDA PyTorch。
RUN python3 -m venv --system-site-packages /opt/kokoro \
    && /opt/kokoro/bin/pip install 'kokoro==0.9.4' 'misaki[zh]==0.9.4'

WORKDIR /app
COPY docker/cascade/kokoro_server.py docker/cascade/tts_adapter.py docker/cascade/patch_dialog.py docker/cascade/launch.yaml.tpl docker/cascade/entrypoint.sh /app/
RUN chmod +x /app/entrypoint.sh
ENTRYPOINT ["/app/entrypoint.sh"]
