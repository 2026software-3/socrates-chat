import { useMemo, useState } from 'react'
import { ErrorBlock, LoadingBlock, PageHeader } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { PERIODS, type Period } from '@/features/admin/diagnostics-model'
import { formatDuration } from '@/features/admin/duration'
import {
  PERIOD_DAYS,
  serviceText,
  type ServiceRow,
  type ServicesView,
  type ServiceStatus,
  type VoiceNodeRow,
} from '@/features/admin/service-health-model'
import { errorText, useI18n, useT, type MessageKey } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'

const PERIOD_LABEL: Record<Period, MessageKey> = {
  '24h': 'admin.overview.period.24h',
  '7d': 'admin.overview.period.7d',
  '30d': 'admin.overview.period.30d',
}

type Filter = 'all' | 'healthy' | 'unhealthy' | 'pending'
const FILTERS: Filter[] = ['all', 'healthy', 'unhealthy', 'pending']
const FILTER_LABEL: Record<Filter, MessageKey> = {
  all: 'admin.services.filter.all',
  healthy: 'admin.services.filter.healthy',
  unhealthy: 'admin.services.filter.unhealthy',
  pending: 'admin.services.filter.pending',
}

const STATE_LABEL: Record<ServiceStatus, MessageKey> = {
  healthy: 'admin.services.state.healthy',
  unhealthy: 'admin.services.state.unhealthy',
  checking: 'admin.services.state.checking',
  no_data: 'admin.services.state.no_data',
}

const FAILURE_LABEL = {
  connection_failed: 'admin.services.failure.connection_failed',
  timeout: 'admin.services.failure.timeout',
  non_2xx: 'admin.services.failure.non_2xx',
} as const satisfies Record<string, MessageKey>

const matches = (filter: Filter, status: ServiceStatus) =>
  filter === 'all' ||
  (filter === 'pending' ? status === 'checking' || status === 'no_data' : status === filter)

/** 後台「服務」分頁（F-39.3）：各容器服務的狀態、可用率與最近的狀態變化，可立即檢查。 */
export function ServicesPage() {
  const t = useT()
  const [period, setPeriod] = useState<Period>('24h')
  const [filter, setFilter] = useState<Filter>('all')
  const view = useFetch<ServicesView>(`/api/admin/services?period_days=${PERIOD_DAYS[period]}`)
  // 「立即檢查」的結果：只有目前狀態，可用率與歷史仍用列表的
  const [checked, setChecked] = useState<{ source: ServicesView | undefined; rows: Record<string, ServiceRow> }>({
    source: undefined,
    rows: {},
  })
  // 列表重新載入後，舊的立即檢查結果就不再適用
  const fresh = checked.source === view.data ? checked.rows : {}

  const rows = useMemo(() => {
    const merged = (view.data?.services ?? []).map((row) => {
      const latest = fresh[row.name]
      return latest ? { ...latest, availability: row.availability, recent_events: row.recent_events } : row
    })
    // 異常的排在前面；其餘維持後端的順序
    return [...merged].sort((a, b) => Number(b.status === 'unhealthy') - Number(a.status === 'unhealthy'))
  }, [view.data, fresh])

  const failed = view.error !== undefined && view.data === undefined
  const onlyDatabase = view.data !== undefined && rows.every((row) => row.target === null)
  const shown = rows.filter((row) => matches(filter, row.status))

  return (
    <div className="space-y-6">
      <PageHeader title={t('admin.services.title')} description={t('admin.services.intro')} />
      <div className="flex flex-wrap items-center justify-between gap-3">
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
        <div role="group" aria-label={t('admin.services.filter')} className="flex flex-wrap gap-1">
          {FILTERS.map((f) => (
            <Button
              key={f}
              type="button"
              size="sm"
              variant={f === filter ? 'default' : 'outline'}
              aria-pressed={f === filter}
              onClick={() => setFilter(f)}
            >
              {t(FILTER_LABEL[f])}
            </Button>
          ))}
        </div>
      </div>

      {view.loading && !view.data && !failed ? <LoadingBlock /> : null}
      {failed ? <ErrorBlock error={view.error} onRetry={view.reload} /> : null}

      {view.data ? (
        <>
          {!view.data.history_available ? (
            <Alert role="status">
              <AlertDescription>{t('admin.services.historyUnavailable')}</AlertDescription>
            </Alert>
          ) : null}
          {onlyDatabase ? (
            <Alert role="status">
              <AlertDescription>{t('admin.services.emptyConfig')}</AlertDescription>
            </Alert>
          ) : null}
          {shown.length === 0 ? (
            <p className="text-muted-foreground rounded-md border border-dashed p-6 text-center text-sm">
              {t('admin.services.filterEmpty')}
            </p>
          ) : (
            <ul className="space-y-4">
              {shown.map((row) => (
                <li key={row.name}>
                  <ServiceCard
                    row={row}
                    period={period}
                    historyAvailable={view.data?.history_available ?? false}
                    onChecked={(next) =>
                      setChecked((current) => ({
                        source: view.data,
                        rows: { ...(current.source === view.data ? current.rows : {}), [row.name]: next },
                      }))
                    }
                  />
                </li>
              ))}
            </ul>
          )}
          {view.data.voice_nodes.length > 0 ? <VoiceNodes nodes={view.data.voice_nodes} /> : null}
        </>
      ) : null}
    </div>
  )
}

