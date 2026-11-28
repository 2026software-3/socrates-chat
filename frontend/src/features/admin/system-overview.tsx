import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { ArrowRight, Clock, FileWarning, KeyRound, Lock, Sparkles, type LucideIcon } from 'lucide-react'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { Meta } from '@/components/vedute/meta'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import {
  AdminCard,
  AdminErrorAlert,
  AdminStatusBadge,
  PeriodSelector,
  type AdminTone,
} from '@/features/admin/admin-ui'
import {
  CLASS_LABEL,
  FUNCTION_LABEL,
  KIND_LABEL,
  type AiStatus,
  type DiagnosticEvent,
  type Period,
  type SystemStatus,
} from '@/features/admin/diagnostics-model'
import { HealthItem, healthOf, useBasicHealth } from '@/features/admin/basic-health'
import { ServiceHealth } from '@/features/admin/service-health'
import { useI18n, useT, type MessageKey } from '@/i18n'
import { useFetch } from '@/lib/use-fetch'

const PERIOD_HOURS: Record<Period, number> = { '24h': 24, '7d': 168, '30d': 720 }

const AI_LABEL: Record<AiStatus, MessageKey> = {
  no_data: 'admin.overview.ai.no_data',
  ok: 'admin.overview.ai.ok',
  has_failures: 'admin.overview.ai.has_failures',
}

const AI_TONE: Record<AiStatus, AdminTone> = {
  no_data: 'inactive',
  ok: 'active',
  has_failures: 'error',
}

/** 系統概況：服務與資料庫狀態、AI 狀態與失敗次數、登入失敗、帳號鎖定與近期錯誤（S-09.3）。 */
export function SystemOverview() {
  const t = useT()
  const [period, setPeriod] = useState<Period>('24h')
  const status = useFetch<SystemStatus>(`/api/admin/status?period=${period}`)
  const failed = status.error !== undefined && status.data === undefined
  const basic = useBasicHealth(failed)

  return (
    <section aria-labelledby="overview-heading" className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-faint pb-3">
        <h2 id="overview-heading" className="type-h2 text-ink">
          {t('admin.overview.title')}
        </h2>
        <PeriodSelector value={period} onChange={setPeriod} />
      </div>

      <ServiceHealth period={period} />

      {status.loading && !status.data && !failed ? <OverviewSkeleton /> : null}

      {failed ? (
        <div className="space-y-3">
          <AdminErrorAlert error={status.error} onRetry={status.reload} />
          {basic ? (
            <AdminCard className="p-5">
              <p className="type-note text-ink-soft">{t('admin.overview.fallbackNote')}</p>
              <div className="mt-3 flex flex-wrap gap-6">
                <HealthItem label={t('admin.overview.service')} state={basic.service} />
                <HealthItem label={t('admin.overview.database')} state={basic.database} />
              </div>
            </AdminCard>
          ) : null}
        </div>
      ) : null}

      {status.data ? <Overview data={status.data} period={period} /> : null}
    </section>
  )
}

function OverviewSkeleton() {
  const t = useT()
  return (
    <SkeletonRegion loadingLabel={t('common.loading')} className="space-y-4">
      <div className="rounded-sm border border-ink-faint bg-surface p-4">
        <div className="flex flex-wrap gap-6">
          <Skeleton className="h-5 w-24" />
          <Skeleton className="h-5 w-24" />
          <Skeleton className="h-5 w-28" />
        </div>
      </div>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }, (_, i) => (
          <div key={i} className="space-y-3 rounded-sm border border-ink-faint bg-surface p-5">
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-9 w-16" />
            <Skeleton className="h-4 w-3/4" />
          </div>
        ))}
      </div>
    </SkeletonRegion>
  )
}

