"""macOS 啟動腳本的靜態檢查：語法，以及 bash 會把全形字元吃進變數名稱的陷阱。"""
import os
import pathlib
import platform
import re
import shutil
import subprocess
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPTS = sorted(HERE.glob('*.sh'))
# `$NAME` 後面直接接非 ASCII 字元（例如全形逗號、括號）時，bash 會把它當成變數名稱的一部分，
# 在 `set -u` 下變成「unbound variable」。要寫成 `${NAME}`。
UNBRACED = re.compile(r'\$([A-Za-z_][A-Za-z0-9_]*)(?=[^\x00-\x7f])')


class ShellScriptsTest(unittest.TestCase):
    def test_scripts_exist_and_are_executable(self):
        names = {path.name for path in SCRIPTS}
        self.assertTrue({'install.sh', 'start.sh', 'stop.sh', 'status.sh', 'lib.sh'} <= names)
        for path in SCRIPTS:
            if path.name != 'lib.sh':  # lib.sh 只被 source
                self.assertTrue(path.stat().st_mode & 0o111, f'{path.name} is not executable')

    def test_scripts_pass_bash_syntax_check(self):
        bash = shutil.which('bash')
        self.assertIsNotNone(bash)
        for path in SCRIPTS:
            result = subprocess.run([bash, '-n', str(path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, f'{path.name}: {result.stderr}')

    def test_no_variable_is_glued_to_a_non_ascii_character(self):
        for path in SCRIPTS:
            for number, line in enumerate(path.read_text().splitlines(), 1):
                match = UNBRACED.search(line)
                self.assertIsNone(match, f'{path.name}:{number}: write ${{{match.group(1) if match else ""}}} with braces')

    def test_token_is_never_written_into_a_tracked_file(self):
        # 密碼只能從環境變數來；腳本裡不能出現給它賦值的字面值
        for path in SCRIPTS:
            self.assertNotRegex(path.read_text(), r'CASCADE_LLM_TOKEN=["\']?[A-Za-z0-9]{8,}')


@unittest.skipUnless(platform.system() == 'Darwin' and platform.machine() == 'arm64', 'start.sh 只在 Apple Silicon 上執行')
class StartOptionsTest(unittest.TestCase):
    """推論後端打錯字時要在啟動任何服務之前就失敗，不能默默用別的後端。"""

    def run_start(self, **env):
        return subprocess.run(
            [str(HERE / 'start.sh')], capture_output=True, text=True, timeout=30,
            env={'PATH': os.environ['PATH'], 'HOME': os.environ['HOME'], 'CASCADE_LLM_TOKEN': 'x',
                 'CASCADE_MACOS_HOME': '/nonexistent/cascade-macos-test', **env})

    def test_unknown_whisper_backend_is_rejected(self):
        result = self.run_start(WHISPER_BACKEND='whisper.cpp')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('WHISPER_BACKEND', result.stderr)

    def test_unknown_kokoro_backend_is_rejected(self):
        result = self.run_start(KOKORO_BACKEND='coreml')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('KOKORO_BACKEND', result.stderr)

    def test_valid_backends_get_past_validation(self):
        # 沒有虛擬環境，所以停在下一步的檢查；重點是不會被後端檢查擋下
        for env in ({}, {'WHISPER_BACKEND': 'faster-whisper', 'KOKORO_BACKEND': 'torch'}):
            result = self.run_start(**env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('install.sh', result.stderr)


class DefaultsTest(unittest.TestCase):
    def test_start_passes_the_backends_and_lead_silence_to_the_services(self):
        text = (HERE / 'start.sh').read_text()
        for needle in ['WHISPER_BACKEND="${WHISPER_BACKEND:-mlx}"', 'KOKORO_BACKEND="${KOKORO_BACKEND:-mlx}"',
                       'KOKORO_LEAD_SILENCE_MS="${KOKORO_LEAD_SILENCE_MS-50}"', 'whisper_mlx_server.py',
                       'scripts/whisper-server.py', 'KOKORO_LEAD_SILENCE_MS="$KOKORO_LEAD_SILENCE_MS"']:
            self.assertIn(needle, text)

    def test_install_provides_both_the_mlx_and_fallback_packages(self):
        text = (HERE / 'install.sh').read_text()
        for needle in ['mlx-whisper', 'mlx-audio', 'faster-whisper', 'kokoro==0.9.4']:
            self.assertIn(needle, text)


if __name__ == '__main__':
    unittest.main()
