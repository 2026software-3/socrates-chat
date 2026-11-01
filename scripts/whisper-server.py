#!/usr/bin/env python3
"""自架的 Whisper 語音辨識服務，提供 OpenAI 相容的 POST /v1/audio/transcriptions。

後端設 OPENAI_STT_BASE_URL=http://127.0.0.1:8000/v1、OPENAI_STT_MODEL=whisper-base 即可使用。
音訊只在記憶體中處理，不寫入磁碟；記錄只含時間與 GPU 記憶體，不含辨識文字。
中文一律輸出臺灣繁體：模型對同一段音訊有時給繁體、有時給簡體，見 whisper_text.py。

依賴：pip install faster-whisper fastapi uvicorn python-multipart nvidia-ml-py "av>=14,<16" opencc-python-reimplemented
GPU 需要 cuBLAS／cuDNN 9（pip 的 nvidia-cublas-cu12、nvidia-cudnn-cu12 即可，路徑會自動加入）。

環境變數：WHISPER_MODEL（預設 base）、WHISPER_DEVICE（cuda／cpu，預設 cuda）、
WHISPER_COMPUTE（預設 float16；CPU 用 int8）、WHISPER_PERF_LOG（每個請求一行 JSON，預設不寫）、
WHISPER_INITIAL_PROMPT（所有請求共用的提示詞，預設空，即不帶）。
請求帶 OpenAI 的 `prompt` 欄位時用請求的，沒有才用 WHISPER_INITIAL_PROMPT。
預設不帶提示詞：Common Voice 實測它對一般句子沒有穩定的好處，還會讓 2–3 秒的短句變成空字串；
只在活動題目有術語、確實需要時才由請求帶入（見 docs/specs/whisper-base-stt-perf.md）。
輸出幾乎全是提示詞內的詞時（模型把提示詞當辨識結果吐出來），回空字串，見 whisper_echo.py。
WHISPER_VAD（預設 1）：先用 Silero VAD 濾掉無語音的片段。帶提示詞時一定要開：實測 small 遇到靜音或雜訊會把提示詞整段當成辨識結果吐出來。
"""
import io
import json
import os
import sys
import time

if "_WHISPER_REEXEC" not in os.environ:
    try:
        import nvidia

        root = list(nvidia.__path__)[0]
        libs = [f"{root}/{d}/lib" for d in ("cublas", "cudnn")]
        os.environ["LD_LIBRARY_PATH"] = ":".join(libs + [os.environ.get("LD_LIBRARY_PATH", "")])
    except ImportError:
        pass
    os.environ["_WHISPER_REEXEC"] = "1"
    os.execv(sys.executable, [sys.executable, *sys.argv])

import uvicorn
from fastapi import FastAPI, File, Form, UploadFile
from faster_whisper import WhisperModel
from whisper_echo import is_prompt_echo, pick_prompt
from whisper_text import normalize

MODEL = os.environ.get("WHISPER_MODEL", "base")
DEVICE = os.environ.get("WHISPER_DEVICE", "cuda")
COMPUTE = os.environ.get("WHISPER_COMPUTE", "float16" if DEVICE == "cuda" else "int8")
PERF_LOG = os.environ.get("WHISPER_PERF_LOG")
# 提示詞只影響用詞偏好，不是辨識結果；放術語可減少同音字錯誤，但預設不帶（理由見檔頭）
INITIAL_PROMPT = os.environ.get("WHISPER_INITIAL_PROMPT", "")
VAD = os.environ.get("WHISPER_VAD", "1") != "0"

try:
    import pynvml

    pynvml.nvmlInit()
    _gpu = pynvml.nvmlDeviceGetHandleByIndex(0)

    def gpu_mb():
        return round(pynvml.nvmlDeviceGetMemoryInfo(_gpu).used / 2**20)
except Exception:  # 沒有 GPU 或沒裝 pynvml
    def gpu_mb():
        return None


model = WhisperModel(MODEL, device=DEVICE, compute_type=COMPUTE)
app = FastAPI()


@app.post("/v1/audio/transcriptions")
def transcribe(file: UploadFile = File(...), model_name: str = Form("", alias="model"),
               language: str = Form(""), prompt: str = Form("")):
    # 同步函式由 FastAPI 放進 threadpool；CTranslate2 的 transcribe 可多執行緒並行
    started = time.time()
    audio = file.file.read()
    used_prompt = pick_prompt(prompt, INITIAL_PROMPT)
    segments, info = model.transcribe(io.BytesIO(audio), language=language or None, beam_size=5,
                                    initial_prompt=used_prompt, vad_filter=VAD)
    text = normalize("".join(s.text for s in segments).strip(), info.language)
    echoed = is_prompt_echo(text, used_prompt)
    if echoed:
        text = ""
    elapsed = time.time() - started
    if PERF_LOG:
        with open(PERF_LOG, "a") as f:
            f.write(json.dumps({
                "ts": round(started), "model": MODEL, "device": DEVICE, "compute": COMPUTE,
                "audio_s": round(info.duration, 2), "elapsed_s": round(elapsed, 3),
                "rtf": round(elapsed / info.duration, 4) if info.duration else None,
                "upload_bytes": len(audio), "prompt_echo_dropped": echoed, "gpu_used_mb": gpu_mb(),
            }) + "\n")
    return {"text": text}


if __name__ == "__main__":
    uvicorn.run(app, host=os.environ.get("WHISPER_HOST", "127.0.0.1"),
                port=int(os.environ.get("WHISPER_PORT", "8000")))
