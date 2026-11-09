"""patch_dialog 的測試。用法（需安裝 sglang-omni==0.2.0 的 python）：python3 docker/cascade/test_patch_dialog.py

不修改已安裝的套件：把安裝的 dialog.py 讀進來、套補丁後載入成另一個模組來跑。"""
import importlib.metadata
import importlib.util
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).parent))
import patch_dialog  # noqa: E402

from sglang_omni.models.streaming_cascade.base import control_chunk  # noqa: E402
from sglang_omni.proto.session import TimedChunk  # noqa: E402
from sglang_omni.proto.streams import (  # noqa: E402
    SPEECH_STARTED,
    SPEECH_STOPPED,
    TRANSCRIPT_FORMAT,
    control_type,
)

INSTALLED = pathlib.Path(patch_dialog.installed_dialog_path())


def load(source: str, name: str):
    spec = importlib.util.spec_from_loader(name, loader=None)
    module = importlib.util.module_from_spec(spec)
    exec(compile(source, name, "exec"), module.__dict__)
    return module


def transcript(text: str) -> TimedChunk:
    return TimedChunk(
        modality="text", t_start_ms=0.0, duration_ms=0.0, seq=0,
        payload={"text": text, "is_final": True}, format=TRANSCRIPT_FORMAT,
    )


def feed(dialog, *steps):
    """每一步送進一個 chunk，回傳每一步新增的輸出種類。"""
    out, kinds = [], []
    dialog.emit = out.append
    for chunk in steps:
        before = len(out)
        dialog.handle(chunk)
        dialog.respond_to_waiting()
        kinds.append([control_type(c) or c.format for c in out[before:]])
    return kinds


class PatchDialog(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        assert importlib.metadata.version("sglang-omni") == "0.2.0"
        cls.original = INSTALLED.read_text(encoding="utf-8")
        cls.patched_source = patch_dialog.patch_source(cls.original)

    def dialogs(self):
        base = load(self.original, "dialog_original")
        patched = load(self.patched_source, "dialog_patched")
        return base.Dialog(base.DialogConfig()), patched.Dialog(patched.DialogConfig())

    def test_baseline_cuts_the_reply_as_it_opens_when_speech_continues(self):
        # 保留這個對照：證明下面的差異來自補丁，而不是情境本身
        base, _ = self.dialogs()
        kinds = feed(
            base,
            control_chunk(SPEECH_STARTED), control_chunk(SPEECH_STOPPED),
            control_chunk(SPEECH_STARTED), transcript("甲"),
        )
        self.assertIn("interrupt", kinds[-1])

    def test_does_not_open_a_reply_while_the_user_is_speaking(self):
        _, patched = self.dialogs()
        kinds = feed(
            patched,
            control_chunk(SPEECH_STARTED), control_chunk(SPEECH_STOPPED),
            control_chunk(SPEECH_STARTED), transcript("甲"),
        )
        self.assertNotIn("messages", kinds[-1])
        self.assertNotIn("interrupt", kinds[-1])

    def test_opens_the_waiting_reply_once_the_speech_stops(self):
        _, patched = self.dialogs()
        kinds = feed(
            patched,
            control_chunk(SPEECH_STARTED), control_chunk(SPEECH_STOPPED),
            control_chunk(SPEECH_STARTED), transcript("甲"), control_chunk(SPEECH_STOPPED),
        )
        self.assertIn("messages", kinds[-1])
        self.assertNotIn("interrupt", kinds[-1])
        self.assertNotIn("turn.interrupted", kinds[-1])

    def test_a_turn_without_overlap_is_unchanged(self):
        base, patched = self.dialogs()
        steps = (control_chunk(SPEECH_STARTED), control_chunk(SPEECH_STOPPED), transcript("甲"))
        self.assertEqual(feed(base, *steps), feed(patched, *steps))

    def test_patching_twice_changes_nothing_more(self):
        self.assertEqual(patch_dialog.patch_source(self.patched_source), self.patched_source)

    def test_refuses_source_it_does_not_recognise(self):
        with self.assertRaises(RuntimeError):
            patch_dialog.patch_source("class Dialog:\n    pass\n")

    def test_startup_script_fails_when_the_installed_source_is_not_the_expected_one(self):
        # 升版後對不上原始碼：patch_dialog.py 以例外結束（非 0），entrypoint 的 set -e 擋住 exec，cascade 不會帶著沒補丁的 Dialog 啟動
        with tempfile.TemporaryDirectory() as tmp:
            other = pathlib.Path(tmp, "dialog.py")
            other.write_text("class Dialog:\n    pass\n", encoding="utf-8")
            with mock.patch.object(patch_dialog, "installed_dialog_path", return_value=str(other)):
                with self.assertRaises(RuntimeError):
                    patch_dialog.main()
            self.assertEqual(other.read_text(encoding="utf-8"), "class Dialog:\n    pass\n")


class Entrypoint(unittest.TestCase):
    script = pathlib.Path(__file__).parent.joinpath("entrypoint.sh").read_text(encoding="utf-8")

    def test_stops_on_any_failing_command(self):
        self.assertIn("\nset -eu\n", self.script)

    def test_applies_the_patch_before_the_cascade_server_starts(self):
        cascade = self.script[self.script.index("  cascade)") :]
        self.assertLess(cascade.index("python3 /app/patch_dialog.py"), cascade.index("exec sgl-omni serve --config"))


if __name__ == "__main__":
    unittest.main()
