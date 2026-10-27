"""initial_prompt 的選擇，以及偵測 Whisper 把它當成辨識結果吐出來的情況（無語音或聲音很短、很不清楚時會發生）。"""
import re

# 輸出中有多少比例的字落在「也出現在提示詞裡的連續片段（至少 2 字）」，達到才算幾乎全是提示詞的內容
COVERAGE = 0.9
MIN_PIECE = 2


def pick_prompt(request_prompt, default_prompt):
    """請求帶的 prompt 優先，其次是設定的預設值；兩者都沒有就不帶（回傳 None）。"""
    return (request_prompt or "").strip() or (default_prompt or "").strip() or None


def _normalize(text):
    return re.sub(r"[\s\W_]", "", text)


def _coverage(text, prompt):
    covered = 0
    i = 0
    while i < len(text):
        longest = 0
        for j in range(len(text), i + MIN_PIECE - 1, -1):
            if text[i:j] in prompt:
                longest = j - i
                break
        if longest:
            covered += longest
            i += longest
        else:
            i += 1
    return covered / len(text)


def _has_repeat(text):
    n = len(text)
    return any(
        text.find(text[i : i + MIN_PIECE], i + MIN_PIECE) != -1 for i in range(n - MIN_PIECE)
    )


def is_prompt_echo(text, prompt):
    """輸出幾乎全是提示詞內的詞，而且有洩漏特徵（含頓號，或有重複片段）時為 True。

    只看涵蓋率會誤殺學生真的說出「蘇格拉底說未經檢視的人生」這類句子，所以另外要求洩漏特徵；
    單獨一個術語（例如只回答「產婆術」）不會被丟掉。
    """
    if not text or not prompt:
        return False
    body, ref = _normalize(text), _normalize(prompt)
    if len(body) < MIN_PIECE or _coverage(body, ref) < COVERAGE:
        return False
    return "、" in text or _has_repeat(body)