const NODE_STATUS_LABEL = {
  active: 'admin.services.voiceNodes.status.active',
  draining: 'admin.services.voiceNodes.status.draining',
  disabled: 'admin.services.voiceNodes.status.disabled',
} as const satisfies Record<VoiceNodeRow['status'], MessageKey>

/** 即時語音節點（S-19）：每個節點一張卡，狀態、健康與目前場次數。 */
function VoiceNodes({ nodes }: { nodes: VoiceNodeRow[] }) {
  const t = useT()
  return (
    <section aria-labelledby="voice-nodes-title" className="space-y-3">
      <div className="space-y-1">
        <h2 id="voice-nodes-title" className="text-lg font-semibold">
          {t('admin.services.voiceNodes.title')}
        </h2>
        <p className="text-muted-foreground text-sm">{t('admin.services.voiceNodes.intro')}</p>
      </div>
      <ul className="space-y-4">
        {nodes.map((node) => {
          const health = t(node.healthy ? 'admin.services.state.healthy' : 'admin.services.state.unhealthy')
          return (
            <li key={node.name}>
              <Card>
                <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-2">
                  <CardTitle className="text-base">{node.name}</CardTitle>
                  <div className="flex items-center gap-2">
                    <Badge variant="outline">{t(NODE_STATUS_LABEL[node.status])}</Badge>
                    <Badge
                      variant={node.healthy ? 'secondary' : 'destructive'}
                      role="status"
                      aria-label={`${node.name}: ${health}`}
                    >
                      {health}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent>
                  <dl className="text-sm">
                    <dt className="text-muted-foreground">{t('admin.services.voiceNodes.sessions')}</dt>
                    <dd>
                      {node.max_sessions === null
                        ? t('admin.services.voiceNodes.sessionsUnlimited', { n: node.sessions })
                        : t('admin.services.voiceNodes.sessionsValue', { n: node.sessions, max: node.max_sessions })}
                    </dd>
                  </dl>
                </CardContent>
              </Card>
            </li>
          )
        })}
      </ul>
    </section>
  )
}

