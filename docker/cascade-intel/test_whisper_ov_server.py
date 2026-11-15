"""NPU 版 Whisper 服務的契約測試：模型以函式注入，不需要 NPU。

執行：PYTHONPATH=docker/cascade-intel:scripts python3 -m unittest docker/cascade-intel/test_whisper_ov_server.py
（需要 fastapi、httpx、python-multipart、av、numpy、opencc）
"""
import io
import threading
import time
import unittest
import wave

import numpy as np
from fastapi.testclient import TestClient

from whisper_ov_server import create_app, decode_audio, is_speech, speech_only


def wav_bytes(samples, rate=16000):
    pcm = (np.clip(samples, -1, 1) * 32767).astype('<i2')
    buf = io.BytesIO()
    with wave.open(buf, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


def tone(seconds=2.0, rate=16000, amp=0.3):
    t = np.arange(int(seconds * rate)) / rate
    return (amp * np.sin(2 * np.pi * 220 * t)).astype(np.float32)


def post(client, audio, **form):
    return client.post('/v1/audio/transcriptions', files={'file': ('a.wav', audio, 'audio/wav')}, data=form)


class TranscriptionContractTest(unittest.TestCase):
    def test_returns_text_json_and_passes_audio_and_language_to_the_model(self):
        seen = {}

        def transcribe(samples, language):
            seen['n'] = len(samples)
            seen['language'] = language
            return '你好'

        with TestClient(create_app(transcribe)) as client:
            response = post(client, wav_bytes(tone(2.0)), language='zh', model='whisper-small')
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json(), {'text': '你好'})
        self.assertEqual(seen['n'], 32000)
        self.assertEqual(seen['language'], 'zh')

    def test_chinese_output_is_traditional_and_credits_are_removed(self):
        with TestClient(create_app(lambda s, l: '你觉得民主制度怎么样 字幕by某某')) as client:
            text = post(client, wav_bytes(tone()), language='zh').json()['text']
        self.assertEqual(text, '你覺得民主制度怎麼樣')

    def test_missing_language_lets_the_model_detect(self):
        seen = {}

        def transcribe(samples, language):
            seen['language'] = language
            return 'x'

        with TestClient(create_app(transcribe)) as client:
            post(client, wav_bytes(tone()))
        self.assertIsNone(seen['language'])

    def test_silence_returns_empty_text_without_calling_the_model(self):
        def transcribe(samples, language):
            raise AssertionError('model must not run on silence')

        with TestClient(create_app(transcribe)) as client:
            quiet = post(client, wav_bytes(np.zeros(32000, dtype=np.float32)), language='zh')
            faint = post(client, wav_bytes(tone(amp=0.0005)), language='zh')
        self.assertEqual(quiet.json(), {'text': ''})
        self.assertEqual(faint.json(), {'text': ''})

    def test_very_short_audio_returns_empty_text_without_calling_the_model(self):
        def transcribe(samples, language):
            raise AssertionError('model must not run on clicks')

        with TestClient(create_app(transcribe)) as client:
            response = post(client, wav_bytes(tone(0.1)), language='zh')
        self.assertEqual(response.json(), {'text': ''})

    def test_audio_without_speech_according_to_vad_never_reaches_the_model(self):
        def transcribe(samples, language):
            raise AssertionError('model must not run when VAD finds no speech')

        with TestClient(create_app(transcribe, vad=lambda s: s[:0])) as client:
            response = post(client, wav_bytes(tone()), language='zh')
        self.assertEqual(response.json(), {'text': ''})

    def test_model_only_receives_the_speech_part_chosen_by_vad(self):
        seen = {}

        def transcribe(samples, language):
            seen['n'] = len(samples)
            return '好'

        with TestClient(create_app(transcribe, vad=lambda s: s[:16000])) as client:
            post(client, wav_bytes(tone(3.0)), language='zh')
        self.assertEqual(seen['n'], 16000)

    def test_prompt_echo_is_dropped(self):
        prompt = '蘇格拉底、產婆術、反詰法'
        with TestClient(create_app(lambda s, l: '蘇格拉底、產婆術、反詰法')) as client:
            response = post(client, wav_bytes(tone()), language='zh', prompt=prompt)
        self.assertEqual(response.json(), {'text': ''})

    def test_model_failure_does_not_expose_internal_errors(self):
        def transcribe(samples, language):
            raise RuntimeError('secret NPU driver path /opt/x')

        with TestClient(create_app(transcribe), raise_server_exceptions=False) as client:
            response = post(client, wav_bytes(tone()), language='zh')
        self.assertEqual(response.status_code, 500)
        self.assertNotIn('secret', response.text)

    def test_undecodable_upload_is_a_client_error(self):
        with TestClient(create_app(lambda s, l: 'x')) as client:
            response = post(client, b'not audio at all', language='zh')
        self.assertEqual(response.status_code, 400)

    def test_requests_run_one_at_a_time_because_the_npu_pipeline_is_single_stream(self):
        active = {'now': 0, 'max': 0}
        guard = threading.Lock()

        def transcribe(samples, language):
            with guard:
                active['now'] += 1
                active['max'] = max(active['max'], active['now'])
            time.sleep(0.05)
            with guard:
                active['now'] -= 1
            return '好'

        with TestClient(create_app(transcribe)) as client:
            threads = [threading.Thread(target=lambda: post(client, wav_bytes(tone()), language='zh')) for _ in range(4)]
            [t.start() for t in threads]
            [t.join() for t in threads]
        self.assertEqual(active['max'], 1)

    def test_openapi_is_available_for_the_compose_healthcheck(self):
        with TestClient(create_app(lambda s, l: '')) as client:
            self.assertEqual(client.get('/openapi.json').status_code, 200)


class AudioHelpersTest(unittest.TestCase):
    def test_decode_resamples_to_16khz_mono_float32(self):
        samples = decode_audio(wav_bytes(tone(1.0, rate=24000), rate=24000))
        self.assertEqual(samples.dtype, np.float32)
        self.assertEqual(samples.ndim, 1)
        self.assertAlmostEqual(len(samples), 16000, delta=200)

    def test_silero_vad_drops_noise_that_passes_the_level_gate(self):
        rng = np.random.default_rng(0)
        for name, x in {
            'white noise': rng.normal(0, 0.05, 64000),
            'hum': 0.05 * np.sin(2 * np.pi * 120 * np.arange(64000) / 16000) + rng.normal(0, 0.01, 64000),
        }.items():
            self.assertEqual(len(speech_only(x.astype(np.float32))), 0, name)

    def test_is_speech_needs_enough_length_and_level(self):
        self.assertTrue(is_speech(tone(1.0)))
        self.assertFalse(is_speech(tone(0.1)))
        self.assertFalse(is_speech(np.zeros(16000, dtype=np.float32)))


if __name__ == '__main__':
    unittest.main()
