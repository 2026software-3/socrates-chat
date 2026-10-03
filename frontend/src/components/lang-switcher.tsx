import { LANGS, LANG_LABELS, type Lang } from '@/i18n/define'
import { useI18n } from '@/i18n'

export function LangSwitcher() {
  const { lang, setLang, t } = useI18n()
  return (
    <label className="flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{t('nav.language')}</span>
      <select
        className="border-input bg-background h-8 rounded-md border px-2 text-sm"
        value={lang}
        onChange={(e) => setLang(e.target.value as Lang)}
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
