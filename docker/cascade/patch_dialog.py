"""cascade 啟動前套用的小補丁：使用者還在說話時，Dialog 不開口，等說完再開。

為什麼：sglang-omni 0.2.0 的 Dialog 在回覆一開始時若 VAD 仍判定使用者在說話，會把這個回覆當成
「蓋在使用者說話上面」而立刻 interrupt（見該檔 Dialog 的 docstring）。語音辨識（whisper-small CPU）要 1–4 秒，
第一句辨識完成時學生常常已經在說第二句，結果第一句的回覆一開就被取消、學生什麼都沒聽到。
補丁只多一個條件：`is_user_speaking` 時 respond_to_waiting 不開口；說完（speech_stopped）後原本就會再呼叫
一次，等待中的發言這時才開口。沒有重疊的回合行為不變。

只認得 0.2.0 的原始碼：找不到預期的程式碼就報錯，不會默默跳過（升版時要重新檢視）。
用法：python3 patch_dialog.py        （就地修改已安裝的 dialog.py，可重複執行）"""
import importlib.util
import pathlib

ANCHOR = """        while (
            self.reply is None
            and self.waiting
"""
PATCHED = """        while (
            self.reply is None
            and not self.is_user_speaking
            and self.waiting
"""


def installed_dialog_path() -> str:
    spec = importlib.util.find_spec("sglang_omni.models.streaming_cascade.dialog")
    if spec is None or spec.origin is None:
        raise RuntimeError("找不到 sglang_omni 的 dialog.py")
    return spec.origin


def patch_source(source: str) -> str:
    if PATCHED in source:
        return source
    if source.count(ANCHOR) != 1:
        raise RuntimeError("dialog.py 的 respond_to_waiting 和預期不同（只支援 sglang-omni 0.2.0），不套用補丁")
    return source.replace(ANCHOR, PATCHED)


def main() -> None:
    path = pathlib.Path(installed_dialog_path())
    source = path.read_text(encoding="utf-8")
    patched = patch_source(source)
    if patched != source:
        path.write_text(patched, encoding="utf-8")
        print(f"patch_dialog: 已套用 {path}")
    else:
        print(f"patch_dialog: 已經套用過 {path}")


if __name__ == "__main__":
    main()
