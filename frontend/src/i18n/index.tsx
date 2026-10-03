import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { adminMessages } from '@/features/admin/messages'
import { authMessages } from '@/features/auth/messages'
import { chatMessages } from '@/features/chat/messages'
import { studentMessages } from '@/features/student/messages'
import { teacherMessages } from '@/features/teacher/messages'
import { common } from '@/i18n/common.messages'
import { DEFAULT_LANG, LANGS, pickLang, type Lang } from '@/i18n/define'
import { isApiError } from '@/lib/api'

// 新增功能時：在該功能目錄建立 messages.ts，再加進這裡。
const bundles = [common, authMessages, studentMessages, chatMessages, teacherMessages, adminMessages] as const

type KeysOf<B> = B extends { 'zh-TW': infer M } ? keyof M : never
export type MessageKey = KeysOf<(typeof bundles)[number]>

function merge(lang: Lang): Record<string, string> {
  return Object.assign({}, ...bundles.map((b) => b[lang]))
}

export const dictionaries: Record<Lang, Record<string, string>> = {
  'zh-TW': merge('zh-TW'),
  en: merge('en'),
  es: merge('es'),
}

export type TFunction = (key: MessageKey, vars?: Record<string, string | number>) => string

export function translate(lang: Lang, key: MessageKey, vars?: Record<string, string | number>): string {
  const text = dictionaries[lang][key] ?? dictionaries[DEFAULT_LANG][key] ?? key
  return vars ? text.replace(/\{(\w+)\}/g, (m, name: string) => String(vars[name] ?? m)) : text
}

/** 後端錯誤 → 已翻譯的訊息（後端只回 code，S-12.1）。 */
export function errorText(t: TFunction, err: unknown): string {
  if (!isApiError(err)) return t('error.unknown')
  const key = `error.${err.code}` as MessageKey
  return dictionaries['zh-TW'][key] ? t(key) : t('error.unknown')
}

const STORAGE_KEY = 'lang'

function initialLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved && (LANGS as readonly string[]).includes(saved)) return saved as Lang
  } catch {
    // localStorage 不可用（隱私模式等）
  }
  return pickLang(typeof navigator === 'undefined' ? [] : navigator.languages ?? [navigator.language])
}

type I18nValue = { lang: Lang; setLang: (l: Lang) => void; t: TFunction }
const I18nContext = createContext<I18nValue | null>(null)

export function I18nProvider({ children, initial }: { children: ReactNode; initial?: Lang }) {
  const [lang, setLangState] = useState<Lang>(initial ?? initialLang())

  useEffect(() => {
    document.documentElement.lang = lang
  }, [lang])

  const setLang = useCallback((l: Lang) => {
    setLangState(l)
    try {
      localStorage.setItem(STORAGE_KEY, l)
    } catch {
      // 忽略
    }
  }, [])

  const value = useMemo<I18nValue>(
    () => ({ lang, setLang, t: (key, vars) => translate(lang, key, vars) }),
    [lang, setLang],
  )
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

export function useI18n(): I18nValue {
  const ctx = useContext(I18nContext)
  if (!ctx) throw new Error('useI18n must be used inside <I18nProvider>')
  return ctx
}

export function useT(): TFunction {
  return useI18n().t
}
