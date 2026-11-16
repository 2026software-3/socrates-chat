"""smoke 測試客戶端：用 TTS 合成一句話當「學生的聲音」，經 cascade 的 realtime WebSocket 跑完整一輪，印出各階段時間。

流程：TTS（cascade-adapter 的 /v1/audio/speech）→ 24 kHz 轉 16 kHz → 每 100 ms 一包送進 cascade →
等 `response.done`。輸出只含時間與長度，辨識文字只印字數與是否含預期關鍵字（不印全文）。
用法：python chain_client.py [句子]（預設一句繁體問句）。環境變數 CASCADE_URL、ADAPTER_URL；
經 app 代理測試時再設 COOKIE（學生的 session cookie）與 ORIGIN（APP_BASE_URL），CASCADE_URL 指向 ws://app:3000/api/conversations/<id>/voice。
"""
import asyncio
import base64
import json
import os
import sys
import time
import uuid

import httpx
import numpy as np
import websockets

CASCADE_URL = os.environ.get('CASCADE_URL', 'ws://cascade:8110/v1/realtime')
ADAPTER_URL = os.environ.get('ADAPTER_URL', 'http://cascade-adapter:8103/v1/audio/speech')
SENTENCE = sys.argv[1] if len(sys.argv) > 1 else '我覺得民主制度比較能保障個人自由，因為人民可以自己選擇領導者。'
KEYWORDS = ['民主', '自由']
FRAME_SAMPLES = 1600  # 100 ms @ 16 kHz


def synth(text):
    r = httpx.post(ADAPTER_URL, json={'input': text, 'response_format': 'pcm', 'model': 'x', 'voice': 'x'}, timeout=60)
    r.raise_for_status()
    pcm24 = np.frombuffer(r.content, dtype='<i2').astype(np.float32) / 32768
    idx = (np.arange(int(len(pcm24) * 16000 / 24000)) * 1.5).astype(int)
    return pcm24[idx]


def encode(samples):
    return base64.b64encode((np.clip(samples, -1, 1) * 32767).astype('<i2').tobytes()).decode()


def event(payload):
    return json.dumps({**payload, 'event_id': uuid.uuid4().hex})


async def main():
    speech = synth(SENTENCE)
    audio = np.concatenate([np.zeros(8000, dtype=np.float32), speech, np.zeros(16000 * 3, dtype=np.float32)])
    print(f'speech_s={len(speech) / 16000:.2f}')
    marks = {}
    transcript = ''
    deltas = audio_chunks = 0
    # 經真的 app 代理（/api/conversations/{id}/voice）測試時，帶學生的 session cookie 與符合 APP_BASE_URL 的 Origin
    headers = {k: v for k, v in (('Cookie', os.environ.get('COOKIE')), ('Origin', os.environ.get('ORIGIN'))) if v}
    async with websockets.connect(CASCADE_URL, max_size=None, additional_headers=headers or None) as ws:
        await ws.send(event({'type': 'session.update', 'session': {'type': 'realtime', 'instructions': 'smoke', 'audio': {'input': {'format': {'type': 'audio/pcm', 'rate': 16000}}}}}))

        async def sender():
            speech_end = (8000 + len(speech)) / 16000
            t0 = time.time()
            for seq, i in enumerate(range(0, len(audio), FRAME_SAMPLES)):
                await ws.send(event({'type': 'input_audio_buffer.append', 'audio': encode(audio[i:i + FRAME_SAMPLES]), 'sglang': {'seq': seq}}))
                await asyncio.sleep(max(0, t0 + (i + FRAME_SAMPLES) / 16000 - time.time()))
                if 'speech_end' not in marks and (i + FRAME_SAMPLES) / 16000 >= speech_end:
                    marks['speech_end'] = time.time()

        send_task = asyncio.create_task(sender())
        try:
            async with asyncio.timeout(60):
                async for raw in ws:
                    ev = json.loads(raw)
                    t = ev.get('type', '')
                    now = time.time()
                    if t == 'conversation.item.input_audio_transcription.completed':
                        marks['transcript'] = now
                        transcript = ev.get('transcript', '')
                    elif t == 'response.output_text.delta':
                        deltas += 1
                        marks.setdefault('first_text', now)
                    elif t == 'response.output_audio.delta':
                        audio_chunks += 1
                        marks.setdefault('first_audio', now)
                    elif t == 'response.done':
                        marks['done'] = now
                        break
                    elif t == 'error':
                        print('error event:', ev.get('error', {}).get('code'))
        finally:
            send_task.cancel()
    base = marks.get('speech_end', 0)
    for k in ('transcript', 'first_text', 'first_audio', 'done'):
        print(f'{k}_after_speech_end_s=' + (f'{marks[k] - base:.2f}' if k in marks else 'MISSING'))
    print(f'transcript_chars={len(transcript)} keywords_hit={[k for k in KEYWORDS if k in transcript]} text_deltas={deltas} audio_chunks={audio_chunks}')
    ok = bool(transcript) and deltas > 0 and audio_chunks > 0 and 'done' in marks
    print('RESULT', 'PASS' if ok else 'FAIL')
    sys.exit(0 if ok else 1)


asyncio.run(main())
