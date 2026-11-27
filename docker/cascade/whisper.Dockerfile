# cascade 的 ASR：Whisper（scripts/whisper-server.py）。預設是 CPU 版；NVIDIA GPU 版用 build arg WHISPER_GPU=1
# （加裝 cuBLAS、cuDNN 9），搭配 docker-compose.whisper-gpu.yml。模型下載後存在 HF 快取 volume。
FROM python:3.12-slim

ARG WHISPER_GPU=0
ENV PIP_NO_CACHE_DIR=1 PYTHONUNBUFFERED=1
RUN pip install faster-whisper fastapi uvicorn python-multipart nvidia-ml-py "av>=14,<16" opencc-python-reimplemented
RUN if [ "$WHISPER_GPU" = "1" ]; then pip install nvidia-cublas-cu12 nvidia-cudnn-cu12; fi

WORKDIR /app
COPY scripts/whisper-server.py scripts/whisper_text.py scripts/whisper_echo.py /app/
CMD ["python", "whisper-server.py"]
