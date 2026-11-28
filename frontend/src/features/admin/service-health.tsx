import type { ReactNode } from 'react'
import { Link } from 'react-router'
import {
  Activity,
  AlertTriangle,
  ArrowRight,
  CalendarClock,
  Clock,
  Database,
  Gauge,
  Layers,
  Server,
  ShieldAlert,
  type LucideIcon,
} from 'lucide-react'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { StackBar } from '@/components/vedute/stack-bar'
import {
  AdminCard,
  AdminErrorAlert,
  AdminSectionSkeleton,
  AdminStatusBadge,
  type AdminTone,
} from '@/features/admin/admin-ui'
import { HealthItem, useBasicHealth } from '@/features/admin/basic-health'
import type { Period } from '@/features/admin/diagnostics-model'
import { formatDuration } from '@/features/admin/duration'
import { PERIOD_DAYS, type HealthView, type Overall } from '@/features/admin/service-health-model'
import { useI18n, useT, type MessageKey } from '@/i18n'
import { useFetch } from '@/lib/use-fetch'

const OVERALL_LABEL: Record<Overall, MessageKey> = {
  healthy: 'admin.health.overall.healthy',
  degraded: 'admin.health.overall.degraded',
  unhealthy: 'admin.health.overall.unhealthy',
}

const OVERALL_TONE: Record<Overall, AdminTone> = {
  healthy: 'active',
  degraded: 'draft',
  unhealthy: 'error',
}

function Item({
  icon: Icon,
  label,
  children,
}: {
  icon: LucideIcon
  label: string
  children: ReactNode
}) {
  return (
    <div className="space-y-1">
      <dt className="type-note flex items-center gap-1.5 text-ink-soft">
        <Icon className="size-4 shrink-0 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
        <span>{label}</span>
      </dt>
      <dd className="type-body-strong text-ink tabular-nums">{children}</dd>
    </div>
  )
}

/** 後台首頁的服務健康度（F-39.4）：整體狀態、資料庫、周邊服務、連線池、程序、維護模式、背景清理與請求統計。 */
export function ServiceHealth({ period }: { period: Period }) {
  const t = useT()
  const health = useFetch<HealthView>(`/api/admin/health?period_days=${PERIOD_DAYS[period]}`)
  const failed = health.error !== undefined && health.data === undefined
  const basic = useBasicHealth(failed)

  return (
    <section aria-labelledby="service-health-heading" className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="service-health-heading" className="type-h3 text-ink">
          {t('admin.health.title')}
        </h3>
        <div className="flex flex-wrap items-center gap-2">
          <Button asChild variant="link">
            <Link to="/admin/metrics">
              {t('admin.health.metricsLink')}
              <ArrowRight className="size-4" strokeWidth={1.5} aria-hidden="true" />
            </Link>
          </Button>
        </div>
      </div>

      {health.loading && !health.data && !failed ? <AdminSectionSkeleton rows={6} /> : null}

      {failed ? (
        <div className="space-y-3">
          <AdminErrorAlert error={health.error} onRetry={health.reload} />
          {basic ? (
            <AdminCard className="p-5">
              <p className="type-note text-ink-soft">{t('admin.health.fallbackNote')}</p>
              <div className="mt-3 flex flex-wrap gap-6">
                <HealthItem label={t('admin.overview.service')} state={basic.service} />
                <HealthItem label={t('admin.overview.database')} state={basic.database} />
              </div>
            </AdminCard>
          ) : null}
        </div>
      ) : null}

      {health.data ? <HealthCard data={health.data} /> : null}
    </section>
  )
}

