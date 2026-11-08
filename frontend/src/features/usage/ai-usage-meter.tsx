import { useEffect } from 'react'
import { useT, type TFunction } from '@/i18n'
import { AI_USAGE_STALE_EVENT } from '@/lib/ai-usage-events'
import type { AiUsage } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'
import { cn } from '@/lib/utils'

/** 收到「用量可能變了」後等一下再載入：一次操作常連續觸發好幾個通知（例如串流結束加上寫入請求）。 */
const REFRESH_DELAY_MS = 300
/** 已用到這個比例就提醒「即將用完」。 */
const WARN_RATIO = 0.8

/** 距離釋出下一個額度還有多久：不到一小時顯示分鐘（進位），其餘顯示小時（四捨五入）。 */
function resetText(t: TFunction, secs: number): string {
  if (secs < 3600) return t('usage.resetsMinutes', { n: Math.max(1, Math.ceil(secs / 60)) })
  return t('usage.resetsHours', { n: Math.round(secs / 3600) })
}

function statusText(t: TFunction, u: AiUsage): string | undefined {
  if (u.used >= u.limit) return t('usage.exhausted')
  if (u.used >= u.limit * WARN_RATIO) return t('usage.nearLimit')
  return undefined
}

/** 外框上的 AI 用量：已用／上限的進度條；快用完時提醒，用完時說明多久後會有額度。載入失敗就不顯示。 */
export function AiUsageMeter() {
  const t = useT()
  const { data, reload } = useFetch<AiUsage>('/api/me/usage')

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    const onStale = () => {
      clearTimeout(timer)
      timer = setTimeout(reload, REFRESH_DELAY_MS)
    }
    window.addEventListener(AI_USAGE_STALE_EVENT, onStale)
    return () => {
      window.removeEventListener(AI_USAGE_STALE_EVENT, onStale)
      clearTimeout(timer)
    }
  }, [reload])

  if (!data) return null
  const ratio = data.limit > 0 ? Math.min(1, data.used / data.limit) : 1
  const status = statusText(t, data)
  const exhausted = data.used >= data.limit
  return (
    <div className="flex w-full flex-col gap-1">
      <div className="flex items-baseline justify-between gap-2 text-xs">
        <span className="text-muted-foreground">{t('usage.label')}</span>
        <span className="tabular-nums">
          {data.used} / {data.limit}
        </span>
      </div>
      <div
        role="meter"
        aria-label={t('usage.label')}
        aria-valuemin={0}
        aria-valuemax={data.limit}
        aria-valuenow={data.used}
        className="bg-border h-1.5 w-full overflow-hidden rounded-full"
      >
        <div
          className={cn('h-full rounded-full', exhausted ? 'bg-destructive' : status ? 'bg-amber-500' : 'bg-primary')}
          style={{ width: `${ratio * 100}%` }}
        />
      </div>
      {status && (
        <p className={cn('text-xs', exhausted && 'text-destructive font-medium')}>
          {status}
          {exhausted && data.resets_in_secs !== null && <span className="text-muted-foreground block font-normal">{resetText(t, data.resets_in_secs)}</span>}
        </p>
      )}
    </div>
  )
}
