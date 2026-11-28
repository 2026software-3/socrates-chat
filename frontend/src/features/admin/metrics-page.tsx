import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import {
  Activity,
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronUp,
  Clock,
  Copy,
  Cpu,
  Database,
  Gauge,
  Layers,
  Mail,
  Mic,
  RotateCw,
  Server,
  ShieldCheck,
  Sparkles,
  Workflow,
  type LucideIcon,
} from 'lucide-react'
import { PageHeader } from '@/components/page'
import { Button } from '@/components/vedute/button'
import { Meta } from '@/components/vedute/meta'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import { StackBar, type StackBarSegment } from '@/components/vedute/stack-bar'
import { Toast, type ToastNotification } from '@/components/vedute/toast'
import {
  AdminCard,
  AdminEmptyState,
  AdminErrorAlert,
  AdminStatusBadge,
} from '@/features/admin/admin-ui'
import { FUNCTION_LABEL } from '@/features/admin/diagnostics-model'
import { formatDuration } from '@/features/admin/duration'
import { summarizeMetrics, type MetricsSummary, type OutcomeGroupRow } from '@/features/admin/metrics-model'
import { useMediaQuery } from '@/hooks/use-media-query'
import { useI18n, useT } from '@/i18n'
import { useTextFetch } from '@/lib/use-fetch'

const DEFAULT_ROUTE_LIMIT = 20

/** 後台「系統指標」分頁：讀取 `GET /api/admin/metrics`（Prometheus 0.0.4），視覺化呈現即時執行狀態與聚合指標（S-09.3）。 */
export function MetricsPage() {
  const t = useT()
  const { lang } = useI18n()
  const isDesktop = useMediaQuery('(min-width: 1024px)', true)
  const metrics = useTextFetch('/api/admin/metrics')
  const [rawOpen, setRawOpen] = useState(false)
  const [copied, setCopied] = useState(false)
  const [toast, setToast] = useState<ToastNotification | null>(null)
  const [toastEverShown, setToastEverShown] = useState(false)
  const copyTimerRef = useRef<number | undefined>(undefined)
  const toastIdRef = useRef(0)

  useEffect(() => () => window.clearTimeout(copyTimerRef.current), [])

  const summary = useMemo(
    () => (metrics.data !== undefined ? summarizeMetrics(metrics.data) : undefined),
    [metrics.data],
  )
  const number = useMemo(() => new Intl.NumberFormat(lang), [lang])

  const showToast = (kind: ToastNotification['kind'], message: string) => {
    toastIdRef.current += 1
    setToastEverShown(true)
    setToast({ id: toastIdRef.current, kind, message })
  }

  const copyRaw = async () => {
    if (!metrics.data) return
    if (typeof navigator === 'undefined' || !navigator.clipboard?.writeText) {
      showToast('error', t('admin.metrics.copyFailed'))
      return
    }
    try {
      await navigator.clipboard.writeText(metrics.data)
      setCopied(true)
      window.clearTimeout(copyTimerRef.current)
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 2000)
      showToast('success', t('admin.metrics.rawCopiedToast'))
    } catch {
      showToast('error', t('admin.metrics.copyFailed'))
    }
  }

  const refreshAction = (
    <Button
      type="button"
      variant="quiet"
      disabled={metrics.loading}
      onClick={metrics.reload}
    >
      <RotateCw className="size-4" strokeWidth={1.5} aria-hidden="true" />
      {metrics.loading ? t('admin.metrics.refreshing') : t('admin.metrics.refresh')}
    </Button>
  )

  return (
    <div className="space-y-6">
      <PageHeader
        title={t('admin.metrics.title')}
        description={t('admin.metrics.intro')}
        actions={refreshAction}
      />

      <p className="type-note border-l border-caeruleum py-1 pl-3 text-ink-soft">{t('admin.metrics.traceHint')}</p>

      {metrics.loading && metrics.data === undefined && !metrics.error ? <MetricsSkeleton /> : null}
      {metrics.error ? <AdminErrorAlert error={metrics.error} onRetry={metrics.reload} /> : null}

      {summary ? (
        summary.sampleCount === 0 ? (
          <AdminEmptyState>{t('admin.metrics.empty')}</AdminEmptyState>
        ) : (
          <div className="space-y-6">
            <LiveSection summary={summary} number={number} />
            <HttpSection summary={summary} isDesktop={isDesktop} number={number} />

            <div className="grid gap-6 lg:grid-cols-2">
              <AiSection summary={summary} number={number} />
              <JobsSection summary={summary} number={number} />
            </div>

            <DependenciesSection summary={summary} number={number} />

            <AdminCard className="overflow-hidden">
              <div className="flex flex-wrap items-center justify-between gap-3 px-5 py-3.5">
                <div className="flex flex-wrap items-center gap-3">
                  <Button
                    type="button"
                    variant="link"
                    className="px-0"
                    aria-expanded={rawOpen}
                    onClick={() => setRawOpen((v) => !v)}
                  >
                    <span>{rawOpen ? t('admin.metrics.rawHide') : t('admin.metrics.rawShow')}</span>
                    {rawOpen ? (
                      <ChevronUp className="size-4" strokeWidth={1.5} aria-hidden="true" />
                    ) : (
                      <ChevronDown className="size-4" strokeWidth={1.5} aria-hidden="true" />
                    )}
                  </Button>
                  <span className="type-note text-ink-soft">
                    {t('admin.metrics.rawMeta', { families: summary.familyCount, samples: summary.sampleCount })}
                  </span>
                </div>
                <Button type="button" variant="quiet" onClick={() => void copyRaw()}>
                  {copied ? (
                    <Check className="size-4 text-aerugo" strokeWidth={1.5} aria-hidden="true" />
                  ) : (
                    <Copy className="size-4" strokeWidth={1.5} aria-hidden="true" />
                  )}
                  {copied ? t('admin.metrics.rawCopied') : t('admin.metrics.rawCopy')}
                </Button>
              </div>
              {rawOpen && metrics.data ? (
                <pre className="type-note font-serif max-h-96 overflow-auto whitespace-pre-wrap break-all border-t border-ink-faint bg-paper/40 p-4 text-ink tabular-nums">
                  {metrics.data}
                </pre>
              ) : null}
            </AdminCard>
          </div>
        )
      ) : null}

      {toastEverShown ? (
        <Toast notification={toast} dismissLabel={t('admin.metrics.dismissToast')} onDismiss={() => setToast(null)} />
      ) : null}
    </div>
  )
}

