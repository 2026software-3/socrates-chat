/// <reference types="node" />

import { describe, expect, it } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve } from 'node:path'
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

  it('has a translation for every fixed backend API error code', () => {
    const backend = resolve(process.cwd(), '../backend/src')
    const files = readdirSync(backend, { recursive: true })
      .filter((file): file is string => typeof file === 'string' && file.endsWith('.rs'))
      .map((file) => readFileSync(resolve(backend, file), 'utf8'))
      .join('\n')
    const codes = new Set<string>()
    const patterns = [
      /ApiError::(?:bad_request|conflict|forbidden)\(\s*"([a-z][a-z0-9_]+)"/g,
      /(?:ApiError|Self)::new\(\s*[^,]+,\s*"([a-z][a-z0-9_]+)"/g,
      /StatusCode::[A-Z_]+\s*=>\s*\("([a-z][a-z0-9_]+)"/g,
    ]
    for (const pattern of patterns) {
      for (const match of files.matchAll(pattern)) codes.add(match[1])
    }

    for (const code of codes) {
      for (const lang of LANGS) {
        expect(dictionaries[lang][`error.${code}`], `${lang}:error.${code}`).toBeTruthy()
      }
    }
  })
})
