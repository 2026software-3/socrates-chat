import { Fragment, useEffect, useMemo, useState, type FormEvent } from 'react'
import { useSearchParams } from 'react-router'
import { LoadMore } from '@/components/load-more'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
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
import { useI18n, useT } from '@/i18n'
import type { Page as PageResult } from '@/lib/types'
import { usePagedFetch } from '@/lib/use-fetch'

const PAGE_SIZE = 50

const selectClass =
  'border-input bg-background h-9 w-full rounded-md border px-3 text-sm shadow-xs focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none'

/** 表頭固定在捲動區頂端，捲動長清單時仍看得到欄位名稱。 */
const STICKY_HEAD = 'sticky top-0 z-10 bg-background'

/** 網址上的篩選用 RFC 3339（UTC）；表單裡的時間是本地時間。 */
function toDraft(applied: Filters): Filters {
  return { ...applied, from: isoToLocal(applied.from), to: isoToLocal(applied.to) }
}

function statusVariant(status: number): 'destructive' | 'outline' | 'secondary' {
  if (status >= 500) return 'destructive'
  if (status >= 400) return 'outline'
  return 'secondary'
}

/** 管理者診斷頁：依時間、類型、失敗分類與 request ID 查詢最近 30 天的失敗事件（S-09.3）。 */
export function DiagnosticsPage() {
  const t = useT()
  const { lang } = useI18n()
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
    <Page title={t('admin.diag.title')}>
      <div className="text-muted-foreground space-y-1 text-sm">
        <p>{t('admin.diag.intro')}</p>
        <p>{t('admin.diag.lookup')}</p>
      </div>

      <form onSubmit={submit} className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3" noValidate>
        <div className="space-y-1.5">
          <Label htmlFor="diag-from">{t('admin.diag.from')}</Label>
          <Input
            id="diag-from"
            type="datetime-local"
            value={draft.from}
            onChange={(e) => setDraft({ ...draft, from: e.target.value })}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="diag-to">{t('admin.diag.to')}</Label>
          <Input
            id="diag-to"
            type="datetime-local"
            value={draft.to}
            onChange={(e) => setDraft({ ...draft, to: e.target.value })}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="diag-kind">{t('admin.diag.kind')}</Label>
          <select
            id="diag-kind"
            className={selectClass}
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
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="diag-class">{t('admin.diag.failureClass')}</Label>
          <select
            id="diag-class"
            className={selectClass}
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
        </div>
        <div className="space-y-1.5 sm:col-span-2 lg:col-span-1">
          <Label htmlFor="diag-request">{t('admin.diag.requestId')}</Label>
          <Input
            id="diag-request"
            value={draft.request_id}
            autoComplete="off"
            spellCheck={false}
            placeholder="00000000-0000-4000-8000-000000000000"
            aria-invalid={formError === 'requestId'}
            onChange={(e) => setDraft({ ...draft, request_id: e.target.value })}
          />
        </div>
        <div className="flex items-end gap-2">
          <Button type="submit">{t('admin.diag.apply')}</Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => {
              setDraft(EMPTY_FILTERS)
              setFormError(undefined)
              apply(EMPTY_FILTERS)
            }}
          >
            {t('admin.diag.reset')}
          </Button>
        </div>
      </form>

      {formError ? (
        <Alert variant="destructive" role="alert">
          <AlertDescription>
            {formError === 'requestId' ? t('admin.diag.invalidRequestId') : t('admin.diag.invalidRange')}
          </AlertDescription>
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

      {list.loading && !list.items ? <LoadingBlock /> : null}
      {list.error ? <ErrorBlock error={list.error} onRetry={list.reload} /> : null}
      {list.items?.length === 0 ? (
        <EmptyBlock>{filtered ? t('admin.diag.emptyFiltered') : t('admin.diag.empty')}</EmptyBlock>
      ) : null}
      {list.items?.length ? (
        <ScrollArea type="auto" className="max-h-[65vh] rounded-md border">
          <Table aria-label={t('admin.diag.table')}>
            <TableHeader>
              <TableRow>
                <TableHead className={STICKY_HEAD}>{t('admin.diag.time')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.diag.kind')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.diag.failureClass')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.diag.function')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.diag.status')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.diag.requestId')}</TableHead>
                <TableHead className={STICKY_HEAD}>
                  <span className="sr-only">{t('admin.diag.details')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.items.map((event, index) => {
                const key = `${event.request_id}-${event.occurred_at}-${index}`
                const open = expanded === key
                return (
                  <Fragment key={key}>
                    <TableRow>
                      <TableCell>
                        <time dateTime={event.occurred_at}>{date.format(new Date(event.occurred_at))}</time>
                      </TableCell>
                      <TableCell>{t(KIND_LABEL[event.kind])}</TableCell>
                      <TableCell>
                        <Badge variant="outline">{t(CLASS_LABEL[event.failure_class])}</Badge>
                      </TableCell>
                      <TableCell>{functionName(t, event.function)}</TableCell>
                      <TableCell>
                        <Badge variant={statusVariant(event.status)}>{event.status}</Badge>
                      </TableCell>
                      <TableCell>
                        <code>{event.request_id}</code>
                      </TableCell>
                      <TableCell>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          aria-expanded={open}
                          onClick={() => setExpanded(open ? undefined : key)}
                        >
                          {open ? t('admin.diag.collapse') : t('admin.diag.details')}
                        </Button>
                      </TableCell>
                    </TableRow>
                    {open ? (
                      <TableRow>
                        <TableCell colSpan={7} className="bg-muted/40">
                          <EventDetails event={event} onFilter={(next) => apply({ ...applied, ...next })} />
                        </TableCell>
                      </TableRow>
                    ) : null}
                  </Fragment>
                )
              })}
            </TableBody>
          </Table>
        </ScrollArea>
      ) : null}
      <LoadMore hasMore={list.hasMore} loading={list.loadingMore} error={list.moreError} onLoadMore={list.loadMore} />
    </Page>
  )
}