function MetricGaugeCard({
  icon: Icon,
  iconClass = 'text-ink-soft',
  label,
  value,
}: {
  icon: LucideIcon
  iconClass?: string
  label: string
  value: string
}) {
  return (
    <AdminCard className="space-y-2 p-5">
      <div className="flex items-center gap-2">
        <Icon className={`size-4 shrink-0 ${iconClass}`} strokeWidth={1.5} aria-hidden="true" />
        <span className="type-note text-ink-soft">{label}</span>
      </div>
      <p className="type-h1 tabular-nums text-ink">{value}</p>
    </AdminCard>
  )
}

function LiveSection({ summary, number }: { summary: MetricsSummary; number: Intl.NumberFormat }) {
  const t = useT()
  const { lang } = useI18n()
  const date = useMemo(() => new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'short' }), [lang])

  return (
    <section aria-labelledby="metrics-live-heading" className="space-y-4">
      <h2 id="metrics-live-heading" className="type-h2 text-ink">
        {t('admin.metrics.liveTitle')}
      </h2>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <MetricGaugeCard
          icon={Activity}
          iconClass="text-caeruleum"
          label={t('admin.metrics.httpInFlight')}
          value={number.format(summary.live.httpInFlight)}
        />
        <MetricGaugeCard
          icon={Sparkles}
          iconClass="text-caeruleum"
          label={t('admin.metrics.liveReplies')}
          value={number.format(summary.live.liveRepliesActive)}
        />
        <MetricGaugeCard
          icon={Mic}
          label={t('admin.metrics.voiceSessions')}
          value={number.format(summary.live.voiceSessionsActive)}
        />
        <MetricGaugeCard
          icon={Workflow}
          label={t('admin.metrics.jobsInFlight')}
          value={number.format(summary.live.backgroundJobsInFlight)}
        />
      </div>

      <AdminCard className="px-5 py-4">
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
          {summary.process.version ? (
            <Meta
              icon={Cpu}
              variant="bibliographic"
              value={t('admin.health.version', { version: summary.process.version })}
            />
          ) : null}
          {summary.process.uptimeSeconds !== null ? (
            <Meta
              icon={Clock}
              variant="bibliographic"
              value={t('admin.health.uptime', { duration: formatDuration(t, summary.process.uptimeSeconds) })}
            />
          ) : null}
          {summary.process.maintenanceOn !== null ? (
            <div className="flex items-center gap-2">
              <span className="type-note text-ink-soft">{t('admin.health.maintenance')}</span>
              <AdminStatusBadge tone={summary.process.maintenanceOn ? 'draft' : 'inactive'}>
                {summary.process.maintenanceOn ? t('admin.health.maintenance.on') : t('admin.health.maintenance.off')}
              </AdminStatusBadge>
            </div>
          ) : null}
          {summary.live.dbPool ? (
            <Meta
              icon={Database}
              label={t('admin.health.pool')}
              value={t('admin.health.poolValue', {
                used: summary.live.dbPool.active,
                idle: summary.live.dbPool.idle,
                max: summary.live.dbPool.max,
              })}
            />
          ) : null}
          {summary.process.lastCleanupTimestamp ? (
            <Meta
              icon={Clock}
              label={t('admin.health.cleanup')}
              value={date.format(new Date(summary.process.lastCleanupTimestamp * 1000))}
            />
          ) : null}
        </div>
        {summary.live.servicesUp.length > 0 ? (
          <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-ink-faint pt-3">
            <span className="type-note text-ink-soft">{t('admin.health.services')}</span>
            {summary.live.servicesUp.map((svc) => (
              <AdminStatusBadge key={svc.service} tone={svc.up ? 'active' : 'error'}>
                {`${svc.service}: ${svc.up ? t('admin.services.state.healthy') : t('admin.services.state.unhealthy')}`}
              </AdminStatusBadge>
            ))}
          </div>
        ) : null}
      </AdminCard>
    </section>
  )
}

