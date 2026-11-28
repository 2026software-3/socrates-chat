import { useMemo, useState, type ReactNode } from 'react'
import {
  Activity,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Clock,
  Gauge,
  Globe,
  RotateCw,
  Server,
  Timer,
  Users,
  type LucideIcon,
} from 'lucide-react'
import { cn } from 'cn'
import { PageHeader } from '@/components/page'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { StackBar } from '@/components/vedute/stack-bar'
import {
  AdminCard,
  AdminEmptyState,
  AdminErrorAlert,
  AdminSectionSkeleton,
  AdminStatusBadge,
  PeriodSelector,
  type AdminTone,
} from '@/features/admin/admin-ui'
import type { Period } from '@/features/admin/diagnostics-model'
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

const STATE_TONE: Record<ServiceStatus, AdminTone> = {
  healthy: 'active',
  unhealthy: 'error',
  checking: 'inactive',
  no_data: 'inactive',
}

const STATE_ACCENT: Record<ServiceStatus, string> = {
  healthy: 'border-l-3 border-l-aerugo',
  unhealthy: 'border-l-3 border-l-error',
  checking: 'border-l-3 border-l-ink-soft',
  no_data: 'border-l-3 border-l-ink-soft',
}

const FAILURE_LABEL: Record<string, MessageKey> = {
  connection_failed: 'admin.services.failure.connection_failed',
  timeout: 'admin.services.failure.timeout',
  non_2xx: 'admin.services.failure.non_2xx',
}

const failureText = (t: ReturnType<typeof useT>, code: string) =>
  FAILURE_LABEL[code] ? t(FAILURE_LABEL[code]) : code

const matches = (filter: Filter, status: ServiceStatus) =>
  filter === 'all' ||
  (filter === 'pending' ? status === 'checking' || status === 'no_data' : status === filter)

