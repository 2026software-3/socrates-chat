# cascade 的 TTS（Intel iGPU 版）：Kokoro-82M 跑在 PyTorch XPU，服務程式與 NVIDIA 版共用 docker/cascade/kokoro_server.py，
# 以 KOKORO_DEVICE=xpu 選裝置。容器要掛 /dev/dri，使用者需在 render 群組。
# 基底與主機同為 Ubuntu 26.04（Level Zero GPU 驅動版本要和主機核心驅動相容）。映像約 8 GB，主因是 XPU 版 torch。
# 模型權重不放進映像：HF 快取以 volume 掛入。
FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive PYTHONUNBUFFERED=1 PIP_NO_CACHE_DIR=1 UV_PYTHON_INSTALL_DIR=/opt/python UV_LINK_MODE=copy \
    KOKORO_DEVICE=xpu SYCL_CACHE_PERSISTENT=1
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3-pip libze1 libze-intel-gpu1 intel-opencl-icd libsndfile1 \
    && rm -rf /var/lib/apt/lists/*

# kokoro 0.9.4 要求 Python <3.13，系統是 3.14，所以用 uv 取得 3.12。
RUN pip install --break-system-packages uv && uv venv --python 3.12 /opt/venv
ENV VIRTUAL_ENV=/opt/venv PATH=/opt/venv/bin:$PATH
RUN uv pip install --no-cache torch==2.15.0 --index-url https://download.pytorch.org/whl/xpu
RUN uv pip install --no-cache 'kokoro==0.9.4' 'misaki[zh]==0.9.4' fastapi==0.163.1 uvicorn==0.54.0 pydantic==2.14.3

WORKDIR /app
COPY docker/cascade/kokoro_server.py /app/
CMD ["python", "/app/kokoro_server.py"]
