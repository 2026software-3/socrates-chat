import type { Lang } from '@/i18n/define'

type Availability = 'unavailable' | 'downloadable' | 'downloading' | 'available'

type BrowserTranslator = {
  translate: (text: string) => Promise<string>
  destroy?: () => void
}

type BrowserTranslatorFactory = {
  availability: (options: LanguagePair) => Promise<Availability>
  create: (options: LanguagePair) => Promise<BrowserTranslator>
}

type LanguagePair = {
  sourceLanguage: string
  targetLanguage: string
}

function browserLanguage(lang: Lang): string {
  return lang === 'es' ? 'es-ES' : lang
}

/**
 * Uses the browser's on-device Translator API when the requested pair is installed.
 * Returns `null` when the API is missing, the pair is unavailable or its language pack is not
 * installed yet (`create()` would wait on a large download and needs a live user gesture), or
 * translation fails, allowing the caller to use its server-side fallback.
 * Blank texts are returned as `''` without being sent to the translator.
 */
export async function translateInBrowser(source: Lang, target: Lang, texts: string[]): Promise<string[] | null> {
  const factory = (globalThis as typeof globalThis & { Translator?: BrowserTranslatorFactory }).Translator
  if (!factory) return null
  if (texts.every((text) => !text.trim())) return texts.map(() => '')
  const options: LanguagePair = {
    sourceLanguage: browserLanguage(source),
    targetLanguage: browserLanguage(target),
  }
  try {
    if ((await factory.availability(options)) !== 'available') return null
    const translator = await factory.create(options)
    try {
      return await Promise.all(texts.map((text) => (text.trim() ? translator.translate(text) : '')))
    } finally {
      translator.destroy?.()
    }
  } catch {
    return null
  }
}
