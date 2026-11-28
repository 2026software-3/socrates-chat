import { Fragment, useEffect, useMemo, useState, type FormEvent } from 'react'
import { useSearchParams } from 'react-router'
import {
  Activity,
  AlertCircle,
  ChevronDown,
  Clock,
  Cpu,
  Eye,
  Hash,
  Search,
  Tag,
  X,
} from 'lucide-react'
import { PageHeader } from '@/components/page'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { Field } from '@/components/vedute/field'
import { Meta } from '@/components/vedute/meta'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import {
  AdminCard,
  AdminEmptyState,
  AdminErrorAlert,
  AdminStatusBadge,
  type AdminTone,
} from '@/features/admin/admin-ui'
import {
  ALL_CLASSES,
  CLASSES_BY_KIND,
  CLASS_LABEL,
  EMPTY_FILTERS,
  FUNCTION_LABEL,
  KINDS,
  KIND_LABEL,
  filtersFromParams,
  filtersToParams,
  isUuid,
  isoToLocal,
  localToIso,
  type DiagnosticEvent,
  type Filters,
} from '@/features/admin/diagnostics-model'
import { useMediaQuery } from '@/hooks/use-media-query'
import { useI18n, useT } from '@/i18n'
import type { Page as PageResult } from '@/lib/types'
import { usePagedFetch } from '@/lib/use-fetch'

const PAGE_SIZE = 50

/** 網址上的篩選用 RFC 3339（UTC）；表單裡的時間是本地時間。 */
function toDraft(applied: Filters): Filters {
  return { ...applied, from: isoToLocal(applied.from), to: isoToLocal(applied.to) }
}

function statusTone(status: number): AdminTone {
  if (status >= 500) return 'error'
  if (status >= 400) return 'draft'
  return 'active'
}