function functionName(t: ReturnType<typeof useT>, code: string): string {
  const key = FUNCTION_LABEL[code]
  return key ? t(key) : code
}

function RelatedChip({ label, clear, onClear }: { label: string; clear: string; onClear: () => void }) {
  return (
    <span className="bg-muted inline-flex items-center gap-2 rounded-md px-2 py-1 text-sm">
      <span>{label}</span>
      <Button type="button" variant="ghost" size="sm" onClick={onClear}>
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
  const rows: [string, React.ReactNode][] = [
    [t('admin.diag.route'), <code key="r">{event.route}</code>],
    [t('admin.diag.errorCode'), <code key="e">{event.error_code}</code>],
    [t('admin.diag.duration'), event.duration_ms === null ? none : `${event.duration_ms} ms`],
    [t('admin.diag.retries'), event.retry_count === null ? none : String(event.retry_count)],
    [t('admin.diag.model'), event.model ? <code key="m">{event.model}</code> : none],
    [t('admin.diag.userId'), event.user_id ? <code key="u">{event.user_id}</code> : t('admin.diag.noAccount')],
    [t('admin.diag.classroomId'), event.classroom_id ? <code key="c">{event.classroom_id}</code> : none],
  ]
  return (
    <div className="space-y-3">
      <dl className="grid gap-x-6 gap-y-1 text-sm sm:grid-cols-2">
        {rows.map(([label, value]) => (
          <div key={label} className="flex gap-2">
            <dt className="text-muted-foreground shrink-0">{label}</dt>
            <dd className="min-w-0 break-all">{value}</dd>
          </div>
        ))}
      </dl>
      <div className="flex flex-wrap gap-2">
        {event.user_id ? (
          <Button type="button" variant="outline" size="sm" onClick={() => onFilter({ user_id: event.user_id ?? '' })}>
            {t('admin.diag.sameAccount')}
          </Button>
        ) : null}
        {event.classroom_id ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onFilter({ classroom_id: event.classroom_id ?? '' })}
          >
            {t('admin.diag.sameClassroom')}
          </Button>
        ) : null}
        <Button type="button" variant="outline" size="sm" onClick={() => onFilter({ request_id: event.request_id })}>
          {t('admin.diag.sameRequest')}
        </Button>
      </div>
    </div>
  )
}
