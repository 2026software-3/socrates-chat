import type { ReactNode } from 'react'
import { RotateCw } from 'lucide-react'
import { cn } from 'cn'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import { StatusChip } from '@/components/vedute/status-chip'
import { PERIODS, type Period } from '@/features/admin/diagnostics-model'
import { errorText, useT, type MessageKey } from '@/i18n'
import { isApiError } from '@/lib/api'

const PERIOD_LABEL: Record<Period, MessageKey> = {
  '24h': 'admin.overview.period.24h',
  '7d': 'admin.overview.period.7d',
  '30d': 'admin.overview.period.30d',
}

export type AdminTone = 'active' | 'draft' | 'inactive' | 'error'

/**
 * 管理者頁面的狀態標籤：正常／啟用用銅綠、降級／草稿用赭金、未確認／關閉用淡墨、異常／失敗用錯誤色（DESIGN.md 狀態標籤）。
 */
export function AdminStatusBadge({
  tone,
  children,
  className,
  role,
  'aria-label': ariaLabel,
  'data-testid': testId,
}: {
  tone: AdminTone
  children: string
  className?: string
  role?: 'status'
  'aria-label'?: string
  'data-testid'?: string
}) {
  if (tone !== 'error' && !role && !ariaLabel && !testId && !className) {
    return <StatusChip tone={tone}>{children}</StatusChip>
  }
  const palette: Record<AdminTone, string> = {
    active: 'border-aerugo bg-aerugo-wash text-aerugo',
    draft: 'border-ochra bg-ochra-wash text-ochra',
    inactive: 'border-ink-faint bg-transparent text-ink-soft',
    error: 'border-error bg-chalk-wash text-error',
  }
  return (
    <span
      role={role}
      aria-label={ariaLabel}
      data-testid={testId}
      className={cn(
        'vedute-status-chip inline-flex shrink-0 items-center whitespace-nowrap border',
        palette[tone],
        className,
      )}
    >
      {children}
    </span>
  )
}

/** Vedute 白底墨框卡片（The Pressed Not Lifted Rule：無投影、1px 墨框、2px 圓角）。 */
export function AdminCard({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div
      data-slot="card"
      className={cn('rounded-sm border border-ink bg-surface text-ink', className)}
    >
      {children}
    </div>
  )
}

/** 管理者頁面的讀取錯誤列（DESIGN.md Alerts：左側錯誤線、附 request ID 與可重試按鈕）。 */
export function AdminErrorAlert({
  error,
  onRetry,
}: {
  error: unknown
  onRetry?: () => void
}) {
  const t = useT()
  return (
    <Alert kind="error">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <p className="type-body-strong text-error">{errorText(t, error)}</p>
          {isApiError(error) && error.requestId ? (
            <p className="type-note break-all text-ink-soft">{t('common.requestId', { id: error.requestId })}</p>
          ) : null}
          {isApiError(error) && error.traceparent ? (
            <p className="type-note break-all text-ink-soft">
              {t('admin.metrics.traceparent', { id: error.traceparent })}
            </p>
          ) : null}
        </div>
        {onRetry ? (
          <Button type="button" variant="quiet" onClick={onRetry}>
            <RotateCw className="size-4" strokeWidth={1.5} aria-hidden="true" />
            {t('common.retry')}
          </Button>
        ) : null}
      </div>
    </Alert>
  )
}

/** 統計期間切換（24 小時／7 天／30 天）：44 高框線按鈕，選中為墨底紙字（DESIGN.md）。 */
export function PeriodSelector({
  value,
  onChange,
}: {
  value: Period
  onChange: (period: Period) => void
}) {
  const t = useT()
  return (
    <div role="group" aria-label={t('admin.overview.period')} className="flex flex-wrap gap-2">
      {PERIODS.map((p) => (
        <Button
          key={p}
          type="button"
          variant={p === value ? 'primary' : 'quiet'}
          aria-pressed={p === value}
          onClick={() => onChange(p)}
        >
          {t(PERIOD_LABEL[p])}
        </Button>
      ))}
    </div>
  )
}

/** 空資料狀態（DESIGN.md：細線框、紙色微底、正文文字）。 */
export function AdminEmptyState({ children }: { children: ReactNode }) {
  return (
    <p className="type-body rounded-sm border border-dashed border-ink-faint bg-paper/30 p-6 text-center text-ink-soft">
      {children}
    </p>
  )
}

/** 卡片型區塊的 Vedute Skeleton 載入佔位（DESIGN.md 載入中一律用 skeleton）。 */
export function AdminSectionSkeleton({ rows = 3 }: { rows?: number }) {
  const t = useT()
  return (
    <SkeletonRegion loadingLabel={t('common.loading')} className="space-y-4">
      <div className="rounded-sm border border-ink-faint bg-surface p-5">
        <div className="flex items-center justify-between gap-4 border-b border-ink-faint pb-3">
          <Skeleton className="h-6 w-36" />
          <Skeleton className="h-6 w-16" />
        </div>
        <div className="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: rows }, (_, i) => (
            <div key={i} className="space-y-2">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-6 w-3/4" />
            </div>
          ))}
        </div>
      </div>
    </SkeletonRegion>
  )
}
