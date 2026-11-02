"""Whisper 輸出的文字整理：中文統一成臺灣繁體，並去掉靜音或雜訊時幻想出來的內容。

模型對同一段音訊有時給繁體、有時給簡體，甚至混用；學生會在畫面上直接看到，也會存進對話紀錄。
模型的訓練資料有大量影片字幕，遇到雜訊會吐出「字幕 by」「請訂閱」這類片尾套語。
"""
import re

from opencc import OpenCC

_to_traditional = OpenCC("s2tw")

# 只比對片尾套語的完整形式：學生真的談到訂閱或字幕時不能被刪
_CREDITS = re.compile(
    r"請不吝[點按指賜][^\s。！？]*"
    r"|(?:點贊|點讚|按讚)[、，\s]*(?:訂閱|分享|轉發)[^\s。！？]*"
    r"|字幕\s*(?:by|BY|By|由|提供|製作)[^\s。！？]*"
)
# Whisper 自己判斷靜音的門檻是 0.6，平均對數機率低於 -1 時結果通常不可信
NO_SPEECH_PROB = 0.6
MIN_AVG_LOGPROB = -1.0


def normalize(text: str, language: str) -> str:
    return _to_traditional.convert(text) if language == "zh" else text


def keep_segment(no_speech_prob: float, avg_logprob: float) -> bool:
    """這一段看起來是真的語音。"""
    return no_speech_prob < NO_SPEECH_PROB and avg_logprob >= MIN_AVG_LOGPROB


def strip_credits(text: str) -> str:
    """去掉影片片尾套語，留下其他內容。"""
    return re.sub(r"\s+", " ", _CREDITS.sub("", text)).strip()
