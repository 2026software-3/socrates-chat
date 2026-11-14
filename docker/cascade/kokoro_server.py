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


class SpeechRequest(BaseModel):
    input: str = Field(min_length=1)
    response_format: str = 'pcm'


def render_pcm(synthesize, text, cancelled):
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
    return (np.clip(samples, -1.0, 1.0 - 1.0 / 32768) * 32768).astype('<i2').tobytes()


def create_app(synthesize):
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
                return await run_in_threadpool(render_pcm, synthesize, request.input, cancelled)

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


def main():
    from kokoro import KPipeline

    pipeline = KPipeline(lang_code='z', device='cuda')
    voice = os.environ.get('KOKORO_VOICE', 'zm_yunjian')
    pipeline.load_voice(voice)
    synthesize = make_synthesizer(pipeline, voice)
    # 預熱：第一次推論會做延遲載入與 CUDA 核心初始化，先在啟動時付掉，/health 才代表真的可用。
    if not list(synthesize(WARMUP_TEXT)):
        raise RuntimeError('Kokoro warm-up produced no audio')

    uvicorn.run(create_app(synthesize), host='0.0.0.0', port=8100, access_log=False)


if __name__ == '__main__':
    main()
