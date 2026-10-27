import { useState } from 'react'
import { toast } from 'sonner'
import { rememberLoginPick, useAuth } from '@/auth/auth-context'
import { LANGS, LANG_LABELS, type Lang } from '@/i18n/define'
import { errorText, useI18n } from '@/i18n'
import { api } from '@/lib/api'
import type { Me } from '@/lib/types'

export function LangSwitcher() {
  const { lang, setLang, t } = useI18n()
  const auth = useAuth()
  const [saving, setSaving] = useState(false)

  async function change(next: Lang) {
    const previous = lang
    setLang(next)
    if (auth.status !== 'authed') {
      rememberLoginPick(next)
      return
    }
    setSaving(true)
    try {
      const me = await api.patch<Me>('/api/me', { language: next })
      auth.update(me)
    } catch (error) {
      setLang(previous)
      toast.error(errorText(t, error))
    } finally {
      setSaving(false)
    }
  }
  return (
    <label className="flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{t('nav.language')}</span>
      <select
        className="border-input bg-background h-8 rounded-md border px-2 text-sm"
        value={lang}
        disabled={saving}
        onChange={(e) => void change(e.target.value as Lang)}
      >
        {LANGS.map((l) => (
          <option key={l} value={l}>
            {LANG_LABELS[l]}
          </option>
        ))}
      </select>
    </label>
  )
}