function HttpSection({
  summary,
  isDesktop,
  number,
}: {
  summary: MetricsSummary
  isDesktop: boolean
  number: Intl.NumberFormat
}) {
  const t = useT()
  const [showAllRoutes, setShowAllRoutes] = useState(false)
  const { http } = summary
  const ms = (v: number | null) => (v === null ? '—' : t('admin.metrics.msValue', { ms: number.format(v) }))
  const visibleRoutes = showAllRoutes ? http.routes : http.routes.slice(0, DEFAULT_ROUTE_LIMIT)
  const hasMoreRoutes = http.routes.length > DEFAULT_ROUTE_LIMIT

  const statusSegments: StackBarSegment[] = [
    { label: '2xx', value: http.statusClasses['2xx'], valueLabel: number.format(http.statusClasses['2xx']), tone: 'aerugo' },
    { label: '3xx', value: http.statusClasses['3xx'], valueLabel: number.format(http.statusClasses['3xx']), tone: 'caeruleum' },
    { label: '4xx', value: http.statusClasses['4xx'], valueLabel: number.format(http.statusClasses['4xx']), tone: 'ochra' },
    { label: '5xx', value: http.statusClasses['5xx'], valueLabel: number.format(http.statusClasses['5xx']), tone: 'neutral' },
    ...(http.statusClasses.other > 0
      ? [{ label: t('admin.metrics.statusOther'), value: http.statusClasses.other, valueLabel: number.format(http.statusClasses.other), tone: 'track' as const }]
      : []),
  ]

  return (
    <section aria-labelledby="metrics-http-heading" className="space-y-4">
      <h2 id="metrics-http-heading" className="type-h2 text-ink">
        {t('admin.metrics.httpTitle')}
      </h2>

      <AdminCard className="space-y-5 p-5">
        <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <StatItem label={t('admin.metrics.httpTotal')} value={number.format(http.totalRequests)} />
          <StatItem
            label={t('admin.metrics.httpErrors')}
            value={number.format(http.errorsTotal)}
            tone={http.errorsTotal > 0 ? 'ochra' : 'ink'}
          />
          <StatItem label={t('admin.metrics.httpAborted')} value={number.format(http.abortedTotal)} />
          <StatItem
            label={t('admin.metrics.httpTimeouts')}
            value={number.format(http.timeoutsTotal)}
            tone={http.timeoutsTotal > 0 ? 'error' : 'ink'}
          />
        </dl>

        <div className="space-y-2 border-t border-ink-faint pt-4">
          <p className="type-note text-ink-soft">{t('admin.metrics.statusDistribution')}</p>
          <StackBar
            ariaLabel={t('admin.metrics.statusDistribution')}
            emptyLabel={t('admin.metrics.noRoutes')}
            segments={statusSegments}
          />
        </div>

        {http.topErrors.length > 0 ? (
          <div className="flex flex-wrap items-center gap-2 border-t border-ink-faint pt-4">
            <span className="type-note text-ink-soft">{t('admin.metrics.topErrors')}</span>
            {http.topErrors.slice(0, 6).map((err) => (
              <AdminStatusBadge key={err.code} tone="draft">
                {`${err.code}: ${number.format(err.count)}`}
              </AdminStatusBadge>
            ))}
          </div>
        ) : null}
      </AdminCard>

      {http.routes.length === 0 ? (
        <AdminEmptyState>{t('admin.metrics.noRoutes')}</AdminEmptyState>
      ) : (
        <div className="space-y-3">
          {isDesktop ? (
            <div className="overflow-hidden rounded-sm border border-ink-faint bg-surface">
              <table aria-label={t('admin.metrics.routeTable')} className="w-full border-collapse text-left">
                <thead>
                  <tr className="border-b border-ink-faint bg-paper-deep">
                    <th scope="col" className="type-body-strong px-5 py-3 text-ink">
                      {t('admin.metrics.route')}
                    </th>
                    <th scope="col" className="type-body-strong px-4 py-3 text-right text-ink">
                      {t('admin.metrics.requests')}
                    </th>
                    <th scope="col" className="type-body-strong px-4 py-3 text-right text-ink">
                      {t('admin.metrics.errors5xx')}
                    </th>
                    <th scope="col" className="type-body-strong px-4 py-3 text-right text-ink">
                      {t('admin.metrics.avgLatency')}
                    </th>
                    <th scope="col" className="type-body-strong px-5 py-3 text-right text-ink">
                      {t('admin.metrics.p95Latency')}
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-ink-faint">
                  {visibleRoutes.map((r) => (
                    <tr key={`${r.method}-${r.route}`}>
                      <td className="type-body px-5 py-3 text-ink break-all">
                        <span className="type-body-strong mr-2 text-caeruleum">{r.method}</span>
                        <span>{r.route}</span>
                        {r.aborted > 0 ? (
                          <span className="type-note ml-2 text-ink-soft tabular-nums">
                            ({t('admin.metrics.httpAborted')} {number.format(r.aborted)})
                          </span>
                        ) : null}
                      </td>
                      <td className="type-body px-4 py-3 text-right text-ink tabular-nums">
                        {number.format(r.total)}
                      </td>
                      <td className="px-4 py-3 text-right tabular-nums">
                        {r.errors5xx > 0 ? (
                          <AdminStatusBadge tone="error">{number.format(r.errors5xx)}</AdminStatusBadge>
                        ) : (
                          <span className="type-body text-ink-soft">0</span>
                        )}
                      </td>
                      <td className="type-body px-4 py-3 text-right text-ink-soft tabular-nums">{ms(r.avgMs)}</td>
                      <td className="type-body px-5 py-3 text-right text-ink tabular-nums">{ms(r.p95Ms)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <ul aria-label={t('admin.metrics.routeTable')} className="space-y-3">
              {visibleRoutes.map((r) => (
                <li key={`${r.method}-${r.route}`}>
                  <AdminCard className="space-y-2.5 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="type-body-strong break-all text-ink">
                        <span className="mr-2 text-caeruleum">{r.method}</span>
                        {r.route}
                      </span>
                      <div className="flex flex-wrap items-center gap-1.5">
                        {r.errors5xx > 0 ? (
                          <AdminStatusBadge tone="error">
                            {`${t('admin.metrics.errors5xx')} ${number.format(r.errors5xx)}`}
                          </AdminStatusBadge>
                        ) : null}
                        {r.aborted > 0 ? (
                          <AdminStatusBadge tone="inactive">
                            {`${t('admin.metrics.httpAborted')} ${number.format(r.aborted)}`}
                          </AdminStatusBadge>
                        ) : null}
                      </div>
                    </div>
                    <div className="grid gap-1.5">
                      <Meta icon={Activity} label={t('admin.metrics.requests')} value={number.format(r.total)} />
                      <Meta icon={Gauge} label={t('admin.metrics.avgLatency')} value={ms(r.avgMs)} />
                      <Meta icon={Clock} label={t('admin.metrics.p95Latency')} value={ms(r.p95Ms)} />
                    </div>
                  </AdminCard>
                </li>
              ))}
            </ul>
          )}

          {hasMoreRoutes ? (
            <div>
              <Button
                type="button"
                variant="quiet"
                onClick={() => setShowAllRoutes((v) => !v)}
              >
                {showAllRoutes
                  ? t('admin.metrics.showTopRoutes', { count: DEFAULT_ROUTE_LIMIT })
                  : t('admin.metrics.showAllRoutes', { count: http.routes.length })}
              </Button>
            </div>
          ) : null}
        </div>
      )}
    </section>
  )
}

function AiSection({ summary, number }: { summary: MetricsSummary; number: Intl.NumberFormat }) {
  const t = useT()
  const { ai } = summary
  return (
    <section aria-labelledby="metrics-ai-heading" className="space-y-3">
      <h2 id="metrics-ai-heading" className="type-h3 text-ink">
        {t('admin.metrics.aiTitle')}
      </h2>
      <AdminCard className="space-y-4 p-5">
        <dl className="grid grid-cols-2 gap-3 border-b border-ink-faint pb-4 sm:grid-cols-4">
          <StatItem label={t('admin.metrics.aiTotal')} value={number.format(ai.totalCalls)} />
          <StatItem
            label={t('admin.metrics.aiFailed')}
            value={number.format(ai.failedCalls)}
            tone={ai.failedCalls > 0 ? 'error' : 'ink'}
          />
          <StatItem
            label={t('admin.metrics.aiInvalidOutputs')}
            value={number.format(ai.invalidOutputs)}
            tone={ai.invalidOutputs > 0 ? 'ochra' : 'ink'}
          />
          <StatItem label={t('admin.metrics.aiRetries')} value={number.format(ai.retriesTotal)} />
        </dl>
        {ai.functions.length === 0 ? (
          <p className="type-note text-ink-soft">{t('admin.metrics.aiNoData')}</p>
        ) : (
          <ul className="divide-y divide-ink-faint">
            {ai.functions.map((row) => {
              const fnKey = FUNCTION_LABEL[row.fn]
              const fnLabel = fnKey ? t(fnKey) : row.fn
              return (
                <li key={row.fn} className="flex flex-wrap items-center justify-between gap-2 py-2.5 first:pt-0 last:pb-0">
                  <div className="space-y-0.5">
                    <p className="type-body-strong text-ink">{fnLabel}</p>
                    <p className="type-note text-ink-soft tabular-nums">
                      {t('admin.metrics.okFailSummary', {
                        ok: number.format(row.ok),
                        failures: number.format(row.failures),
                      })}
                      {row.invalidOutputs > 0
                        ? ` · ${t('admin.metrics.aiInvalidOutputs')} ${number.format(row.invalidOutputs)}`
                        : ''}
                      {row.retries > 0 ? ` · ${t('admin.metrics.aiRetries')} ${number.format(row.retries)}` : ''}
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    {row.p95Ms !== null ? (
                      <span className="type-note text-ink-soft tabular-nums">
                        {t('admin.metrics.p95Latency')} {t('admin.metrics.msValue', { ms: number.format(row.p95Ms) })}
                      </span>
                    ) : null}
                    <AdminStatusBadge
                      tone={row.failures > 0 ? 'error' : row.invalidOutputs > 0 ? 'draft' : 'active'}
                    >
                      {number.format(row.total)}
                    </AdminStatusBadge>
                  </div>
                </li>
              )
            })}
          </ul>
        )}
      </AdminCard>
    </section>
  )
}

function JobsSection({ summary, number }: { summary: MetricsSummary; number: Intl.NumberFormat }) {
  const t = useT()
  const { jobs } = summary
  return (
    <section aria-labelledby="metrics-jobs-heading" className="space-y-3">
      <h2 id="metrics-jobs-heading" className="type-h3 text-ink">
        {t('admin.metrics.jobsTitle')}
      </h2>
      <AdminCard className="p-5">
        {jobs.items.length === 0 ? (
          <p className="type-note text-ink-soft">{t('admin.metrics.jobsEmpty')}</p>
        ) : (
          <ul className="divide-y divide-ink-faint">
            {jobs.items.map((job) => {
              const parts = [
                `${t('admin.metrics.jobOk')} ${number.format(job.ok)}`,
                ...(job.degraded > 0 ? [`${t('admin.metrics.jobDegraded')} ${number.format(job.degraded)}`] : []),
                ...(job.skipped > 0 ? [`${t('admin.metrics.jobSkipped')} ${number.format(job.skipped)}`] : []),
                `${t('admin.metrics.jobFailed')} ${number.format(job.failed)}`,
                `${t('admin.metrics.jobAborted')} ${number.format(job.aborted)}`,
              ]
              return (
                <li key={job.job} className="flex flex-wrap items-center justify-between gap-2 py-2.5 first:pt-0 last:pb-0">
                  <div className="space-y-0.5">
                    <p className="type-body-strong text-ink">{job.job}</p>
                    <p className="type-note text-ink-soft tabular-nums">{parts.join(' · ')}</p>
                  </div>
                  <div className="flex items-center gap-2">
                    {job.inFlight > 0 ? (
                      <AdminStatusBadge tone="draft">
                        {`${t('admin.metrics.jobInFlight')} ${number.format(job.inFlight)}`}
                      </AdminStatusBadge>
                    ) : null}
                    {job.p95Ms !== null ? (
                      <span className="type-note text-ink-soft tabular-nums">
                        {t('admin.metrics.msValue', { ms: number.format(job.p95Ms) })}
                      </span>
                    ) : null}
                  </div>
                </li>
              )
            })}
          </ul>
        )}
      </AdminCard>
    </section>
  )
}

function DependenciesSection({ summary, number }: { summary: MetricsSummary; number: Intl.NumberFormat }) {
  const t = useT()
  const { dependencies: dep } = summary
  return (
    <section aria-labelledby="metrics-deps-heading" className="space-y-3">
      <h2 id="metrics-deps-heading" className="type-h3 text-ink">
        {t('admin.metrics.depsTitle')}
      </h2>
      <AdminCard className="p-5">
        <dl className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
          <OutcomeList icon={Server} label={t('admin.metrics.probes')} rows={dep.serviceProbes} number={number} />
          <OutcomeList icon={Layers} label={t('admin.metrics.translations')} rows={dep.translations} number={number} />
          <OutcomeList icon={Layers} label={t('admin.metrics.bookLookups')} rows={dep.bookSourceLookups} number={number} />
          <OutcomeList icon={ShieldCheck} label={t('admin.metrics.authEvents')} rows={dep.authEvents} number={number} />
          <div className="space-y-1">
            <dt className="type-note flex items-center gap-1.5 text-ink-soft">
              <Mail className="size-4 shrink-0 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
              <span>{t('admin.metrics.complaintMails')}</span>
            </dt>
            <dd className="type-body text-ink tabular-nums">
              {dep.complaintMails.total === 0 ? (
                '—'
              ) : (
                <span>
                  {t('admin.metrics.okFailSummary', {
                    ok: number.format(dep.complaintMails.ok),
                    failures: number.format(dep.complaintMails.failures),
                  })}
                  {dep.complaintMails.skipped > 0
                    ? ` · ${t('admin.metrics.mailSkipped', { count: number.format(dep.complaintMails.skipped) })}`
                    : ''}
                </span>
              )}
            </dd>
          </div>
          <div className="space-y-1">
            <dt className="type-note flex items-center gap-1.5 text-ink-soft">
              <AlertTriangle className="size-4 shrink-0 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
              <span>{t('admin.metrics.diagDropped')}</span>
            </dt>
            <dd
              className={`type-body-strong tabular-nums ${dep.diagnosticWritesDropped > 0 ? 'text-error' : 'text-ink'}`}
            >
              {number.format(dep.diagnosticWritesDropped)}
            </dd>
          </div>
        </dl>
      </AdminCard>
    </section>
  )
}

function OutcomeList({
  icon: Icon,
  label,
  rows,
  number,
}: {
  icon: LucideIcon
  label: string
  rows: OutcomeGroupRow[]
  number: Intl.NumberFormat
}) {
  const t = useT()
  return (
    <div className="space-y-1.5">
      <dt className="type-note flex items-center gap-1.5 text-ink-soft">
        <Icon className="size-4 shrink-0 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
        <span>{label}</span>
      </dt>
      <dd className="space-y-1">
        {rows.length === 0 ? (
          <span className="type-body text-ink-soft">—</span>
        ) : (
          rows.map((r) => (
            <div key={r.name} className="type-note flex items-center justify-between gap-2 text-ink tabular-nums">
              <span className="type-body-strong">{r.name}</span>
              <span className={r.failures > 0 ? 'text-error' : 'text-ink-soft'}>
                {t('admin.metrics.okFailSummary', {
                  ok: number.format(r.ok),
                  failures: number.format(r.failures),
                })}
              </span>
            </div>
          ))
        )}
      </dd>
    </div>
  )
}

function StatItem({
  label,
  value,
  tone = 'ink',
}: {
  label: string
  value: ReactNode
  tone?: 'ink' | 'ochra' | 'error'
}) {
  const color = tone === 'error' ? 'text-error' : tone === 'ochra' ? 'text-ochra' : 'text-ink'
  return (
    <div className="space-y-1">
      <dt className="type-note text-ink-soft">{label}</dt>
      <dd className={`type-h2 tabular-nums ${color}`}>{value}</dd>
    </div>
  )
}

function MetricsSkeleton() {
  const t = useT()
  return (
    <SkeletonRegion loadingLabel={t('common.loading')} className="space-y-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }, (_, i) => (
          <div key={i} className="space-y-2 rounded-sm border border-ink-faint bg-surface p-5">
            <Skeleton className="h-4 w-28" />
            <Skeleton className="h-9 w-16" />
          </div>
        ))}
      </div>
      <div className="space-y-4 rounded-sm border border-ink-faint bg-surface p-5">
        <div className="grid gap-4 sm:grid-cols-4">
          {Array.from({ length: 4 }, (_, i) => (
            <div key={i} className="space-y-2">
              <Skeleton className="h-4 w-20" />
              <Skeleton className="h-7 w-16" />
            </div>
          ))}
        </div>
        <Skeleton className="h-2.5 w-full" />
      </div>
    </SkeletonRegion>
  )
}
