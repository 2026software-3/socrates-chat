import type { Lang } from '@/i18n/define'

/** 依目前語言格式化日期；無法解析時回傳空字串。 */
export function formatDate(iso: string | null, lang: Lang): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  return new Intl.DateTimeFormat(lang, { dateStyle: 'medium' }).format(d)
}