function HealthCard({ data }: { data: HealthView }) {
  const t = useT()
  const { lang } = useI18n()
  const date = new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'short' })
  const percent = new Intl.NumberFormat(lang, { style: 'percent', maximumFractionDigits: 2 })
  const number = new Intl.NumberFormat(lang)
  const services = data.services
  const requests = data.requests
  const poolUsed = Math.max(0, data.pool.size - data.pool.idle)
  const poolFree = Math.max(0, data.pool.max - data.pool.size)

  return (
    <AdminCard className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink-faint bg-paper/40 px-5 py-3.5">
        <span data-slot="card-title" className="type-body-strong text-ink">
          {t('admin.health.overall')}
        </span>
        <AdminStatusBadge tone={OVERALL_TONE[data.status]} data-testid="overall-status">
          {t(OVERALL_LABEL[data.status])}
        </AdminStatusBadge>
      </div>

      <div className="space-y-5 p-5">
        <dl className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
          <Item icon={Database} label={t('admin.health.database')}>
            <span className={data.database.status === 'healthy' ? 'text-aerugo' : 'text-error'}>
              {data.database.status === 'healthy' ? t('admin.overview.state.ok') : t('admin.overview.state.unavailable')}
            </span>
            {data.database.response_time_ms !== null ? (
              <span className="type-note block text-ink-soft">
                {t('admin.health.dbTime', { ms: data.database.response_time_ms })}
              </span>
            ) : null}
          </Item>

          <Item icon={Server} label={t('admin.health.services')}>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <span>
                {services.total === 0
                  ? t('admin.health.servicesNone')
                  : t('admin.health.servicesSummary', { total: services.total, healthy: services.healthy })}
              </span>
              <Button asChild variant="link" className="px-0">
                <Link to="/admin/services">{t('admin.health.servicesLink')}</Link>
              </Button>
            </div>
          </Item>

          <Item icon={Layers} label={t('admin.health.pool')}>
            <span>
              {t('admin.health.poolValue', {
                used: poolUsed,
                idle: data.pool.idle,
                max: data.pool.max,
              })}
            </span>
            {data.pool.max > 0 ? (
              <StackBar
                className="mt-2"
                size="sm"
                showLegend={false}
                ariaLabel={t('admin.health.pool')}
                segments={[
                  { label: t('admin.health.pool'), value: poolUsed, tone: 'caeruleum' },
                  { label: t('admin.health.pool'), value: data.pool.idle, tone: 'aerugo' },
                  { label: t('admin.health.pool'), value: poolFree, tone: 'track' },
                ]}
              />
            ) : null}
          </Item>

          <Item icon={Clock} label={t('admin.health.process')}>
            <span>{t('admin.health.uptime', { duration: formatDuration(t, data.process.uptime_seconds) })}</span>
            <span className="type-note block text-ink-soft">
              {t('admin.health.version', { version: data.process.version })}
            </span>
          </Item>

          <Item icon={ShieldAlert} label={t('admin.health.maintenance')}>
            <AdminStatusBadge tone={data.maintenance ? 'draft' : 'inactive'}>
              {data.maintenance ? t('admin.health.maintenance.on') : t('admin.health.maintenance.off')}
            </AdminStatusBadge>
          </Item>

          <Item icon={CalendarClock} label={t('admin.health.cleanup')}>
            {data.cleanup.last_success_at ? (
              <time dateTime={data.cleanup.last_success_at}>{date.format(new Date(data.cleanup.last_success_at))}</time>
            ) : (
              <span className="text-ink-soft">{t('admin.health.cleanupNever')}</span>
            )}
          </Item>
        </dl>

        <div className="border-t border-ink-faint pt-5">
          {!data.stats_available ? (
            <Alert kind="info">{t('admin.health.statsUnavailable')}</Alert>
          ) : (
            <dl className="grid gap-5 sm:grid-cols-3">
              <Item icon={Activity} label={t('admin.health.requests')}>
                {requests ? number.format(requests.total) : t('admin.health.noData')}
              </Item>
              <Item icon={AlertTriangle} label={t('admin.health.errorRatio')}>
                <span
                  className={
                    requests && requests.error_5xx_ratio >= 0.05
                      ? 'text-error'
                      : requests && requests.error_5xx_ratio >= 0.01
                        ? 'text-ochra'
                        : undefined
                  }
                >
                  {requests ? percent.format(requests.error_5xx_ratio) : t('admin.health.noData')}
                </span>
              </Item>
              <Item icon={Gauge} label={t('admin.health.p95')}>
                <span className={requests && requests.p95_ms >= 3000 ? 'text-ochra' : undefined}>
                  {requests ? t('admin.health.p95Value', { ms: number.format(requests.p95_ms) }) : t('admin.health.noData')}
                </span>
              </Item>
            </dl>
          )}
        </div>
      </div>
    </AdminCard>
  )
}