function ServiceCard({
  row,
  period,
  historyAvailable,
  onChecked,
}: {
  row: ServiceRow
  period: Period
  historyAvailable: boolean
  onChecked: (row: ServiceRow) => void
}) {
  const t = useT()
  const { lang } = useI18n()
  const date = useMemo(() => new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'medium' }), [lang])
  const percent = useMemo(() => new Intl.NumberFormat(lang, { style: 'percent', maximumFractionDigits: 2 }), [lang])
  const [checking, setChecking] = useState(false)
  const [message, setMessage] = useState<string>()
  const [open, setOpen] = useState(false)
  const name = serviceText(row.label, lang)

  const check = async () => {
    if (checking) return
    setChecking(true)
    setMessage(undefined)
    try {
      onChecked(await api.post<ServiceRow>('/api/admin/services/check', { name: row.name }))
    } catch (e) {
      setMessage(
        isApiError(e) && e.status === 429
          ? t('admin.services.rateLimited')
          : isApiError(e)
            ? errorText(t, e)
            : t('admin.services.checkFailed'),
      )
    } finally {
      setChecking(false)
    }
  }

  const time = (iso: string | null) => (iso ? <time dateTime={iso}>{date.format(new Date(iso))}</time> : t('admin.services.never'))
  const since =
    row.status_since && (row.status === 'healthy' || row.status === 'unhealthy')
      ? t(row.status === 'healthy' ? 'admin.services.since.healthy' : 'admin.services.since.unhealthy', {
          duration: formatDuration(t, (new Date(row.checked_at ?? row.status_since).getTime() - new Date(row.status_since).getTime()) / 1000),
        })
      : null
  const shownStatus = checking ? t('admin.services.checking') : t(STATE_LABEL[row.status])

  return (
    <Card>
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-2">
        <div className="space-y-1">
          <CardTitle className="text-base">{name}</CardTitle>
          <p className="text-muted-foreground text-sm">
            {t('admin.services.impact')}：{serviceText(row.impact, lang)}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Badge
            variant={row.status === 'unhealthy' ? 'destructive' : row.status === 'healthy' ? 'secondary' : 'outline'}
            role="status"
            aria-label={`${name}: ${shownStatus}`}
          >
            {shownStatus}
            {row.status === 'unhealthy' && row.failure ? ` · ${t(FAILURE_LABEL[row.failure])}` : ''}
          </Badge>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={checking}
            aria-label={t('admin.services.checkNowFor', { name })}
            onClick={() => void check()}
          >
            {checking ? t('admin.services.checking') : t('admin.services.checkNow')}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {message ? (
          <Alert variant="destructive" role="alert">
            <AlertDescription>{message}</AlertDescription>
          </Alert>
        ) : null}
        <dl className="grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-3">
          {row.target ? (
            <div>
              <dt className="text-muted-foreground">{t('admin.services.target')}</dt>
              <dd className="font-mono text-xs">{row.target}</dd>
            </div>
          ) : null}
          <div>
            <dt className="text-muted-foreground">{t('admin.services.responseTime')}</dt>
            <dd>
              {row.response_time_ms === null ? '–' : t('admin.services.responseTimeValue', { ms: row.response_time_ms })}
            </dd>
          </div>
          <div>
            <dt className="text-muted-foreground">{t('admin.services.checkedAt')}</dt>
            <dd>{time(row.checked_at)}</dd>
          </div>
          <div>
            <dt className="text-muted-foreground">{t('admin.services.lastHealthy')}</dt>
            <dd>{time(row.last_healthy_at)}</dd>
          </div>
          <div>
            <dt className="text-muted-foreground">{t('admin.services.availability', { period: t(PERIOD_LABEL[period]) })}</dt>
            <dd>{row.availability === null ? '–' : percent.format(row.availability)}</dd>
          </div>
          {since ? (
            <div>
              <dt className="sr-only">{t(STATE_LABEL[row.status])}</dt>
              <dd>{since}</dd>
            </div>
          ) : null}
        </dl>
        <Button
          type="button"
          variant="link"
          size="sm"
          className="px-0"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
        >
          {open ? t('admin.services.historyHide') : t('admin.services.history')}
        </Button>
        {open ? (
          historyAvailable && row.recent_events.length > 0 ? (
            <ul className="divide-y text-sm">
              {row.recent_events.map((event, i) => (
                <li key={`${event.occurred_at}-${i}`} className="flex flex-wrap items-center gap-x-3 py-2">
                  <time dateTime={event.occurred_at} className="text-muted-foreground">
                    {date.format(new Date(event.occurred_at))}
                  </time>
                  <span>{t(STATE_LABEL[event.status])}</span>
                  {event.failure ? <Badge variant="outline">{t(FAILURE_LABEL[event.failure])}</Badge> : null}
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-muted-foreground text-sm">
              {historyAvailable ? t('admin.services.historyEmpty') : t('admin.services.historyUnavailable')}
            </p>
          )
        ) : null}
      </CardContent>
    </Card>
  )
}
