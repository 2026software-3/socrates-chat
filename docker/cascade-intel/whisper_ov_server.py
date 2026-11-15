#!/usr/bin/env python3
"""cascade 的 ASR（Intel NPU／iGPU 版）：OpenVINO GenAI 的 Whisper，提供與 scripts/whisper-server.py 相同的
POST /v1/audio/transcriptions（只回 {"text": ...}），轉接層與 cascade 不用改。

和 faster-whisper 版的差別（要知道的風險）：
- openvino-genai 不回每段的 no_speech_prob／avg_logprob，所以沒有 keep_segment 的信心過濾。
  補償：先過一道能量／長度門檻（太短或太小聲直接回空字串），再用與 faster-whisper 版相同的 Silero VAD
  （faster-whisper 內附的 ONNX 模型，CPU 跑）只留語音部分；沒有語音就不呼叫模型。
  其餘文字整理（繁體化、去片尾套語、提示詞回聲）沿用 whisper_text.py／whisper_echo.py。
- NPU 的管線只有一條串流，請求以鎖排隊。
- 音訊只在記憶體中處理；記錄只含時間與長度，不含辨識文字。

環境變數：WHISPER_OV_REPO（預設 OpenVINO/whisper-small-int8-ov）、WHISPER_OV_DEVICE（NPU／GPU／CPU，預設 NPU）、
WHISPER_HOST、WHISPER_PORT、WHISPER_PERF_LOG。
"""
import io
import json
import os
import threading
import time

import numpy as np
import uvicorn
from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from whisper_echo import is_prompt_echo, pick_prompt
from whisper_text import normalize, strip_credits

SAMPLE_RATE = 16000
# 短於這個長度（秒）多半是爆音或誤觸；低於這個 RMS 約等於 -50 dBFS，是環境底噪等級。
MIN_SECONDS = 0.3
MIN_RMS = 0.003
PERF_LOG = os.environ.get('WHISPER_PERF_LOG')


def decode_audio(data: bytes) -> np.ndarray:
    """把上傳的音訊（wav 等）解成 16 kHz 單聲道 float32。無法解碼時丟 ValueError。"""
    import av

    try:
        with av.open(io.BytesIO(data)) as container:
            stream = next((s for s in container.streams if s.type == 'audio'), None)
            if stream is None:
                raise ValueError('no audio stream')
            resampler = av.AudioResampler(format='flt', layout='mono', rate=SAMPLE_RATE)
            parts = []
            for frame in container.decode(stream):
                for out in resampler.resample(frame):
                    parts.append(out.to_ndarray().reshape(-1))
            for out in resampler.resample(None):
                parts.append(out.to_ndarray().reshape(-1))
    except av.error.FFmpegError as exc:
        raise ValueError('undecodable audio') from exc
    if not parts:
        return np.zeros(0, dtype=np.float32)
    return np.concatenate(parts).astype(np.float32)


def is_speech(samples: np.ndarray) -> bool:
    if len(samples) < MIN_SECONDS * SAMPLE_RATE:
        return False
    return float(np.sqrt(np.mean(np.square(samples, dtype=np.float64)))) >= MIN_RMS


def speech_only(samples: np.ndarray) -> np.ndarray:
    """只留下 Silero VAD 判定為語音的部分（接起來）；沒有語音回空陣列。

    與 faster-whisper 版的 vad_filter=True 同一套模型與預設參數（faster-whisper 內附的 ONNX，CPU 跑）。
    沒有它時，高於能量門檻的噪音（底噪、嗡聲、鍵盤聲）會被辨識成 1–4 個字的幻聽（2026-11-16 實測）。
    """
    from faster_whisper.vad import VadOptions, collect_chunks, get_speech_timestamps

    chunks = get_speech_timestamps(samples, VadOptions(), sampling_rate=SAMPLE_RATE)
    if not chunks:
        return samples[:0]
    parts, _ = collect_chunks(samples, chunks, sampling_rate=SAMPLE_RATE)
    return np.concatenate(parts).astype(np.float32) if parts else samples[:0]


def create_app(transcribe, vad=None) -> FastAPI:
    """transcribe(samples, language|None) -> str，由呼叫端注入（測試用假的，正式用 NPU 管線）。
    vad(samples) -> samples：只留語音部分；預設不過濾（正式啟動時傳入 speech_only）。"""
    app = FastAPI()
    lock = threading.Lock()

    @app.post('/v1/audio/transcriptions')
    def transcriptions(file: UploadFile = File(...), language: str = Form(''), prompt: str = Form('')):
        # 同步函式由 FastAPI 放進 threadpool；鎖讓同一時間只有一個請求用 NPU。
        started = time.time()
        try:
            samples = decode_audio(file.file.read())
        except ValueError:
            raise HTTPException(status_code=400, detail='invalid audio') from None
        text = ''
        try:
            speech = vad(samples) if vad and is_speech(samples) else samples
        except Exception:
            raise HTTPException(status_code=500, detail='transcription failed') from None
        if is_speech(speech):
            try:
                with lock:
                    raw = transcribe(speech, language or None)
            except Exception:
                raise HTTPException(status_code=500, detail='transcription failed') from None
            text = strip_credits(normalize(raw.strip(), language or 'zh'))
            if is_prompt_echo(text, pick_prompt(prompt, None)):
                text = ''
        if PERF_LOG:
            audio_s = len(samples) / SAMPLE_RATE
            elapsed = time.time() - started
            with open(PERF_LOG, 'a') as f:
                f.write(json.dumps({'ts': round(started), 'audio_s': round(audio_s, 2), 'elapsed_s': round(elapsed, 3),
                                    'rtf': round(elapsed / audio_s, 4) if audio_s else None, 'empty': not text}) + '\n')
        return {'text': text}

    return app


def make_transcriber(pipeline):
    config = pipeline.get_generation_config()
    config.task = 'transcribe'

    def transcribe(samples, language):
        # language 為 None 時交給模型自動偵測
        config.language = f'<|{language}|>' if language else None
        return str(pipeline.generate(samples, config=config).texts[0])

    return transcribe


def main():
    import openvino_genai
    from huggingface_hub import snapshot_download

    repo = os.environ.get('WHISPER_OV_REPO', 'OpenVINO/whisper-small-int8-ov')
    device = os.environ.get('WHISPER_OV_DEVICE', 'NPU')
    pipeline = openvino_genai.WhisperPipeline(snapshot_download(repo), device)
    transcribe = make_transcriber(pipeline)
    # 預熱：第一次推論才會真正編譯／配置，先在啟動時付掉，/openapi.json（healthcheck）才代表真的可用。
    transcribe(np.zeros(SAMPLE_RATE, dtype=np.float32), 'zh')

    speech_only(np.zeros(SAMPLE_RATE, dtype=np.float32))  # 預熱 VAD 模型載入
    uvicorn.run(create_app(transcribe, vad=speech_only), host=os.environ.get('WHISPER_HOST', '0.0.0.0'),
                port=int(os.environ.get('WHISPER_PORT', '8101')), access_log=False)


if __name__ == '__main__':
    main()
