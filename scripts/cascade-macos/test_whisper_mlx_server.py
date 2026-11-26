"""whisper_mlx_server 的契約測試：換了推論框架，整理文字的規則必須與 scripts/whisper-server.py 相同。

只用假的辨識函式，不需要 MLX、模型或 Apple Silicon。
執行：PYTHONPATH=scripts/cascade-macos:scripts python -m unittest test_whisper_mlx_server
"""
import threading
import time
import unittest

from fastapi.testclient import TestClient

from whisper_mlx_server import create_app, finalize, speech_only


def seg(text, no_speech_prob=0.05, avg_logprob=-0.2):
    return {'text': text, 'no_speech_prob': no_speech_prob, 'avg_logprob': avg_logprob}


def post(client, **form):
    return client.post('/v1/audio/transcriptions', files={'file': ('audio.wav', b'RIFF-fake', 'audio/wav')},
                       data={'model': 'whisper-small', 'response_format': 'json', **form})


class FinalizeTest(unittest.TestCase):
    def test_simplified_output_becomes_taiwan_traditional(self):
        self.assertEqual(finalize([seg('苏格拉底说这样的行为')], 'zh', None), '蘇格拉底說這樣的行為')

    def test_other_languages_are_left_alone(self):
        self.assertEqual(finalize([seg(' I think so.')], 'en', None), 'I think so.')

    def test_segments_that_look_like_silence_or_garbage_are_dropped(self):
        segments = [seg('我覺得是。'), seg('謝謝觀看', no_speech_prob=0.6), seg('亂碼', avg_logprob=-1.01), seg('因為公平。')]
        self.assertEqual(finalize(segments, 'zh', None), '我覺得是。因為公平。')

    def test_thresholds_match_whisper_text(self):
        self.assertEqual(finalize([seg('好', no_speech_prob=0.59, avg_logprob=-1.0)], 'zh', None), '好')

    def test_video_credits_are_stripped(self):
        self.assertEqual(finalize([seg('我覺得是。字幕by索蘭婭')], 'zh', None), '我覺得是。')
        self.assertEqual(finalize([seg('請不吝點贊訂閱轉發打賞支持明鏡與點點欄目')], 'zh', None), '')

    def test_prompt_echo_is_dropped_but_real_answers_are_kept(self):
        prompt = '蘇格拉底、柏拉圖、產婆術、理型論'
        self.assertEqual(finalize([seg('蘇格拉底、柏拉圖、產婆術')], 'zh', prompt), '')
        self.assertEqual(finalize([seg('產婆術')], 'zh', prompt), '產婆術')

    def test_no_segments_is_an_empty_transcript(self):
        self.assertEqual(finalize([], 'zh', None), '')


class EndpointTest(unittest.TestCase):
    def test_returns_openai_shaped_json_with_normalized_text(self):
        seen = []

        def recognize(audio, language, prompt):
            seen.append((audio, language, prompt))
            return [seg('我觉得'), seg('请订阅', no_speech_prob=0.9), seg('是这样。')], 'zh'

        with TestClient(create_app(recognize)) as client:
            response = post(client, language='zh')
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json(), {'text': '我覺得是這樣。'})
        self.assertEqual(seen, [(b'RIFF-fake', 'zh', None)])

    def test_request_prompt_wins_over_the_default_and_echo_is_dropped(self):
        seen = []

        def recognize(_audio, _language, prompt):
            seen.append(prompt)
            return [seg('產婆術、產婆術')], 'zh'

        with TestClient(create_app(recognize, default_prompt='預設詞')) as client:
            self.assertEqual(post(client, language='zh', prompt='產婆術').json(), {'text': ''})
            post(client, language='zh')
        self.assertEqual(seen, ['產婆術', '預設詞'])

    def test_missing_language_lets_the_model_detect(self):
        seen = []

        def recognize(_audio, language, _prompt):
            seen.append(language)
            return [seg('hello')], 'en'

        with TestClient(create_app(recognize)) as client:
            self.assertEqual(post(client).json(), {'text': 'hello'})
        self.assertEqual(seen, [None])

    def test_failure_is_a_500_without_internal_details(self):
        def recognize(*_):
            raise RuntimeError('internal secret /path/to/model')

        with TestClient(create_app(recognize), raise_server_exceptions=False) as client:
            response = post(client, language='zh')
        self.assertEqual(response.status_code, 500)
        self.assertNotIn('secret', response.text)

    def test_file_is_required(self):
        with TestClient(create_app(lambda *_: ([], 'zh'))) as client:
            self.assertEqual(client.post('/v1/audio/transcriptions', data={'language': 'zh'}).status_code, 422)

    def test_requests_run_one_at_a_time_on_a_single_thread(self):
        # MLX 的模型不能多執行緒同時推論；也要固定在同一條執行緒上。
        state = {'active': 0, 'peak': 0, 'threads': set()}
        guard = threading.Lock()

        def recognize(*_):
            with guard:
                state['active'] += 1
                state['peak'] = max(state['peak'], state['active'])
                state['threads'].add(threading.get_ident())
            time.sleep(0.05)
            with guard:
                state['active'] -= 1
            return [seg('好')], 'zh'

        codes = []
        with TestClient(create_app(recognize)) as client:
            threads = [threading.Thread(target=lambda: codes.append(post(client, language='zh').status_code)) for _ in range(4)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
        self.assertEqual(codes, [200] * 4)
        self.assertEqual(state['peak'], 1)
        self.assertEqual(len(state['threads']), 1)


class SpeechOnlyTest(unittest.TestCase):
    """先用 VAD 去掉沒有語音的部分；整段都沒有語音時不呼叫模型（small 遇到靜音會幻想出內容）。"""

    def test_keeps_only_the_detected_speech(self):
        audio = list(range(10))
        kept = speech_only(audio, lambda _: [{'start': 2, 'end': 4}, {'start': 7, 'end': 9}], lambda parts: sum(parts, []))
        self.assertEqual(kept, [2, 3, 7, 8])

    def test_no_speech_returns_none(self):
        self.assertIsNone(speech_only([0, 0, 0], lambda _: [], lambda parts: parts))


if __name__ == '__main__':
    unittest.main()
