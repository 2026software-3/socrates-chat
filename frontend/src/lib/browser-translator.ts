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
 * Uses the browser's on-device Translator API when the requested pair is supported.
 * Returns `null` when the API or pair is unavailable, its language pack cannot be downloaded,
 * or translation fails, allowing the caller to use its server-side fallback.
 */
export async function translateInBrowser(source: Lang, target: Lang, texts: string[]): Promise<string[] | null> {
  const factory = (globalThis as typeof globalThis & { Translator?: BrowserTranslatorFactory }).Translator
  if (!factory) return null
  const options: LanguagePair = {
    sourceLanguage: browserLanguage(source),
    targetLanguage: browserLanguage(target),
  }
  try {
    if ((await factory.availability(options)) === 'unavailable') return null
    const translator = await factory.create(options)
    try {
      return await Promise.all(texts.map((text) => translator.translate(text)))
    } finally {
      translator.destroy?.()
    }
  } catch {
    return null
  }
}
