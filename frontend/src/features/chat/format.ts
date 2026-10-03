import { useMemo } from 'react'
import type { Lang } from '@/i18n/define'

/** 依目前語言格式化日期（Intl）。 */
export function useDateFormat(lang: Lang): (iso: string) => string {
  return useMemo(() => {
    const fmt = new Intl.DateTimeFormat(lang, { dateStyle: 'medium' })
    return (iso: string) => {
      const d = new Date(iso)
      return Number.isNaN(d.getTime()) ? '' : fmt.format(d)
    }
  }, [lang])
}
