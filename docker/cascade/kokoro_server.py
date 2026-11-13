"""Kokoro speech endpoint for the cascade's 24 kHz PCM contract."""
import asyncio
import os
import re
import threading

import numpy as np
import uvicorn
from fastapi import FastAPI, HTTPException, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import Response
from pydantic import BaseModel, Field

SAMPLE_RATE = 24000
# 沒有可唸內容（只有標點、符號或表情）時回傳的靜音長度；cascade 的 TTS 或 ASR 回 500 會關閉整場會話，
# 所以「無聲」必須是成功回應，而不是錯誤。
SILENCE_SECONDS = 0.05
SILENCE = bytes(int(SAMPLE_RATE * SILENCE_SECONDS) * 2)
# 文字中至少要有一個字母、數字或漢字，才值得交給模型。
SPEAKABLE = re.compile(r'\w')
WARMUP_TEXT = '你好，歡迎使用語音對話。'
# 振幅低於這個值視為靜音（約 -46 dBFS）；只用來找每句開頭的靜音。
SILENCE_LEVEL = 0.005


class SpeechRequest(BaseModel):
    input: str = Field(min_length=1)
    response_format: str = 'pcm'


def parse_lead_ms(value):
    """KOKORO_LEAD_SILENCE_MS：每句開頭最多保留幾毫秒靜音。沒設定（預設）就不動音訊。"""
    if value is None or not value.strip():
        return None
    if not value.strip().isdigit():
        raise ValueError(f'KOKORO_LEAD_SILENCE_MS must be a non-negative integer; got {value!r}')
    return int(value)


def trim_leading_silence(samples, keep_ms):
    """開頭的靜音只留 keep_ms 毫秒。Kokoro 每句開頭約有 0.4 秒靜音，學生要多等這段才聽到第一個字；
    句尾的靜音（句與句之間的停頓）不動。整段都是靜音時原樣回傳。"""
    voiced = np.flatnonzero(np.abs(samples) > SILENCE_LEVEL)
    if voiced.size == 0:
        return samples
    start = int(voiced[0]) - SAMPLE_RATE * keep_ms // 1000
    return samples[start:] if start > 0 else samples


def render_pcm(synthesize, text, cancelled, lead_ms=None):
    """合成成 16-bit 小端 PCM。沒有可唸內容回傳靜音；被取消回傳 None。"""
    if not SPEAKABLE.search(text):
        return SILENCE
    chunks = []
    for chunk in synthesize(text):
        if cancelled.is_set():
            return None
        chunks.append(np.asarray(chunk, dtype=np.float32))
    if cancelled.is_set():
        return None
    samples = np.concatenate(chunks) if chunks else np.zeros(0, dtype=np.float32)
    if samples.size == 0:
        return SILENCE
    if lead_ms is not None:
        samples = trim_leading_silence(samples, lead_ms)
    return (np.clip(samples, -1.0, 1.0 - 1.0 / 32768) * 32768).astype('<i2').tobytes()


def create_app(synthesize, lead_ms=None):
    app = FastAPI()
    # 用 asyncio 鎖排隊，等待中的請求不佔用執行緒池，/health 也不會被它們擠掉。
    lock = asyncio.Lock()

    @app.get('/health')
    async def health():
        return {'status': 'ok'}

    @app.post('/v1/audio/speech')
    async def speech(request: SpeechRequest, http_request: Request):
        if request.response_format != 'pcm' or not request.input.strip():
            raise HTTPException(status_code=400, detail='invalid speech request')

        cancelled = threading.Event()

        async def serialized():
            async with lock:
                if cancelled.is_set():
                    return None
                return await run_in_threadpool(render_pcm, synthesize, request.input, cancelled, lead_ms)

        work = asyncio.ensure_future(serialized())
        # 取消後沒人會 await 它，先接住結果，避免「從未取得例外」的警告。
        work.add_done_callback(lambda task: task.cancelled() or task.exception())
        while True:
            done, _ = await asyncio.wait({work}, timeout=0.1)
            if done:
                break
            if await http_request.is_disconnected():
                # 打斷時 cascade 會斷線：排隊中的直接放棄，合成中的在下一段之間停止。
                cancelled.set()
                work.cancel()
                return Response(status_code=499)
        try:
            audio = work.result()
        except Exception:
            raise HTTPException(status_code=500, detail='speech synthesis failed') from None
        if audio is None:
            return Response(status_code=499)
        return Response(audio, media_type='application/octet-stream', headers={'X-Sample-Rate': str(SAMPLE_RATE)})

    return app


