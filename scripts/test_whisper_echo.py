"""whisper_echo 的單元測試（提示詞過濾與提示詞選擇）。執行：python3 scripts/test_whisper_echo.py"""
import unittest

from whisper_echo import is_prompt_echo, pick_prompt

PROMPT = "蘇格拉底、柏拉圖、產婆術、未經檢視的人生、無知之知、德爾菲神諭、詰問法、美德即知識。"


class PromptEchoTest(unittest.TestCase):
    def test_whole_prompt_is_an_echo(self):
        self.assertTrue(is_prompt_echo(PROMPT, PROMPT))

    def test_recorded_leaks_from_real_speech_are_echoes(self):
        # Common Voice zh-TW 實測時，small／base 在短句上吐出的內容
        for leak in [
            "法、詰問法、詰問法、美德即知識。",
            "蘇格拉底、菲神諭、美德即知識",
            "即知識知識。",
            "詰問法、美德即知識。",
        ]:
            self.assertTrue(is_prompt_echo(leak, PROMPT), leak)

    def test_garbage_that_only_partly_overlaps_is_not_caught(self):
        # 已知限制：只有一小部分是提示詞內的詞，不算「幾乎全是」
        self.assertFalse(is_prompt_echo("免領大派、領域、術品、美德路。", PROMPT))

    def test_real_utterances_are_kept(self):
        for text in [
            "",
            "蘇格拉底",
            "產婆術",
            "未經檢視的人生",
            "蘇格拉底說未經檢視的人生",
            "蘇格拉底說未經檢視的人生不值得活。",
            "我覺得產婆術的意思是老師不直接給答案,而是用提問幫學生自己生出想法。",
            "如果美德即知識,那麼有人明知識壞事卻還去做,是不是代表他其實不知道什麼是善?",
        ]:
            self.assertFalse(is_prompt_echo(text, PROMPT), text)

    def test_no_prompt_never_drops(self):
        self.assertFalse(is_prompt_echo(PROMPT, ""))
        self.assertFalse(is_prompt_echo(PROMPT, None))

    def test_known_false_positive_a_list_made_only_of_terms(self):
        # 已知限制：學生只念出幾個術語並用頓號隔開，會被當成洩漏
        self.assertTrue(is_prompt_echo("蘇格拉底、柏拉圖", PROMPT))


class PickPromptTest(unittest.TestCase):
    def test_no_request_prompt_and_no_default_means_no_prompt(self):
        # 預設不帶提示詞：沒有請求的 prompt、環境變數也沒設，就不要用
        for request in [None, "", "   "]:
            self.assertIsNone(pick_prompt(request, ""), repr(request))

    def test_request_prompt_is_used(self):
        self.assertEqual(pick_prompt("產婆術、詰問法", ""), "產婆術、詰問法")

    def test_request_prompt_wins_over_the_configured_default(self):
        self.assertEqual(pick_prompt("產婆術", "蘇格拉底"), "產婆術")

    def test_configured_default_applies_only_when_the_request_has_none(self):
        self.assertEqual(pick_prompt(None, "蘇格拉底"), "蘇格拉底")
        self.assertEqual(pick_prompt("", "蘇格拉底"), "蘇格拉底")


if __name__ == "__main__":
    unittest.main()
