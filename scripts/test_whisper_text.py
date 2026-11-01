"""whisper_text 的單元測試（中文一律輸出臺灣繁體）。執行：python3 scripts/test_whisper_text.py"""
import unittest

from whisper_text import keep_segment, normalize, strip_credits


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


class CreditsTest(unittest.TestCase):
    def test_a_video_outro_is_dropped_entirely(self):
        self.assertEqual(strip_credits("請不吝點贊訂閱轉發打賞支持明鏡與點點欄目"), "")

    def test_a_subtitle_credit_is_cut_out_of_real_speech(self):
        self.assertEqual(
            strip_credits("這是你自己決定要不要留下來的 字幕by索蘭婭 把他選擇"),
            "這是你自己決定要不要留下來的 把他選擇",
        )

    def test_real_speech_about_these_topics_stays(self):
        for text in ["我覺得訂閱制不公平", "他不吝嗇自己的時間", "字幕讓我更容易理解", "謝謝你的觀看建議"]:
            self.assertEqual(strip_credits(text), text)


class KeepSegmentTest(unittest.TestCase):
    def test_clear_speech_is_kept(self):
        self.assertTrue(keep_segment(no_speech_prob=0.05, avg_logprob=-0.3))

    def test_a_likely_silent_segment_is_dropped(self):
        self.assertFalse(keep_segment(no_speech_prob=0.7, avg_logprob=-0.3))

    def test_a_very_unsure_segment_is_dropped(self):
        self.assertFalse(keep_segment(no_speech_prob=0.1, avg_logprob=-1.3))


if __name__ == "__main__":
    unittest.main()