/** 管理者診斷頁：依時間、類型、失敗分類與 request ID 查詢最近 30 天的失敗事件（S-09.3）。 */
export function DiagnosticsPage() {
  const t = useT()
  const { lang } = useI18n()
  const isDesktop = useMediaQuery('(min-width: 1024px)', true)
  const [params, setParams] = useSearchParams()
  const applied = useMemo(() => filtersFromParams(params), [params])
  const [draft, setDraft] = useState<Filters>(() => toDraft(applied))
  const [formError, setFormError] = useState<'requestId' | 'range'>()
  const [expanded, setExpanded] = useState<string>()
  const date = useMemo(
    () => new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'medium' }),
    [lang],
  )

  // 從別的頁面（教室、帳號、概況）連過來或按上一頁時，表單跟著網址走
  useEffect(() => {
    setDraft(toDraft(applied))
    setFormError(undefined)
    setExpanded(undefined)
  }, [applied])

  const query = filtersToParams(applied)
  query.set('limit', String(PAGE_SIZE))
  const list = usePagedFetch<DiagnosticEvent, PageResult<DiagnosticEvent>>(
    `/api/admin/diagnostics?${query.toString()}`,
  )

  const apply = (next: Filters) => setParams(filtersToParams(next))

  const submit = (e: FormEvent) => {
    e.preventDefault()
    const requestId = draft.request_id.trim()
    if (requestId && !isUuid(requestId)) return setFormError('requestId')
    const from = localToIso(draft.from)
    const to = localToIso(draft.to)
    if (from && to && new Date(from) >= new Date(to)) return setFormError('range')
    setFormError(undefined)
    apply({ ...draft, request_id: requestId, from: from ?? '', to: to ?? '' })
  }

  const classOptions = draft.kind ? CLASSES_BY_KIND[draft.kind] : ALL_CLASSES
  const filtered = Object.values(applied).some(Boolean)

  return (
    <div className="space-y-6">
      <PageHeader title={t('admin.diag.title')} description={t('admin.diag.intro')} />

      <p className="type-note border-l border-caeruleum py-1 pl-3 text-ink-soft">{t('admin.diag.lookup')}</p>

      <AdminCard className="p-5">
        <form onSubmit={submit} className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3" noValidate>
          <Field
            id="diag-from"
            label={t('admin.diag.from')}
            type="datetime-local"
            value={draft.from}
            aria-invalid={formError === 'range' || undefined}
            onChange={(e) => setDraft({ ...draft, from: e.target.value })}
          />
          <Field
            id="diag-to"
            label={t('admin.diag.to')}
            type="datetime-local"
            value={draft.to}
            aria-invalid={formError === 'range' || undefined}
            onChange={(e) => setDraft({ ...draft, to: e.target.value })}
          />
          <div className="grid gap-1">
            <label htmlFor="diag-kind" className="type-note text-ink-soft">
              {t('admin.diag.kind')}
            </label>
            <div className="relative">
              <select
                id="diag-kind"
                className="vedute-input appearance-none pr-8"
                value={draft.kind}
                onChange={(e) => {
                  const kind = e.target.value as Filters['kind']
                  const keep = kind === '' || CLASSES_BY_KIND[kind].includes(draft.failure_class as never)
                  setDraft({ ...draft, kind, failure_class: keep ? draft.failure_class : '' })
                }}
              >
                <option value="">{t('admin.diag.any')}</option>
                {KINDS.map((kind) => (
                  <option key={kind} value={kind}>
                    {t(KIND_LABEL[kind])}
                  </option>
                ))}
              </select>
              <ChevronDown
                className="pointer-events-none absolute right-0 top-1/2 size-4 -translate-y-1/2 text-ink-soft"
                strokeWidth={1.5}
                aria-hidden="true"
              />
            </div>
          </div>
          <div className="grid gap-1">
            <label htmlFor="diag-class" className="type-note text-ink-soft">
              {t('admin.diag.failureClass')}
            </label>
            <div className="relative">
              <select
                id="diag-class"
                className="vedute-input appearance-none pr-8"
                value={draft.failure_class}
                onChange={(e) => setDraft({ ...draft, failure_class: e.target.value as Filters['failure_class'] })}
              >
                <option value="">{t('admin.diag.any')}</option>
                {classOptions.map((c) => (
                  <option key={c} value={c}>
                    {t(CLASS_LABEL[c])}
                  </option>
                ))}
              </select>
              <ChevronDown
                className="pointer-events-none absolute right-0 top-1/2 size-4 -translate-y-1/2 text-ink-soft"
                strokeWidth={1.5}
                aria-hidden="true"
              />
            </div>
          </div>
          <div className="sm:col-span-2 lg:col-span-1">
            <Field
              id="diag-request"
              label={t('admin.diag.requestId')}
              value={draft.request_id}
              autoComplete="off"
              spellCheck={false}
              placeholder="00000000-0000-4000-8000-000000000000"
              aria-invalid={formError === 'requestId' || undefined}
              onChange={(e) => setDraft({ ...draft, request_id: e.target.value })}
            />
          </div>
          <div className="flex flex-wrap items-end gap-2">
            <Button type="submit" variant="primary">
              <Search className="size-4" strokeWidth={1.5} aria-hidden="true" />
              {t('admin.diag.apply')}
            </Button>
            <Button
              type="button"
              variant="quiet"
              onClick={() => {
                setDraft(EMPTY_FILTERS)
                setFormError(undefined)
                apply(EMPTY_FILTERS)
              }}
            >
              <X className="size-4" strokeWidth={1.5} aria-hidden="true" />
              {t('admin.diag.reset')}
            </Button>
          </div>
        </form>
      </AdminCard>

      {formError ? (
        <Alert kind="error">
          {formError === 'requestId' ? t('admin.diag.invalidRequestId') : t('admin.diag.invalidRange')}
        </Alert>
      ) : null}

      {applied.user_id || applied.classroom_id ? (
        <div className="flex flex-wrap gap-2" aria-label={t('admin.diag.relatedFilters')}>
          {applied.user_id ? (
            <RelatedChip
              label={t('admin.diag.userFilter', { id: applied.user_id })}
              clear={t('admin.diag.clearFilter')}
              onClear={() => apply({ ...applied, user_id: '' })}
            />
          ) : null}
          {applied.classroom_id ? (
            <RelatedChip
              label={t('admin.diag.classroomFilter', { id: applied.classroom_id })}
              clear={t('admin.diag.clearFilter')}
              onClear={() => apply({ ...applied, classroom_id: '' })}
            />
          ) : null}
        </div>
      ) : null}

      {list.loading && !list.items ? <DiagnosticsSkeleton isDesktop={isDesktop} /> : null}
      {list.error ? <AdminErrorAlert error={list.error} onRetry={list.reload} /> : null}
      {list.items?.length === 0 ? (
        <AdminEmptyState>{filtered ? t('admin.diag.emptyFiltered') : t('admin.diag.empty')}</AdminEmptyState>
      ) : null}

      {list.items?.length ? (
        isDesktop ? (
          <div className="overflow-hidden rounded-sm border border-ink-faint bg-surface">
            <table aria-label={t('admin.diag.table')} className="w-full border-collapse text-left">
              <thead>
                <tr className="border-b border-ink-faint bg-paper-deep">
                  <th scope="col" className="type-body-strong px-5 py-3 text-ink">
                    <span className="inline-flex items-center gap-1.5">
                      <Clock className="size-4 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                      {t('admin.diag.time')}
                    </span>
                  </th>
                  <th scope="col" className="type-body-strong px-4 py-3 text-ink">
                    <span className="inline-flex items-center gap-1.5">
                      <Tag className="size-4 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                      {t('admin.diag.kind')}
                    </span>
                  </th>
                  <th scope="col" className="type-body-strong px-4 py-3 text-ink">
                    <span className="inline-flex items-center gap-1.5">
                      <AlertCircle className="size-4 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                      {t('admin.diag.failureClass')}
                    </span>
                  </th>
                  <th scope="col" className="type-body-strong px-4 py-3 text-ink">
                    <span className="inline-flex items-center gap-1.5">
                      <Cpu className="size-4 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                      {t('admin.diag.function')}
                    </span>
                  </th>
                  <th scope="col" className="type-body-strong px-4 py-3 text-ink">
                    <span className="inline-flex items-center gap-1.5">
                      <Activity className="size-4 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                      {t('admin.diag.status')}
                    </span>
                  </th>
                  <th scope="col" className="type-body-strong px-4 py-3 text-ink">
                    <span className="inline-flex items-center gap-1.5">
                      <Hash className="size-4 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                      {t('admin.diag.requestId')}
                    </span>
                  </th>
                  <th scope="col" className="px-4 py-3">
                    <span className="sr-only">{t('admin.diag.details')}</span>
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-ink-faint">
                {list.items.map((event, index) => {
                  const key = `${event.request_id}-${event.occurred_at}-${index}`
                  const open = expanded === key
                  return (
                    <Fragment key={key}>
                      <tr className="align-middle">
                        <td className="type-body px-5 py-3 text-ink whitespace-nowrap">
                          <time dateTime={event.occurred_at}>{date.format(new Date(event.occurred_at))}</time>
                        </td>
                        <td className="type-body-strong px-4 py-3 text-ink whitespace-nowrap">
                          {t(KIND_LABEL[event.kind])}
                        </td>
                        <td className="px-4 py-3">
                          <AdminStatusBadge tone={event.failure_class === 'invalid_output' ? 'draft' : 'error'}>
                            {t(CLASS_LABEL[event.failure_class])}
                          </AdminStatusBadge>
                        </td>
                        <td className="type-body px-4 py-3 text-ink">{functionName(t, event.function)}</td>
                        <td className="px-4 py-3">
                          <AdminStatusBadge tone={statusTone(event.status)}>{String(event.status)}</AdminStatusBadge>
                        </td>
                        <td className="type-note max-w-44 break-all px-4 py-3 text-ink-soft tabular-nums">
                          {event.request_id}
                        </td>
                        <td className="px-4 py-3 text-right">
                          <Button
                            type="button"
                            variant="ghost"
                            aria-expanded={open}
                            onClick={() => setExpanded(open ? undefined : key)}
                          >
                            <Eye className="size-4" strokeWidth={1.5} aria-hidden="true" />
                            {open ? t('admin.diag.collapse') : t('admin.diag.details')}
                          </Button>
                        </td>
                      </tr>
                      {open ? (
                        <tr className="bg-paper/45">
                          <td colSpan={7} className="px-5 py-4">
                            <EventDetails event={event} onFilter={(next) => apply({ ...applied, ...next })} />
                          </td>
                        </tr>
                      ) : null}
                    </Fragment>
                  )
                })}
              </tbody>
            </table>
          </div>
        ) : (
          <ul aria-label={t('admin.diag.table')} className="space-y-3">
            {list.items.map((event, index) => {
              const key = `${event.request_id}-${event.occurred_at}-${index}`
              const open = expanded === key
              return (
                <li key={key}>
                  <AdminCard className="space-y-3 p-4">
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <h2 className="type-h3 text-ink">{functionName(t, event.function)}</h2>
                      <div className="flex flex-wrap items-center gap-1.5">
                        <AdminStatusBadge tone={event.failure_class === 'invalid_output' ? 'draft' : 'error'}>
                          {t(CLASS_LABEL[event.failure_class])}
                        </AdminStatusBadge>
                        <AdminStatusBadge tone={statusTone(event.status)}>{String(event.status)}</AdminStatusBadge>
                      </div>
                    </div>
                    <div className="grid gap-1.5">
                      <Meta
                        icon={Clock}
                        label={t('admin.diag.time')}
                        value={<time dateTime={event.occurred_at}>{date.format(new Date(event.occurred_at))}</time>}
                      />
                      <Meta icon={Tag} label={t('admin.diag.kind')} value={t(KIND_LABEL[event.kind])} />
                      <Meta
                        icon={Hash}
                        label={t('admin.diag.requestId')}
                        value={<span className="break-all tabular-nums">{event.request_id}</span>}
                      />
                    </div>
                    {open ? (
                      <div className="border-t border-ink-faint pt-3">
                        <EventDetails event={event} onFilter={(next) => apply({ ...applied, ...next })} />
                      </div>
                    ) : null}
                    <div className="border-t border-ink-faint pt-2">
                      <Button
                        type="button"
                        variant="quiet"
                        className="w-full"
                        aria-expanded={open}
                        onClick={() => setExpanded(open ? undefined : key)}
                      >
                        <Eye className="size-4" strokeWidth={1.5} aria-hidden="true" />
                        {open ? t('admin.diag.collapse') : t('admin.diag.details')}
                      </Button>
                    </div>
                  </AdminCard>
                </li>
              )
            })}
          </ul>
        )
      ) : null}

      {list.moreError ? (
        <AdminErrorAlert error={list.moreError} onRetry={list.loadMore} />
      ) : list.hasMore ? (
        <div>
          <Button type="button" variant="quiet" disabled={list.loadingMore} onClick={list.loadMore}>
            {list.loadingMore ? t('common.loadingMore') : t('common.loadMore')}
          </Button>
        </div>
      ) : null}
    </div>
  )
}

