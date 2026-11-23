import type { TFunction } from '@/i18n'
import { splitDuration } from '@/features/admin/service-health-model'

/** 秒數 → 「3 天 4 小時」「5 分鐘」這類概略說明。 */
export function formatDuration(t: TFunction, seconds: number): string {
  const { days, hours, minutes } = splitDuration(seconds)
  if (days > 0) return hours > 0 ? `${t('admin.duration.days', { n: days })} ${t('admin.duration.hours', { n: hours })}` : t('admin.duration.days', { n: days })
  if (hours > 0) return minutes > 0 ? `${t('admin.duration.hours', { n: hours })} ${t('admin.duration.minutes', { n: minutes })}` : t('admin.duration.hours', { n: hours })
  if (minutes > 0) return t('admin.duration.minutes', { n: minutes })
  return t('admin.duration.lessThanMinute')
}
