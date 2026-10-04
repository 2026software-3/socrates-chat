/** 語音設定（S-05.2、S-05.3、S-12.1）。 */

/** 對話語言（S-12.1）→ 辨識與合成使用的語音語言。 */
const SPEECH_LANG: Record<string, string> = {
  'zh-TW': 'zh-TW',
  en: 'en-US',
  es: 'es-ES',
}

export function speechLang(conversationLang: string): string {
  return SPEECH_LANG[conversationLang] ?? 'zh-TW'
}

/** 說話中靜音多久後進入「即將送出」倒數。 */
export const SILENCE_MS = 3000
/** 「即將送出」倒數長度。 */
export const COUNTDOWN_MS = 3000
/** 單次發言上限。 */
export const MAX_UTTERANCE_MS = 60_000
/** 開口前不計時，超過這段時間只顯示提示。 */
export const HINT_AFTER_MS = 30_000

/** 自動送出開關存在這個裝置的 localStorage（只是個人偏好，不影響後端）。 */
const AUTO_SEND_KEY = 'voice.autoSend'

export function loadAutoSend(): boolean {
  try {
    return localStorage.getItem(AUTO_SEND_KEY) !== 'false'
  } catch {
    return true
  }
}

export function saveAutoSend(on: boolean) {
  try {
    localStorage.setItem(AUTO_SEND_KEY, String(on))
  } catch {
    // 忽略
  }
}
