"""whisper_text 的單元測試（中文一律輸出臺灣繁體）。執行：python3 scripts/test_whisper_text.py"""
import unittest

from whisper_text import normalize


class NormalizeTest(unittest.TestCase):
    def test_simplified_chinese_becomes_traditional(self):
        self.assertEqual(
            normalize("这样的行为和为了得到奖励而做的行为有什么不同?", "zh"),
            "這樣的行為和為了得到獎勵而做的行為有什麼不同?",
        )

    def test_mixed_output_becomes_consistent(self):
        self.assertEqual(normalize("这樣的行為", "zh"), "這樣的行為")

    def test_traditional_chinese_is_unchanged(self):
        for text in ["這樣的行為和為了得到獎勵而做的行為有什麼不同?", "蘇格拉底的產婆術", "著作與後來的皇后", ""]:
            self.assertEqual(normalize(text, "zh"), text)

    def test_other_languages_are_unchanged(self):
        for text, language in [("Hello world", "en"), ("¿Por qué?", "es"), ("这样", "ja")]:
            self.assertEqual(normalize(text, language), text)


if __name__ == "__main__":
    unittest.main()