function DiagnosticsSkeleton({ isDesktop }: { isDesktop: boolean }) {
  const t = useT()
  return (
    <SkeletonRegion loadingLabel={t('common.loading')}>
      {isDesktop ? (
        <div className="overflow-hidden rounded-sm border border-ink-faint bg-surface">
          <div className="h-12 bg-paper-deep" />
          <div className="divide-y divide-ink-faint">
            {Array.from({ length: 5 }, (_, i) => (
              <div key={i} className="grid grid-cols-6 gap-4 px-5 py-3.5">
                <Skeleton className="h-5 w-28" />
                <Skeleton className="h-5 w-14" />
                <Skeleton className="h-5 w-20" />
                <Skeleton className="h-5 w-24" />
                <Skeleton className="h-5 w-12" />
                <Skeleton className="h-5 w-32" />
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          {Array.from({ length: 3 }, (_, i) => (
            <div key={i} className="space-y-3 rounded-sm border border-ink-faint bg-surface p-4">
              <div className="flex justify-between gap-2">
                <Skeleton className="h-6 w-28" />
                <Skeleton className="h-5 w-16" />
              </div>
              <Skeleton className="h-4 w-3/4" />
              <Skeleton className="h-4 w-2/3" />
              <Skeleton className="h-11 w-full" />
            </div>
          ))}
        </div>
      )}
    </SkeletonRegion>
  )
}

function functionName(t: ReturnType<typeof useT>, code: string): string {
  const key = FUNCTION_LABEL[code]
  return key ? t(key) : code
}

function RelatedChip({ label, clear, onClear }: { label: string; clear: string; onClear: () => void }) {
  return (
    <span className="inline-flex items-center gap-2 rounded-sm border border-ink-faint bg-paper px-3 py-1">
      <span className="type-note text-ink break-all">{label}</span>
      <Button type="button" variant="ghost" onClick={onClear}>
        <X className="size-4" strokeWidth={1.5} aria-hidden="true" />
        {clear}
      </Button>
    </span>
  )
}

/** 單筆事件的完整欄位。只有分類、固定代碼與內部識別碼，沒有電子郵件、學號或對話內容。 */
function EventDetails({
  event,
  onFilter,
}: {
  event: DiagnosticEvent
  onFilter: (next: Partial<Filters>) => void
}) {
  const t = useT()
  const none = '—'
  const rows: [string, string][] = [
    [t('admin.diag.route'), event.route],
    [t('admin.diag.errorCode'), event.error_code],
    [t('admin.diag.duration'), event.duration_ms === null ? none : `${event.duration_ms} ms`],
    [t('admin.diag.retries'), event.retry_count === null ? none : String(event.retry_count)],
    [t('admin.diag.model'), event.model ?? none],
    [t('admin.diag.userId'), event.user_id ?? t('admin.diag.noAccount')],
    [t('admin.diag.classroomId'), event.classroom_id ?? none],
  ]
  return (
    <div className="space-y-4">
      <dl className="grid gap-x-6 gap-y-2 sm:grid-cols-2">
        {rows.map(([label, value]) => (
          <div key={label} className="flex gap-2">
            <dt className="type-note shrink-0 text-ink-soft">{label}</dt>
            <dd className="type-note min-w-0 break-all text-ink tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      <div className="flex flex-wrap gap-2">
        {event.user_id ? (
          <Button type="button" variant="quiet" onClick={() => onFilter({ user_id: event.user_id ?? '' })}>
            {t('admin.diag.sameAccount')}
          </Button>
        ) : null}
        {event.classroom_id ? (
          <Button
            type="button"
            variant="quiet"
            onClick={() => onFilter({ classroom_id: event.classroom_id ?? '' })}
          >
            {t('admin.diag.sameClassroom')}
          </Button>
        ) : null}
        <Button type="button" variant="quiet" onClick={() => onFilter({ request_id: event.request_id })}>
          {t('admin.diag.sameRequest')}
        </Button>
      </div>
    </div>
  )
}
