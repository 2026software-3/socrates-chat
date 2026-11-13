import { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  CLASS_LABEL,
  FUNCTION_LABEL,
  KIND_LABEL,
  PERIODS,
  type AiStatus,
  type DiagnosticEvent,
  type Period,
  type SystemStatus,
} from '@/features/admin/diagnostics-model'
import { useI18n, useT, type MessageKey } from '@/i18n'
import { useFetch } from '@/lib/use-fetch'

type Health = 'ok' | 'unavailable' | 'unknown'

const PERIOD_LABEL: Record<Period, MessageKey> = {
  '24h': 'admin.overview.period.24h',
  '7d': 'admin.overview.period.7d',
  '30d': 'admin.overview.period.30d',
}

const PERIOD_HOURS: Record<Period, number> = { '24h': 24, '7d': 168, '30d': 720 }

const AI_LABEL: Record<AiStatus, MessageKey> = {
  no_data: 'admin.overview.ai.no_data',
  ok: 'admin.overview.ai.ok',
  has_failures: 'admin.overview.ai.has_failures',
}

function healthOf(value: string | undefined): Health {
  if (value === 'ok') return 'ok'
  if (value === undefined) return 'unknown'
  return 'unavailable'
}

const HEALTH_LABEL: Record<Health, MessageKey> = {
  ok: 'admin.overview.state.ok',
  unavailable: 'admin.overview.state.unavailable',
  unknown: 'admin.overview.state.unknown',
}

function HealthBadge({ state }: { state: Health }) {
  const t = useT()
  return (
    <Badge variant={state === 'ok' ? 'secondary' : state === 'unavailable' ? 'destructive' : 'outline'}>
      {t(HEALTH_LABEL[state])}
    </Badge>
  )
}

/**
 * 資料庫故障時管理者 session 驗證不了，狀態端點會失敗；這時改讀公開的 `/health` 與 `/ready`，
 * 至少顯示服務與資料庫是否正常，整頁不會失敗（S-09.3）。
 */
function useBasicHealth(enabled: boolean): { service: Health; database: Health } | undefined {
  const [state, setState] = useState<{ service: Health; database: Health }>()
  useEffect(() => {
    if (!enabled) return
    const ctrl = new AbortController()
    const probe = async (path: string): Promise<Health> => {
      try {
        const res = await fetch(path, { signal: ctrl.signal, credentials: 'same-origin' })
        if (res.ok) return 'ok'
        return res.status === 503 ? 'unavailable' : 'unknown'
      } catch {
        return 'unknown'
      }
    }
    void Promise.all([probe('/health'), probe('/ready')]).then(([service, database]) => {
      if (!ctrl.signal.aborted) setState({ service, database })
    })
    return () => ctrl.abort()
  }, [enabled])
  return state
}

/** 系統概況：服務與資料庫狀態、AI 狀態與失敗次數、登入失敗、帳號鎖定與近期錯誤（S-09.3）。 */
export function SystemOverview() {
  const t = useT()
  const [period, setPeriod] = useState<Period>('24h')
  const status = useFetch<SystemStatus>(`/api/admin/status?period=${period}`)
  const failed = status.error !== undefined && status.data === undefined
  const basic = useBasicHealth(failed)

  return (
    <section aria-labelledby="overview-heading" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 id="overview-heading" className="text-xl font-medium">
          {t('admin.overview.title')}
        </h2>
        <div role="group" aria-label={t('admin.overview.period')} className="flex gap-1">
          {PERIODS.map((p) => (
            <Button
              key={p}
              type="button"
              size="sm"
              variant={p === period ? 'default' : 'outline'}
              aria-pressed={p === period}
              onClick={() => setPeriod(p)}
            >
              {t(PERIOD_LABEL[p])}
            </Button>
          ))}
        </div>
      </div>

      {status.loading && !status.data && !failed ? <LoadingBlock /> : null}

      {failed ? (
        <>
          <ErrorBlock error={status.error} onRetry={status.reload} />
          {basic ? (
            <Card>
              <CardHeader>
                <CardDescription>{t('admin.overview.fallbackNote')}</CardDescription>
              </CardHeader>
              <CardContent className="flex flex-wrap gap-6">
                <HealthItem label={t('admin.overview.service')} state={basic.service} />
                <HealthItem label={t('admin.overview.database')} state={basic.database} />
              </CardContent>
            </Card>
          ) : null}
        </>
      ) : null}

      {status.data ? <Overview data={status.data} period={period} /> : null}
    </section>
  )
}