def make_synthesizer(pipeline, voice):
    def synthesize(text):
        for _, _, audio in pipeline(text, voice=voice):
            yield audio.numpy()

    return synthesize


DEVICES = ('cuda', 'mps', 'cpu')


def resolve_device(requested, cuda, mps):
    """決定推論裝置。預設 cuda（NVIDIA 部署不變）；auto 依序挑 cuda、mps、cpu。
    指定的裝置不存在時直接失敗：悄悄退回 CPU 會讓延遲變差卻沒有人發現。"""
    name = (requested or 'cuda').strip().lower()
    if name == 'auto':
        return 'cuda' if cuda else 'mps' if mps else 'cpu'
    if name not in DEVICES:
        raise ValueError(f'KOKORO_DEVICE must be one of auto, {", ".join(DEVICES)}; got {name!r}')
    if name == 'cuda' and not cuda:
        raise RuntimeError('KOKORO_DEVICE=cuda but CUDA is not available')
    if name == 'mps' and not mps:
        raise RuntimeError('KOKORO_DEVICE=mps but the PyTorch MPS backend is not available')
    return name


BACKENDS = ('torch', 'mlx')
MLX_MODEL = 'mlx-community/Kokoro-82M-bf16'


def resolve_backend(requested):
    """決定推論框架。預設 torch（NVIDIA 部署不變）；mlx 給 Apple Silicon（scripts/cascade-macos）。"""
    name = (requested or 'torch').strip().lower()
    if name not in BACKENDS:
        raise ValueError(f'KOKORO_BACKEND must be one of {", ".join(BACKENDS)}; got {name!r}')
    return name


def make_mlx_synthesizer(model, voice):
    """mlx-audio 的 Kokoro（同一個模型與聲線）。PyTorch MPS 每遇到沒算過的句長就要重新編譯計算圖，
    實際對話的句子幾乎都是新的；MLX 沒有這個成本，量測見 scripts/cascade-macos/README.md。"""
    def synthesize(text):
        for result in model.generate(text, voice=voice, lang_code='z', speed=1.0):
            yield np.asarray(result.audio, dtype=np.float32).reshape(-1)

    return synthesize


def load_torch_synthesizer(voice):
    import torch
    from kokoro import KPipeline

    device = resolve_device(os.environ.get('KOKORO_DEVICE'), torch.cuda.is_available(), torch.backends.mps.is_available())
    pipeline = KPipeline(lang_code='z', device=device)
    pipeline.load_voice(voice)
    return make_synthesizer(pipeline, voice)


def load_mlx_synthesizer(voice):
    from mlx_audio.tts.utils import load_model

    return make_mlx_synthesizer(load_model(os.environ.get('KOKORO_MLX_MODEL', MLX_MODEL)), voice)


def main():
    voice = os.environ.get('KOKORO_VOICE', 'zm_yunjian')
    backend = resolve_backend(os.environ.get('KOKORO_BACKEND'))
    lead_ms = parse_lead_ms(os.environ.get('KOKORO_LEAD_SILENCE_MS'))
    synthesize = load_mlx_synthesizer(voice) if backend == 'mlx' else load_torch_synthesizer(voice)
    # 預熱：第一次推論會做延遲載入與 CUDA 核心初始化，先在啟動時付掉，/health 才代表真的可用。
    if not list(synthesize(WARMUP_TEXT)):
        raise RuntimeError('Kokoro warm-up produced no audio')

    # 容器內要綁 0.0.0.0 讓 compose 網路連得到；macOS 原生執行時由啟動腳本設成 127.0.0.1。
    uvicorn.run(create_app(synthesize, lead_ms), host=os.environ.get('KOKORO_HOST', '0.0.0.0'), port=8100, access_log=False)


if __name__ == '__main__':
    main()
