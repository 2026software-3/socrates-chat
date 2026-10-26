import type { Lang } from '@/i18n/define'

const dateFormats = new Map<Lang, Intl.DateTimeFormat>()

/** 依目前語言格式化日期；無法解析時回傳空字串。每種語言只建立一次 Intl.DateTimeFormat。 */
export function formatDate(iso: string | null, lang: Lang): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  let fmt = dateFormats.get(lang)
  if (!fmt) {
    fmt = new Intl.DateTimeFormat(lang, { dateStyle: 'medium' })
    dateFormats.set(lang, fmt)
  }
  return fmt.format(d)
}
