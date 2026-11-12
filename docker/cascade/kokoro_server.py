"""Kokoro speech endpoint for the cascade's 24 kHz PCM contract."""
import os
import threading

import numpy as np
import uvicorn
from fastapi import FastAPI, HTTPException
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import Response
from pydantic import BaseModel, Field


class SpeechRequest(BaseModel):
    input: str = Field(min_length=1)
    response_format: str = 'pcm'


def create_app(synthesize):
    app = FastAPI()
    lock = threading.Lock()

    @app.get('/health')
    def health():
        return {'status': 'ok'}

    @app.post('/v1/audio/speech')
    async def speech(request: SpeechRequest):
        if request.response_format != 'pcm' or not request.input.strip():
            raise HTTPException(status_code=400, detail='invalid speech request')

        def render():
            with lock:
                chunks = [np.asarray(chunk, dtype=np.float32) for chunk in synthesize(request.input)]
                if not chunks:
                    raise ValueError('no audio produced')
                samples = np.concatenate(chunks)
                return (np.clip(samples, -1.0, 1.0 - 1.0 / 32768) * 32768).astype('<i2').tobytes()

        try:
            audio = await run_in_threadpool(render)
        except Exception:
            raise HTTPException(status_code=500, detail='speech synthesis failed') from None
        return Response(audio, media_type='application/octet-stream', headers={'X-Sample-Rate': '24000'})

    return app


def main():
    from kokoro import KPipeline

    pipeline = KPipeline(lang_code='z', device='cuda')
    voice = os.environ.get('KOKORO_VOICE', 'zm_yunjian')
    pipeline.load_voice(voice)

    def synthesize(text):
        for _, _, audio in pipeline(text, voice=voice):
            yield audio.numpy()

    uvicorn.run(create_app(synthesize), host='0.0.0.0', port=8100, access_log=False)


if __name__ == '__main__':
    main()
