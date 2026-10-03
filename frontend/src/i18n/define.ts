/** 支援語言（S-12.1）：`zh-TW`、`en`、`es`；預設 `zh-TW`。 */
export const LANGS = ['zh-TW', 'en', 'es'] as const
export type Lang = (typeof LANGS)[number]
export const DEFAULT_LANG: Lang = 'zh-TW'

export const LANG_LABELS: Record<Lang, string> = {
  'zh-TW': '繁體中文',
  en: 'English',
  es: 'Español',
}

/**
 * 定義一組翻譯。三種語言的鍵值必須完全一致：缺少或多出任何一個鍵，型別檢查（tsc）就會失敗，
 * 這也是 S-12.1「CI 檢查缺漏」的第一道防線。鍵名請加上功能前綴，例如 `student.available.title`。
 */
export function defineMessages<const K extends string>(m: { [L in Lang]: Record<K, string> }) {
  return m
}

/** 瀏覽器偏好語言 → 支援語言：取第一個支援的，沒有就用預設（S-12.1）。 */
export function pickLang(preferred: readonly string[]): Lang {
  for (const p of preferred) {
    const lower = p.toLowerCase()
    if (lower.startsWith('zh')) return 'zh-TW'
    if (lower.startsWith('en')) return 'en'
    if (lower.startsWith('es')) return 'es'
  }
  return DEFAULT_LANG
}
