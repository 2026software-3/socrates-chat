# cascade 的 ASR：CPU 版 Whisper（scripts/whisper-server.py）。模型下載後存在 HF 快取 volume。
FROM python:3.12-slim

ENV PIP_NO_CACHE_DIR=1 PYTHONUNBUFFERED=1
RUN pip install faster-whisper fastapi uvicorn python-multipart nvidia-ml-py "av>=14,<16" opencc-python-reimplemented

WORKDIR /app
COPY scripts/whisper-server.py scripts/whisper_text.py scripts/whisper_echo.py /app/
CMD ["python", "whisper-server.py"]
