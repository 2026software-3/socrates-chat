import { Link } from 'react-router'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
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

const OVERALL_VARIANT: Record<Overall, 'secondary' | 'outline' | 'destructive'> = {
  healthy: 'secondary',
  degraded: 'outline',
  unhealthy: 'destructive',
}

function Item({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <dt className="text-muted-foreground text-sm">{label}</dt>
      <dd className="text-sm font-medium">{children}</dd>
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
      <h3 id="service-health-heading" className="text-lg font-medium">
        {t('admin.health.title')}
      </h3>
      {health.loading && !health.data && !failed ? <LoadingBlock /> : null}
      {failed ? (
        <>
          <ErrorBlock error={health.error} onRetry={health.reload} />
          {basic ? (
            <Card>
              <CardHeader>
                <p className="text-muted-foreground text-sm">{t('admin.health.fallbackNote')}</p>
              </CardHeader>
              <CardContent className="flex flex-wrap gap-6">
                <HealthItem label={t('admin.overview.service')} state={basic.service} />
                <HealthItem label={t('admin.overview.database')} state={basic.database} />
              </CardContent>
            </Card>
          ) : null}
        </>
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

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2">
        <CardTitle className="text-base">{t('admin.health.title')}</CardTitle>
        <Badge variant={OVERALL_VARIANT[data.status]} data-testid="overall-status">
          {t(OVERALL_LABEL[data.status])}
        </Badge>
      </CardHeader>
      <CardContent className="space-y-4">
        <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <Item label={t('admin.health.database')}>
            {data.database.status === 'healthy' ? t('admin.overview.state.ok') : t('admin.overview.state.unavailable')}
            {data.database.response_time_ms !== null ? (
              <span className="text-muted-foreground block text-xs font-normal">
                {t('admin.health.dbTime', { ms: data.database.response_time_ms })}
              </span>
            ) : null}
          </Item>
          <Item label={t('admin.health.services')}>
            {services.total === 0 ? (
              t('admin.health.servicesNone')
            ) : (
              <span>{t('admin.health.servicesSummary', { total: services.total, healthy: services.healthy })}</span>
            )}{' '}
            <Link to="/admin/services" className="underline">
              {t('admin.health.servicesLink')}
            </Link>
          </Item>
          <Item label={t('admin.health.pool')}>
            {t('admin.health.poolValue', {
              used: Math.max(0, data.pool.size - data.pool.idle),
              idle: data.pool.idle,
              max: data.pool.max,
            })}
          </Item>
          <Item label={t('admin.health.process')}>
            {t('admin.health.uptime', { duration: formatDuration(t, data.process.uptime_seconds) })}
            <span className="text-muted-foreground block text-xs font-normal">
              {t('admin.health.version', { version: data.process.version })}
            </span>
          </Item>
          <Item label={t('admin.health.maintenance')}>
            {data.maintenance ? t('admin.health.maintenance.on') : t('admin.health.maintenance.off')}
          </Item>
          <Item label={t('admin.health.cleanup')}>
            {data.cleanup.last_success_at ? (
              <time dateTime={data.cleanup.last_success_at}>{date.format(new Date(data.cleanup.last_success_at))}</time>
            ) : (
              t('admin.health.cleanupNever')
            )}
          </Item>
        </dl>

        {!data.stats_available ? (
          <Alert role="status">
            <AlertDescription>{t('admin.health.statsUnavailable')}</AlertDescription>
          </Alert>
        ) : (
          <dl className="grid gap-4 sm:grid-cols-3">
            <Item label={t('admin.health.requests')}>{requests ? number.format(requests.total) : t('admin.health.noData')}</Item>
            <Item label={t('admin.health.errorRatio')}>
              {requests ? percent.format(requests.error_5xx_ratio) : t('admin.health.noData')}
            </Item>
            <Item label={t('admin.health.p95')}>
              {requests ? t('admin.health.p95Value', { ms: number.format(requests.p95_ms) }) : t('admin.health.noData')}
            </Item>
          </dl>
        )}
      </CardContent>
    </Card>
  )
}
