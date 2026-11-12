"""Contract tests for the cascade speech service."""
import threading
import time
import unittest

import numpy as np
from fastapi.testclient import TestClient

from kokoro_server import SILENCE, create_app, make_synthesizer, render_pcm


def post_speech(client, text='你好'):
    return client.post('/v1/audio/speech', json={'input': text, 'response_format': 'pcm'})


class SpeechContractTest(unittest.TestCase):
    def test_speech_returns_24khz_signed_little_endian_pcm(self):
        def synthesize(text):
            self.assertEqual(text, "你好")
            return [np.array([0.0, 0.5, -0.5], dtype=np.float32)]

        with TestClient(create_app(synthesize)) as client:
            response = client.post('/v1/audio/speech', json={
                'model': 'kokoro', 'voice': 'zm_yunjian', 'input': '你好',
                'response_format': 'pcm', 'stream': True, 'language': 'Chinese',
            })
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.headers['x-sample-rate'], '24000')
        self.assertEqual(response.content, b'\x00\x00\x00\x40\x00\xc0')

    def test_failed_synthesis_does_not_expose_internal_errors(self):
        def fail(_text):
            raise RuntimeError('internal secret')

        with TestClient(create_app(fail)) as client:
            response = post_speech(client)
        self.assertEqual(response.status_code, 500)
        self.assertNotIn('internal secret', response.text)

    def test_rejects_unsupported_format(self):
        with TestClient(create_app(lambda _: [])) as client:
            response = client.post('/v1/audio/speech', json={'input': '你好', 'response_format': 'mp3'})
        self.assertEqual(response.status_code, 400)

    def test_rejects_whitespace_only_input(self):
        with TestClient(create_app(lambda _: [])) as client:
            response = post_speech(client, '  \n')
        self.assertEqual(response.status_code, 400)


class UnspeakableTextTest(unittest.TestCase):
    """cascade 遇到 TTS 回 500 會關閉整場會話，沒有可唸內容必須是成功的靜音。"""

    def test_punctuation_symbols_and_emoji_return_silence_without_calling_the_model(self):
        def must_not_run(_text):
            raise AssertionError('model should not be called')

        with TestClient(create_app(must_not_run)) as client:
            for text in ['…', '。', '——', '- ', '• ', '😀', '？！']:
                with self.subTest(text=text):
                    response = post_speech(client, text)
                    self.assertEqual(response.status_code, 200)
                    self.assertEqual(response.headers['x-sample-rate'], '24000')
                    self.assertEqual(response.content, SILENCE)

    def test_speakable_text_that_yields_no_audio_returns_silence(self):
        with TestClient(create_app(lambda _: [])) as client:
            response = post_speech(client, '你好')
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.content, SILENCE)

    def test_silence_is_valid_pcm_of_whole_samples(self):
        self.assertGreater(len(SILENCE), 0)
        self.assertEqual(len(SILENCE) % 2, 0)
        self.assertEqual(set(SILENCE), {0})


class ConcurrencyTest(unittest.TestCase):
    def test_requests_are_synthesized_one_at_a_time(self):
        state = {'active': 0, 'peak': 0}
        guard = threading.Lock()

        def synthesize(_text):
            with guard:
                state['active'] += 1
                state['peak'] = max(state['peak'], state['active'])
            time.sleep(0.05)
            with guard:
                state['active'] -= 1
            return [np.array([0.1], dtype=np.float32)]

        codes = []
        with TestClient(create_app(synthesize)) as client:
            def call():
                codes.append(post_speech(client).status_code)

            threads = [threading.Thread(target=call) for _ in range(4)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
        self.assertEqual(codes, [200] * 4)
        self.assertEqual(state['peak'], 1)

    def test_health_stays_responsive_while_requests_are_queued(self):
        release = threading.Event()

        def synthesize(_text):
            release.wait(5)
            return [np.array([0.1], dtype=np.float32)]

        with TestClient(create_app(synthesize)) as client:
            threads = [threading.Thread(target=post_speech, args=(client,)) for _ in range(6)]
            for thread in threads:
                thread.start()
            time.sleep(0.2)
            started = time.monotonic()
            health = client.get('/health')
            elapsed = time.monotonic() - started
            release.set()
            for thread in threads:
                thread.join()
        self.assertEqual(health.status_code, 200)
        self.assertLess(elapsed, 1.0)


class CancellationTest(unittest.TestCase):
    def test_render_stops_between_chunks_once_cancelled(self):
        cancelled = threading.Event()
        produced = []

        def synthesize(_text):
            for index in range(5):
                produced.append(index)
                if index == 1:
                    cancelled.set()
                yield np.array([0.1], dtype=np.float32)

        self.assertIsNone(render_pcm(synthesize, '你好', cancelled))
        self.assertEqual(produced, [0, 1])

    def test_render_cancelled_before_start_returns_none_for_single_chunk(self):
        cancelled = threading.Event()
        cancelled.set()
        self.assertIsNone(render_pcm(lambda _: [np.array([0.1], dtype=np.float32)], '你好', cancelled))


class SynthesizerTest(unittest.TestCase):
    class Chunk:
        def __init__(self, values):
            self.values = np.array(values, dtype=np.float32)

        def numpy(self):
            return self.values

    def test_make_synthesizer_uses_voice_and_yields_numpy_chunks(self):
        calls = []

        def pipeline(text, voice):
            calls.append((text, voice))
            yield 'g', 'p', self.Chunk([0.25])
            yield 'g', 'p', self.Chunk([-0.25])

        chunks = list(make_synthesizer(pipeline, 'zm_yunjian')('你好'))
        self.assertEqual(calls, [('你好', 'zm_yunjian')])
        self.assertEqual([chunk.tolist() for chunk in chunks], [[0.25], [-0.25]])


if __name__ == '__main__':
    unittest.main()