function HealthItem({ label, state }: { label: string; state: Health }) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-muted-foreground text-sm">{label}</span>
      <HealthBadge state={state} />
    </div>
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
    <div className="space-y-4">
      <Card>
        <CardContent className="flex flex-wrap gap-6">
          <HealthItem label={t('admin.overview.service')} state={healthOf(data.service)} />
          <HealthItem label={t('admin.overview.database')} state={healthOf(data.database)} />
          {stats ? (
            <div className="flex items-center gap-2">
              <span className="text-muted-foreground text-sm">{t('admin.overview.ai')}</span>
              <Badge
                variant={
                  stats.ai_status === 'has_failures'
                    ? 'destructive'
                    : stats.ai_status === 'ok'
                      ? 'secondary'
                      : 'outline'
                }
              >
                {t(AI_LABEL[stats.ai_status])}
              </Badge>
            </div>
          ) : null}
        </CardContent>
      </Card>

      {!data.diagnostics_available || !stats ? (
        <Alert role="status">
          <AlertDescription>{t('admin.overview.unavailable')}</AlertDescription>
        </Alert>
      ) : (
        <>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <StatCard
              title={t('admin.overview.aiFailures')}
              value={stats.ai_failures}
              note={t('admin.overview.aiCalls', { count: stats.ai_calls })}
              to={stats.ai_failures > 0 ? link({ kind: 'ai' }) : undefined}
            />
            <StatCard
              title={t('admin.overview.invalidOutputs')}
              value={stats.ai_invalid_outputs}
              note={t('admin.overview.invalidOutputsNote')}
              to={stats.ai_invalid_outputs > 0 ? link({ kind: 'ai', failure_class: 'invalid_output' }) : undefined}
            />
            <StatCard
              title={t('admin.overview.loginFailures')}
              value={stats.login_failures}
              to={stats.login_failures > 0 ? link({ kind: 'login' }) : undefined}
            />
            <StatCard
              title={t('admin.overview.accountLocks')}
              value={stats.account_locks}
              to={stats.account_locks > 0 ? link({ kind: 'account_lock' }) : undefined}
            />
          </div>
          <RecentErrors errors={data.recent_errors} />
        </>
      )}
    </div>
  )
}

function StatCard({ title, value, note, to }: { title: string; value: number; note?: string; to?: string }) {
  const t = useT()
  return (
    <Card className="justify-between">
      <CardHeader>
        <CardDescription>{title}</CardDescription>
        <CardTitle className="text-3xl tabular-nums">{value}</CardTitle>
        {note ? <CardDescription>{note}</CardDescription> : null}
      </CardHeader>
      {to ? (
        <CardContent>
          <Button asChild variant="outline" size="sm">
            <Link to={to} aria-label={`${title} — ${t('admin.overview.viewDetails')}`}>
              {t('admin.overview.viewDetails')}
            </Link>
          </Button>
        </CardContent>
      ) : null}
    </Card>
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
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2">
        <CardTitle className="text-base">{t('admin.overview.recent')}</CardTitle>
        <Button asChild variant="outline" size="sm">
          <Link to="/admin/diagnostics">{t('admin.overview.viewAll')}</Link>
        </Button>
      </CardHeader>
      <CardContent>
        {errors.length === 0 ? (
          <p className="text-muted-foreground text-sm">{t('admin.overview.recentEmpty')}</p>
        ) : (
          <ul className="divide-y text-sm">
            {errors.map((e, i) => (
              <li key={`${e.request_id}-${i}`} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2">
                <time dateTime={e.occurred_at} className="text-muted-foreground">
                  {date.format(new Date(e.occurred_at))}
                </time>
                <span>{t(KIND_LABEL[e.kind])}</span>
                <Badge variant="outline">{t(CLASS_LABEL[e.failure_class])}</Badge>
                <span>{FUNCTION_LABEL[e.function] ? t(FUNCTION_LABEL[e.function]) : e.function}</span>
                <Link
                  to={`/admin/diagnostics?request_id=${encodeURIComponent(e.request_id)}`}
                  className="font-mono text-xs underline"
                >
                  {e.request_id}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  )
}
