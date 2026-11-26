#!/usr/bin/env python3
"""Apple Silicon 用的 Whisper 語音辨識服務（MLX／GPU），端點與 scripts/whisper-server.py 相同：
POST /v1/audio/transcriptions → {"text": ...}。

只換推論框架（mlx-whisper），整理文字的規則沿用同一份程式（scripts/whisper_text.py、whisper_echo.py）：
中文輸出臺灣繁體、丟掉 no_speech_prob／avg_logprob 不合格的片段、去掉片尾套語、提示詞洩漏回空字串；
推論前同樣先用 Silero VAD 去掉無語音的部分（借用 faster-whisper 內附的 VAD，在 CPU 上跑）。
與 faster-whisper 版的差別：mlx-whisper 只有貪婪解碼（沒有 beam search）。量測與比較見 README.md。

音訊只在記憶體中處理，不寫入磁碟；記錄不含辨識文字。
環境變數：WHISPER_MLX_MODEL（預設 mlx-community/whisper-small-mlx）、WHISPER_HOST、WHISPER_PORT、
WHISPER_INITIAL_PROMPT、WHISPER_VAD（預設 1）、WHISPER_MLX_CACHE_MB（MLX 緩衝區快取上限，預設 256）。
"""
import asyncio
import io
import os
from concurrent.futures import ThreadPoolExecutor

import uvicorn
from fastapi import FastAPI, File, Form, HTTPException, UploadFile

from whisper_echo import is_prompt_echo, pick_prompt
from whisper_text import keep_segment, normalize, strip_credits

SAMPLE_RATE = 16000


def finalize(segments, language, prompt):
    """把模型的片段整理成要回傳的文字；規則與 scripts/whisper-server.py 相同。"""
    spoken = ''.join(s['text'] for s in segments if keep_segment(s['no_speech_prob'], s['avg_logprob']))
    text = strip_credits(normalize(spoken.strip(), language))
    return '' if is_prompt_echo(text, prompt) else text


def speech_only(audio, find_speech, collect):
    """只留下 VAD 判定有語音的部分；完全沒有語音回傳 None（不交給模型）。"""
    chunks = find_speech(audio)
    if not chunks:
        return None
    return collect([audio[chunk['start']:chunk['end']] for chunk in chunks])


def create_app(recognize, default_prompt=''):
    """recognize(audio_bytes, language 或 None, prompt 或 None) → (片段列表, 語言)。"""
    app = FastAPI()
    # MLX 的模型不能同時推論，也固定在同一條執行緒上；排隊的請求只佔 asyncio 的等待，不佔執行緒。
    lock = asyncio.Lock()
    worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix='whisper-mlx')
    app.state.worker = worker

    @app.post('/v1/audio/transcriptions')
    async def transcribe(file: UploadFile = File(...), model_name: str = Form('', alias='model'),
                         language: str = Form(''), prompt: str = Form('')):
        audio = await file.read()
        used_prompt = pick_prompt(prompt, default_prompt)
        try:
            async with lock:
                segments, detected = await asyncio.get_running_loop().run_in_executor(
                    worker, recognize, audio, language or None, used_prompt)
        except Exception:
            # 回給呼叫端的錯誤不帶內部資訊；記錄也只留類別，不留內容。
            print('asr failed', flush=True)
            raise HTTPException(status_code=500, detail='transcription failed') from None
        return {'text': finalize(segments, detected, used_prompt)}

    return app


def make_recognizer(model, use_vad):
    import mlx_whisper
    import numpy as np
    from faster_whisper.audio import decode_audio
    from faster_whisper.vad import VadOptions, get_speech_timestamps

    def find_speech(audio):
        return get_speech_timestamps(audio, VadOptions())

    def run(audio, language, prompt):
        result = mlx_whisper.transcribe(audio, path_or_hf_repo=model, language=language, initial_prompt=prompt,
                                        condition_on_previous_text=False, verbose=None)
        return result['segments'], result['language']

    def recognize(data, language, prompt):
        audio = decode_audio(io.BytesIO(data), sampling_rate=SAMPLE_RATE)
        if use_vad:
            audio = speech_only(audio, find_speech, np.concatenate)
            if audio is None:
                return [], language or 'zh'
        return run(audio, language, prompt)

    return recognize, run


def main():
    import mlx.core as mx
    import numpy as np

    mx.set_cache_limit(int(os.environ.get('WHISPER_MLX_CACHE_MB', '256')) * 2**20)
    model = os.environ.get('WHISPER_MLX_MODEL', 'mlx-community/whisper-small-mlx')
    recognize, run = make_recognizer(model, os.environ.get('WHISPER_VAD', '1') != '0')
    app = create_app(recognize, os.environ.get('WHISPER_INITIAL_PROMPT', ''))

    # 預熱：在推論用的那條執行緒上載入模型並跑一次（一秒的微弱雜訊，不經 VAD），再跑一次完整流程（解碼＋VAD），
    # 之後才開始接受連線，啟動腳本等到的「就緒」才是真的可用，第一位學生不必付冷啟動。
    noise = (np.random.default_rng(0).standard_normal(SAMPLE_RATE) * 0.01).astype(np.float32)
    app.state.worker.submit(run, noise, 'zh', None).result()
    app.state.worker.submit(recognize, _silent_wav(), 'zh', None).result()

    uvicorn.run(app, host=os.environ.get('WHISPER_HOST', '127.0.0.1'), port=int(os.environ.get('WHISPER_PORT', '8101')),
                access_log=False)


def _silent_wav():
    import wave

    buffer = io.BytesIO()
    with wave.open(buffer, 'wb') as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(SAMPLE_RATE)
        out.writeframes(bytes(SAMPLE_RATE * 2))
    return buffer.getvalue()


if __name__ == '__main__':
    main()
