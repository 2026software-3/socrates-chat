"""Contract tests for the cascade speech service."""
import threading
import time
import unittest

import numpy as np
from fastapi.testclient import TestClient

from kokoro_server import (SAMPLE_RATE, SILENCE, create_app, make_mlx_synthesizer, make_synthesizer, parse_lead_ms,
                           render_pcm, resolve_backend, resolve_device, trim_leading_silence)


def post_speech(client, text='你好'):
    return client.post('/v1/audio/speech', json={'input': text, 'response_format': 'pcm'})


class LeadingSilenceTest(unittest.TestCase):
    """Kokoro 每句開頭約有 0.4 秒靜音；設定 KOKORO_LEAD_SILENCE_MS 後只留指定長度，學生更早聽到第一個字。"""

    def speech(self, lead_ms, voiced=4800):
        return np.concatenate([np.zeros(SAMPLE_RATE * lead_ms // 1000, dtype=np.float32),
                               np.full(voiced, 0.5, dtype=np.float32)])

    def test_long_lead_is_cut_down_to_the_kept_length(self):
        trimmed = trim_leading_silence(self.speech(400), 50)
        self.assertEqual(len(trimmed), SAMPLE_RATE * 50 // 1000 + 4800)
        self.assertEqual(float(trimmed[SAMPLE_RATE * 50 // 1000 - 1]), 0.0)
        self.assertEqual(float(trimmed[SAMPLE_RATE * 50 // 1000]), 0.5)

    def test_low_level_noise_counts_as_silence(self):
        samples = self.speech(400)
        samples[:9600] = 0.001
        self.assertEqual(len(trim_leading_silence(samples, 0)), 4800)

    def test_short_lead_and_trailing_silence_are_untouched(self):
        samples = np.concatenate([self.speech(30), np.zeros(24000, dtype=np.float32)])
        self.assertEqual(trim_leading_silence(samples, 50).tolist(), samples.tolist())

    def test_all_silent_audio_is_untouched(self):
        samples = np.zeros(2400, dtype=np.float32)
        self.assertEqual(len(trim_leading_silence(samples, 50)), 2400)

    def test_default_keeps_the_audio_as_synthesized(self):
        samples = self.speech(400)
        pcm = render_pcm(lambda _: [samples], '你好', threading.Event())
        self.assertEqual(len(pcm), len(samples) * 2)

    def test_endpoint_trims_only_when_configured(self):
        samples = self.speech(400)
        with TestClient(create_app(lambda _: [samples[:6000], samples[6000:]], lead_ms=50)) as client:
            response = post_speech(client)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(len(response.content), (SAMPLE_RATE * 50 // 1000 + 4800) * 2)
        with TestClient(create_app(lambda _: [samples])) as client:
            self.assertEqual(len(post_speech(client).content), len(samples) * 2)

    def test_setting_is_optional_and_validated(self):
        self.assertIsNone(parse_lead_ms(None))
        self.assertIsNone(parse_lead_ms(' '))
        self.assertEqual(parse_lead_ms('50'), 50)
        self.assertEqual(parse_lead_ms('0'), 0)
        for bad in ['-1', 'fast', '1.5']:
            with self.assertRaises(ValueError):
                parse_lead_ms(bad)


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


class BackendSelectionTest(unittest.TestCase):
    """KOKORO_BACKEND 選推論框架；預設維持 PyTorch（NVIDIA 部署不變），mlx 只給 Apple Silicon 用。"""

    def test_default_is_torch_so_the_nvidia_deployment_is_unchanged(self):
        self.assertEqual(resolve_backend(None), 'torch')
        self.assertEqual(resolve_backend(''), 'torch')

    def test_mlx_is_selected_explicitly(self):
        self.assertEqual(resolve_backend(' MLX '), 'mlx')
        self.assertEqual(resolve_backend('torch'), 'torch')

    def test_unknown_backend_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'coreml'):
            resolve_backend('coreml')


class MlxSynthesizerTest(unittest.TestCase):
    class Result:
        def __init__(self, values):
            self.audio = values  # mlx-audio 回傳 mx.array；這裡用 list 代表「可轉成 numpy 的東西」

    def test_mlx_synthesizer_passes_voice_and_language_and_yields_float32_chunks(self):
        calls = []

        class Model:
            def generate(_self, text, **options):
                calls.append((text, options))
                yield self.Result([0.25, -0.25])
                yield self.Result([0.5])

        chunks = list(make_mlx_synthesizer(Model(), 'zm_yunjian')('你好'))
        self.assertEqual(calls, [('你好', {'voice': 'zm_yunjian', 'lang_code': 'z', 'speed': 1.0})])
        self.assertEqual([chunk.tolist() for chunk in chunks], [[0.25, -0.25], [0.5]])
        self.assertTrue(all(chunk.dtype == np.float32 and chunk.ndim == 1 for chunk in chunks))

    def test_mlx_output_goes_through_the_same_pcm_contract(self):
        class Model:
            def generate(_self, _text, **_options):
                yield self.Result([[0.0, 0.5, -0.5]])  # 有些版本多一個批次維度

        with TestClient(create_app(make_mlx_synthesizer(Model(), 'zm_yunjian'))) as client:
            response = post_speech(client)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.headers['x-sample-rate'], '24000')
        self.assertEqual(response.content, b'\x00\x00\x00\x40\x00\xc0')


class DeviceSelectionTest(unittest.TestCase):
    """KOKORO_DEVICE 決定推論裝置；預設維持原本的 cuda，指定的裝置不存在時要明確失敗，不能默默退回 CPU。"""

    def test_default_is_cuda_so_the_nvidia_deployment_is_unchanged(self):
        self.assertEqual(resolve_device(None, cuda=True, mps=False), 'cuda')
        self.assertEqual(resolve_device('', cuda=True, mps=True), 'cuda')

    def test_explicit_devices_are_returned_when_available(self):
        self.assertEqual(resolve_device('mps', cuda=False, mps=True), 'mps')
        self.assertEqual(resolve_device('CPU', cuda=False, mps=False), 'cpu')
        self.assertEqual(resolve_device(' cuda ', cuda=True, mps=False), 'cuda')

    def test_auto_prefers_cuda_then_mps_then_cpu(self):
        self.assertEqual(resolve_device('auto', cuda=True, mps=True), 'cuda')
        self.assertEqual(resolve_device('auto', cuda=False, mps=True), 'mps')
        self.assertEqual(resolve_device('auto', cuda=False, mps=False), 'cpu')

    def test_requested_device_that_is_unavailable_fails_loudly(self):
        with self.assertRaisesRegex(RuntimeError, 'cuda'):
            resolve_device('cuda', cuda=False, mps=True)
        with self.assertRaisesRegex(RuntimeError, 'mps'):
            resolve_device('mps', cuda=True, mps=False)

    def test_unknown_device_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'metal'):
            resolve_device('metal', cuda=True, mps=True)


if __name__ == '__main__':
    unittest.main()
