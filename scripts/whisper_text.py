"""Whisper 輸出的文字整理：中文統一成臺灣繁體。

模型對同一段音訊有時給繁體、有時給簡體，甚至混用；學生會在畫面上直接看到，也會存進對話紀錄。
"""
from opencc import OpenCC

_to_traditional = OpenCC("s2tw")


def normalize(text: str, language: str) -> str:
    return _to_traditional.convert(text) if language == "zh" else text
