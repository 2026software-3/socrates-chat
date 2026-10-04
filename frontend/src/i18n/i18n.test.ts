import { describe, expect, it } from 'vitest'
import { dictionaries, errorText, translate } from '@/i18n'
import { LANGS, pickLang } from '@/i18n/define'
import { ApiError } from '@/lib/api'

describe('i18n', () => {
  it('has identical keys in every language (S-12.1)', () => {
    const base = Object.keys(dictionaries['zh-TW']).sort()
    for (const lang of LANGS) {
      expect(Object.keys(dictionaries[lang]).sort(), lang).toEqual(base)
    }
  })

  it('has no empty translations and identical placeholders across languages', () => {
    const placeholders = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join(',')
    for (const [key, zh] of Object.entries(dictionaries['zh-TW'])) {
      for (const lang of LANGS) {
        const text = dictionaries[lang][key]
        expect(text.trim(), `${lang}:${key}`).not.toBe('')
        expect(placeholders(text), `${lang}:${key}`).toBe(placeholders(zh))
      }
    }
  })

  it('picks the first supported browser language, else zh-TW', () => {
    expect(pickLang(['es-MX', 'en'])).toBe('es')
    expect(pickLang(['fr', 'en-US'])).toBe('en')
    expect(pickLang(['fr'])).toBe('zh-TW')
    expect(pickLang([])).toBe('zh-TW')
  })

  it('interpolates variables', () => {
    expect(translate('en', 'home.welcome', { name: 'Ada' })).toBe('Welcome, Ada')
  })

  it('translates backend error codes and falls back for unknown ones', () => {
    const t = (k: Parameters<typeof translate>[1], v?: Record<string, string | number>) => translate('en', k, v)
    expect(errorText(t, new ApiError(403, 'not_enrolled', 'r'))).toBe('Your account has not been enabled yet.')
    expect(errorText(t, new ApiError(500, 'some_new_code', 'r'))).toBe('An unknown error occurred.')
    expect(errorText(t, new Error('x'))).toBe('An unknown error occurred.')
  })
})
