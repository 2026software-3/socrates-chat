"""Contract tests for the cascade speech service."""
import unittest

import numpy as np
from fastapi.testclient import TestClient

from kokoro_server import create_app


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
            response = client.post('/v1/audio/speech', json={'input': '你好', 'response_format': 'pcm'})
        self.assertEqual(response.status_code, 500)
        self.assertNotIn('internal secret', response.text)

    def test_rejects_unsupported_format(self):
        with TestClient(create_app(lambda _: [])) as client:
            response = client.post('/v1/audio/speech', json={'input': '你好', 'response_format': 'mp3'})
        self.assertEqual(response.status_code, 400)


if __name__ == '__main__':
    unittest.main()
