# cascade 本體與轉接層（Intel 部署用，不需要 NVIDIA）。
# NVIDIA 版映像（docker/cascade/Dockerfile）裡 cascade 與 adapter 本來就沒有 GPU 保留，在 CPU 上跑；
# 這裡用 Ubuntu 24.04（與原映像的 Python 3.12 相同）取代 nvidia/cuda 基底，套件版本與原映像一致。
# TTS 與 ASR 另有各自的映像（tts.Dockerfile、whisper.Dockerfile）。
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive PIP_NO_CACHE_DIR=1 PIP_BREAK_SYSTEM_PACKAGES=1 PYTHONUNBUFFERED=1
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-pip python3-venv python3-dev git ffmpeg sox libsox-dev libsndfile1 build-essential ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 版本固定在實測過的 0.2.0；opencc 供轉接層繁轉簡
RUN pip install sglang-omni==0.2.0 opencc-python-reimplemented

WORKDIR /app
COPY docker/cascade/tts_adapter.py docker/cascade/patch_dialog.py docker/cascade/launch.yaml docker/cascade/entrypoint.sh /app/
RUN chmod +x /app/entrypoint.sh
ENTRYPOINT ["/app/entrypoint.sh"]