const EMPTY_CHECKED: Record<string, ServiceRow> = {}

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
  const fresh = checked.source === view.data ? checked.rows : EMPTY_CHECKED

  const rows = useMemo(() => {
    // 語音節點與其他服務同列：節點多帶 node_status 與場次，卡片再顯示出來
    const merged = [...(view.data?.services ?? []), ...(view.data?.voice_nodes ?? [])].map((row) => {
      const latest = fresh[row.name]
      return latest ? { ...row, ...latest, availability: row.availability, recent_events: row.recent_events } : row
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

      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-faint pb-4">
        <PeriodSelector value={period} onChange={setPeriod} />
        <div role="group" aria-label={t('admin.services.filter')} className="flex flex-wrap gap-2">
          {FILTERS.map((f) => (
            <Button
              key={f}
              type="button"
              variant={f === filter ? 'primary' : 'quiet'}
              aria-pressed={f === filter}
              onClick={() => setFilter(f)}
            >
              {t(FILTER_LABEL[f])}
            </Button>
          ))}
        </div>
      </div>

      {view.loading && !view.data && !failed ? <AdminSectionSkeleton rows={6} /> : null}
      {failed ? <AdminErrorAlert error={view.error} onRetry={view.reload} /> : null}

      {view.data ? (
        <div className="space-y-4">
          {!view.data.history_available ? (
            <Alert kind="info">{t('admin.services.historyUnavailable')}</Alert>
          ) : null}
          {onlyDatabase ? (
            <Alert kind="info">{t('admin.services.emptyConfig')}</Alert>
          ) : null}
          {shown.length === 0 ? (
            <AdminEmptyState>{t('admin.services.filterEmpty')}</AdminEmptyState>
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
        </div>
      ) : null}
    </div>
  )
}

const NODE_STATUS_LABEL = {
  active: 'admin.services.voiceNodes.status.active',
  draining: 'admin.services.voiceNodes.status.draining',
  disabled: 'admin.services.voiceNodes.status.disabled',
} as const satisfies Record<VoiceNodeRow['node_status'], MessageKey>

function ServiceFact({
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
      <dd className="type-body text-ink tabular-nums break-words">{children}</dd>
    </div>
  )
}

/** 語音節點才有的欄位：節點狀態與目前場次。 */
function VoiceNodeFacts({ row }: { row: VoiceNodeRow }) {
  const t = useT()
  return (
    <>
      <ServiceFact icon={Server} label={t('admin.services.voiceNodes.nodeStatus')}>
        {t(NODE_STATUS_LABEL[row.node_status])}
      </ServiceFact>
      <ServiceFact icon={Users} label={t('admin.services.voiceNodes.sessions')}>
        {row.max_sessions === null
          ? t('admin.services.voiceNodes.sessionsUnlimited', { n: row.sessions })
          : t('admin.services.voiceNodes.sessionsValue', { n: row.sessions, max: row.max_sessions })}
      </ServiceFact>
    </>
  )
}

function ServiceCard({
  row,
  period,
  historyAvailable,
  onChecked,
}: {
  row: ServiceRow | VoiceNodeRow
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

  const time = (iso: string | null) =>
    iso ? <time dateTime={iso}>{date.format(new Date(iso))}</time> : <span className="text-ink-soft">{t('admin.services.never')}</span>

  const since =
    row.status_since && (row.status === 'healthy' || row.status === 'unhealthy')
      ? t(row.status === 'healthy' ? 'admin.services.since.healthy' : 'admin.services.since.unhealthy', {
          duration: formatDuration(
            t,
            (new Date(row.checked_at ?? row.status_since).getTime() - new Date(row.status_since).getTime()) / 1000,
          ),
        })
      : null

  const shownStatus = checking ? t('admin.services.checking') : t(STATE_LABEL[row.status])
  const badgeText =
    row.status === 'unhealthy' && row.failure
      ? `${shownStatus} · ${failureText(t, row.failure)}`
      : shownStatus

  return (
    <AdminCard className={cn('overflow-hidden', STATE_ACCENT[row.status])}>
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-ink-faint bg-paper/35 px-5 py-4">
        <div className="min-w-0 space-y-1">
          <h2 data-slot="card-title" className="type-h3 text-ink">
            {name}
          </h2>
          <p className="type-note text-ink-soft">
            {t('admin.services.impact')}：{serviceText(row.impact, lang)}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2.5">
          <AdminStatusBadge
            tone={checking ? 'inactive' : STATE_TONE[row.status]}
            role="status"
            aria-label={`${name}: ${shownStatus}`}
          >
            {badgeText}
          </AdminStatusBadge>
          <Button
            type="button"
            variant="quiet"
            disabled={checking}
            aria-label={t('admin.services.checkNowFor', { name })}
            onClick={() => void check()}
          >
            <RotateCw className="size-4" strokeWidth={1.5} aria-hidden="true" />
            {checking ? t('admin.services.checking') : t('admin.services.checkNow')}
          </Button>
        </div>
      </div>

      <div className="space-y-4 px-5 py-4">
        {message ? <Alert kind="error">{message}</Alert> : null}

        <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {row.target ? (
            <ServiceFact icon={Globe} label={t('admin.services.target')}>
              <span className="break-all">{row.target}</span>
            </ServiceFact>
          ) : null}

          <ServiceFact icon={Gauge} label={t('admin.services.responseTime')}>
            {row.response_time_ms === null ? '–' : t('admin.services.responseTimeValue', { ms: row.response_time_ms })}
          </ServiceFact>

          <ServiceFact icon={Clock} label={t('admin.services.checkedAt')}>
            {time(row.checked_at)}
          </ServiceFact>

          <ServiceFact icon={CheckCircle2} label={t('admin.services.lastHealthy')}>
            {time(row.last_healthy_at)}
          </ServiceFact>

          <ServiceFact
            icon={Activity}
            label={t('admin.services.availability', { period: t(PERIOD_LABEL[period]) })}
          >
            <span>{row.availability === null ? '–' : percent.format(row.availability)}</span>
            {row.availability !== null ? (
              <StackBar
                className="mt-1.5"
                size="sm"
                showLegend={false}
                ariaLabel={t('admin.services.availability', { period: t(PERIOD_LABEL[period]) })}
                segments={[
                  { label: t('admin.services.state.healthy'), value: row.availability, tone: 'aerugo' },
                  { label: t('admin.services.state.unhealthy'), value: Math.max(0, 1 - row.availability), tone: 'neutral' },
                ]}
              />
            ) : null}
          </ServiceFact>

          {'node_status' in row ? <VoiceNodeFacts row={row} /> : null}

          {since ? (
            <div className="space-y-1">
              <dt className="sr-only">{t(STATE_LABEL[row.status])}</dt>
              <dd className="type-note flex items-center gap-1.5 text-ink-soft">
                <Timer className="size-4 shrink-0" strokeWidth={1.5} aria-hidden="true" />
                <span>{since}</span>
              </dd>
            </div>
          ) : null}
        </dl>

        <div className="border-t border-ink-faint pt-2">
          <Button
            type="button"
            variant="link"
            className="px-0"
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
          >
            <span>{open ? t('admin.services.historyHide') : t('admin.services.history')}</span>
            {open ? (
              <ChevronUp className="size-4" strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <ChevronDown className="size-4" strokeWidth={1.5} aria-hidden="true" />
            )}
          </Button>

          {open ? (
            <div className="mt-2">
              {historyAvailable && row.recent_events.length > 0 ? (
                <ul className="divide-y divide-ink-faint border-t border-ink-faint">
                  {row.recent_events.map((event, i) => (
                    <li key={`${event.occurred_at}-${i}`} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 py-2.5">
                      <time dateTime={event.occurred_at} className="type-note text-ink-soft">
                        {date.format(new Date(event.occurred_at))}
                      </time>
                      <AdminStatusBadge tone={STATE_TONE[event.status]}>
                        {t(STATE_LABEL[event.status])}
                      </AdminStatusBadge>
                      {event.failure ? (
                        <AdminStatusBadge tone="error">{failureText(t, event.failure)}</AdminStatusBadge>
                      ) : null}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="type-note text-ink-soft">
                  {historyAvailable ? t('admin.services.historyEmpty') : t('admin.services.historyUnavailable')}
                </p>
              )}
            </div>
          ) : null}
        </div>
      </div>
    </AdminCard>
  )
}
