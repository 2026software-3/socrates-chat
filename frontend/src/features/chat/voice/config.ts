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

/** 開口偵測的分析幀長度。 */
export const BARGE_IN_FRAME_MS = 20

/** 播放中，AI 的聲音回灌到麥克風的風險（S-05.5）。 */
export type EchoRisk = 'low' | 'high'

/**
 * 開口偵測的門檻（S-05.5）。數值來自合成訊號的測試，尚未用真實裝置與喇叭校正；
 * 風險高時要求更明顯、更久的開口，寧可漏判也不要 AI 打斷自己。
 */
export const BARGE_IN_DETECTOR: Record<EchoRisk, { calibrationMs: number; marginDb: number; minVoicedMs: number }> = {
  low: { calibrationMs: 600, marginDb: 8, minVoicedMs: 240 },
  high: { calibrationMs: 1000, marginDb: 14, minVoicedMs: 360 },
}

/** 語音偏好存在這個裝置的 localStorage（只是個人偏好，不影響後端）。 */
const AUTO_SEND_KEY = 'voice.autoSend'
const BARGE_IN_KEY = 'voice.bargeIn'

function loadFlag(key: string, fallback: boolean): boolean {
  try {
    const stored = localStorage.getItem(key)
    return stored === null ? fallback : stored === 'true'
  } catch {
    return fallback
  }
}

function saveFlag(key: string, on: boolean) {
  try {
    localStorage.setItem(key, String(on))
  } catch {
    // 忽略
  }
}

export const loadAutoSend = () => loadFlag(AUTO_SEND_KEY, true)
export const saveAutoSend = (on: boolean) => saveFlag(AUTO_SEND_KEY, on)
/** 預設關閉：iOS 實機驗證通過之前，播放時不主動開麥克風（S-05.5）。 */
export const loadBargeIn = () => loadFlag(BARGE_IN_KEY, false)
export const saveBargeIn = (on: boolean) => saveFlag(BARGE_IN_KEY, on)
