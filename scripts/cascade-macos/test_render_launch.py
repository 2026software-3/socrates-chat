"""render_launch：把 docker/cascade/launch.yaml 轉成本機（macOS 原生）版，不複製一份設定。"""
import pathlib
import unittest

from render_launch import render

LAUNCH = pathlib.Path(__file__).resolve().parents[2] / 'docker' / 'cascade' / 'launch.yaml'
SAMPLE = '''model_path: /app
    base_url: "http://cascade-adapter:8103/v1"
    base_url: "http://app:3000/internal/cascade/v1"
    base_url: "http://cascade-adapter:8103/v1"
'''


class RenderLaunchTest(unittest.TestCase):
    def test_replaces_container_hosts_and_model_path(self):
        out = render(SAMPLE, app_url='http://127.0.0.1:3000', model_path='/tmp/run', adapter_url='http://127.0.0.1:8103')
        self.assertIn('model_path: /tmp/run\n', out)
        self.assertEqual(out.count('http://127.0.0.1:8103/v1'), 2)
        self.assertIn('http://127.0.0.1:3000/internal/cascade/v1', out)
        self.assertNotIn('cascade-adapter', out)
        self.assertNotIn('http://app:', out)

    def test_app_url_trailing_slash_is_tolerated(self):
        out = render(SAMPLE, app_url='http://127.0.0.1:3000/', model_path='/tmp/run', adapter_url='http://127.0.0.1:8103')
        self.assertIn('"http://127.0.0.1:3000/internal/cascade/v1"', out)

    def test_fails_when_the_template_no_longer_has_the_expected_lines(self):
        with self.assertRaisesRegex(ValueError, 'cascade-adapter'):
            render('model_path: /app\nbase_url: "http://app:3000/internal/cascade/v1"\n',
                   app_url='http://x', model_path='/r', adapter_url='http://y')
        with self.assertRaisesRegex(ValueError, 'model_path'):
            render(SAMPLE.replace('model_path: /app', 'model_path: /other'),
                   app_url='http://x', model_path='/r', adapter_url='http://y')

    def test_real_launch_yaml_renders_without_leftover_container_names(self):
        out = render(LAUNCH.read_text(), app_url='http://127.0.0.1:3000', model_path='/tmp/run', adapter_url='http://127.0.0.1:8103')
        for leftover in ('cascade-adapter', 'http://app:', 'model_path: /app'):
            self.assertNotIn(leftover, out)
        self.assertIn('api_key_env: CASCADE_LLM_TOKEN', out)  # 密碼仍只從環境變數讀


if __name__ == '__main__':
    unittest.main()
