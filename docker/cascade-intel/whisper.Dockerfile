# cascade 的 ASR（Intel NPU 版）：OpenVINO GenAI 的 Whisper small int8，由 whisper_ov_server.py 提供。
# 適用 Intel Core Ultra（Panther Lake）的 NPU；容器要掛 /dev/accel，使用者需在 render 群組。
# 基底與主機同為 Ubuntu 26.04，使用者空間驅動（Level Zero／NPU）版本才會和主機核心驅動相容。
# 模型權重不放進映像：HF 快取以 volume 掛入。
FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive PYTHONUNBUFFERED=1 PIP_NO_CACHE_DIR=1 UV_PYTHON_INSTALL_DIR=/opt/python UV_LINK_MODE=copy
# NPU 的使用者空間驅動不在 Ubuntu 倉庫：要先把 Intel linux-npu-driver 發行檔裡的三個 .deb 放進 docker/cascade-intel/npu-debs/
# （intel-level-zero-npu、intel-driver-compiler-npu，以及 Level Zero 載入器 libze1；版本與主機一致，msi_cubi 在 ~/npu_driver）。
# 這些 .deb 不進版控（.gitignore）。韌體 intel-fw-npu 是核心用的，只需裝在主機。
COPY docker/cascade-intel/npu-debs/*.deb /tmp/npu-debs/
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3-pip libze-intel-gpu1 intel-opencl-icd /tmp/npu-debs/*.deb \
    && rm -rf /var/lib/apt/lists/* /tmp/npu-debs

# 系統 Python 是 3.14；與 TTS 映像一致，用 uv 取得 3.12 建虛擬環境。
RUN pip install --break-system-packages uv && uv venv --python 3.12 /opt/venv
ENV VIRTUAL_ENV=/opt/venv PATH=/opt/venv/bin:$PATH
RUN uv pip install --no-cache \
    openvino==2026.4.1 openvino-genai==2026.4.1.0 huggingface-hub==2.4.0 \
    fastapi==0.163.1 uvicorn==0.54.0 python-multipart==0.0.33 httpx==0.28.1 \
    "av>=14,<20" numpy opencc-python-reimplemented==0.1.7 \
    faster-whisper==1.2.1

WORKDIR /app
COPY scripts/whisper_text.py scripts/whisper_echo.py docker/cascade-intel/whisper_ov_server.py /app/
CMD ["python", "whisper_ov_server.py"]