function Overview({ data, period }: { data: SystemStatus; period: Period }) {
  const t = useT()
  const stats = data.statistics
  // 統計卡片連到診斷頁，並帶入同一段期間
  const since = useMemo(
    () => new Date(Date.now() - PERIOD_HOURS[period] * 3_600_000).toISOString(),
    [period],
  )
  const link = (extra: Record<string, string>) => {
    const params = new URLSearchParams({ from: since, ...extra })
    return `/admin/diagnostics?${params.toString()}`
  }

  return (
    <div className="space-y-5">
      <AdminCard className="px-5 py-4">
        <div className="flex flex-wrap items-center gap-x-8 gap-y-3">
          <HealthItem label={t('admin.overview.service')} state={healthOf(data.service)} />
          <HealthItem label={t('admin.overview.database')} state={healthOf(data.database)} />
          {stats ? (
            <div className="flex items-center gap-2">
              <span className="type-note text-ink-soft">{t('admin.overview.ai')}</span>
              <AdminStatusBadge tone={AI_TONE[stats.ai_status]}>{t(AI_LABEL[stats.ai_status])}</AdminStatusBadge>
            </div>
          ) : null}
        </div>
      </AdminCard>

      {!data.diagnostics_available || !stats ? (
        <Alert kind="info">{t('admin.overview.unavailable')}</Alert>
      ) : (
        <>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <StatCard
              icon={Sparkles}
              iconClass="text-caeruleum"
              title={t('admin.overview.aiFailures')}
              value={stats.ai_failures}
              highlight={stats.ai_failures > 0 ? 'error' : 'none'}
              note={t('admin.overview.aiCalls', { count: stats.ai_calls })}
              to={stats.ai_failures > 0 ? link({ kind: 'ai' }) : undefined}
            />
            <StatCard
              icon={FileWarning}
              title={t('admin.overview.invalidOutputs')}
              value={stats.ai_invalid_outputs}
              highlight={stats.ai_invalid_outputs > 0 ? 'ochra' : 'none'}
              note={t('admin.overview.invalidOutputsNote')}
              to={stats.ai_invalid_outputs > 0 ? link({ kind: 'ai', failure_class: 'invalid_output' }) : undefined}
            />
            <StatCard
              icon={KeyRound}
              title={t('admin.overview.loginFailures')}
              value={stats.login_failures}
              highlight={stats.login_failures > 0 ? 'ochra' : 'none'}
              to={stats.login_failures > 0 ? link({ kind: 'login' }) : undefined}
            />
            <StatCard
              icon={Lock}
              title={t('admin.overview.accountLocks')}
              value={stats.account_locks}
              highlight={stats.account_locks > 0 ? 'error' : 'none'}
              to={stats.account_locks > 0 ? link({ kind: 'account_lock' }) : undefined}
            />
          </div>
          <RecentErrors errors={data.recent_errors} />
        </>
      )}
    </div>
  )
}

function StatCard({
  icon: Icon,
  iconClass = 'text-ink-soft',
  title,
  value,
  highlight = 'none',
  note,
  to,
}: {
  icon: LucideIcon
  iconClass?: string
  title: string
  value: number
  highlight?: 'none' | 'ochra' | 'error'
  note?: string
  to?: string
}) {
  const t = useT()
  const valueClass =
    highlight === 'error' ? 'text-error' : highlight === 'ochra' ? 'text-ochra' : 'text-ink'
  return (
    <AdminCard className="flex flex-col justify-between p-5">
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <Icon className={`size-4 shrink-0 ${iconClass}`} strokeWidth={1.5} aria-hidden="true" />
          <span className="type-note text-ink-soft">{title}</span>
        </div>
        <p className={`type-h1 tabular-nums ${valueClass}`}>{value}</p>
        {note ? <p className="type-note text-ink-soft">{note}</p> : null}
      </div>
      {to ? (
        <div className="mt-4 pt-3 border-t border-ink-faint">
          <Button asChild variant="quiet" className="w-full justify-between">
            <Link to={to} aria-label={`${title} — ${t('admin.overview.viewDetails')}`}>
              <span>{t('admin.overview.viewDetails')}</span>
              <ArrowRight className="size-4" strokeWidth={1.5} aria-hidden="true" />
            </Link>
          </Button>
        </div>
      ) : null}
    </AdminCard>
  )
}

function RecentErrors({ errors }: { errors: DiagnosticEvent[] }) {
  const t = useT()
  const { lang } = useI18n()
  const date = useMemo(
    () => new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'medium' }),
    [lang],
  )
  return (
    <AdminCard className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink-faint bg-paper/40 px-5 py-3.5">
        <h3 className="type-h3 text-ink">{t('admin.overview.recent')}</h3>
        <Button asChild variant="quiet">
          <Link to="/admin/diagnostics">
            <span>{t('admin.overview.viewAll')}</span>
            <ArrowRight className="size-4" strokeWidth={1.5} aria-hidden="true" />
          </Link>
        </Button>
      </div>
      <div className="px-5 py-4">
        {errors.length === 0 ? (
          <p className="type-body text-ink-soft">{t('admin.overview.recentEmpty')}</p>
        ) : (
          <ul className="divide-y divide-ink-faint">
            {errors.map((e, i) => (
              <li
                key={`${e.request_id}-${i}`}
                className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 py-3 first:pt-0 last:pb-0"
              >
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
                  <Meta
                    icon={Clock}
                    variant="bibliographic"
                    value={<time dateTime={e.occurred_at}>{date.format(new Date(e.occurred_at))}</time>}
                  />
                  <span className="type-body-strong text-ink">{t(KIND_LABEL[e.kind])}</span>
                  <AdminStatusBadge tone={e.failure_class === 'invalid_output' ? 'draft' : 'error'}>
                    {t(CLASS_LABEL[e.failure_class])}
                  </AdminStatusBadge>
                  <span className="type-body text-ink-soft">
                    {FUNCTION_LABEL[e.function] ? t(FUNCTION_LABEL[e.function]) : e.function}
                  </span>
                </div>
                <Button asChild variant="link" className="px-0 whitespace-normal break-all text-left">
                  <Link to={`/admin/diagnostics?request_id=${encodeURIComponent(e.request_id)}`}>
                    {e.request_id}
                  </Link>
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </AdminCard>
  )
}
