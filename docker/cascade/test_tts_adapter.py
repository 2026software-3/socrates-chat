"""Contract tests for the cascade speech adapter."""
import unittest

import httpx
from fastapi.testclient import TestClient

import tts_adapter


class AdapterContractTest(unittest.TestCase):
    def test_forwards_speech_and_preserves_pcm_headers(self):
        seen = []

        class AudioStream(httpx.AsyncByteStream):
            async def __aiter__(self):
                yield b'\x01\x02'

        def upstream(request):
            import json
            seen.append(json.loads(request.content))
            return httpx.Response(200, stream=AudioStream(), headers={
                'content-type': 'application/octet-stream', 'x-sample-rate': '24000',
            })

        previous = tts_adapter.client
        tts_adapter.client = httpx.AsyncClient(transport=httpx.MockTransport(upstream))
        try:
            with TestClient(tts_adapter.app) as client:
                response = client.post('/v1/audio/speech', json={
                    'input': '學習', 'model': 'kokoro', 'response_format': 'pcm', 'stream': True,
                })
            self.assertEqual(response.status_code, 200)
            self.assertEqual(response.headers['x-sample-rate'], '24000')
            self.assertEqual(response.content, b'\x01\x02')
            self.assertEqual(seen[0]['input'], '学习')
            self.assertNotIn('max_new_tokens', seen[0])
        finally:
            tts_adapter.client = previous


if __name__ == '__main__':
    unittest.main()
