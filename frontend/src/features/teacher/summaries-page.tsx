import { useMemo, useState } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { FrameworkBadge } from '@/features/dashboard/framework-badge'
import { formatDate } from '@/features/teacher/format'
import { useI18n, useT } from '@/i18n'
import type { Page as PageResult, TeacherSummary } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

function Field({ label, value }: { label: string; value: string | null }) {
  const t = useT()
  return (
    <div>
      <dt className="text-muted-foreground text-xs font-medium">{label}</dt>
      <dd className="text-sm break-words whitespace-pre-line">{value ? value : t('teacher.summaries.none')}</dd>
    </div>
  )
}

function SummaryCard({ row }: { row: TeacherSummary }) {
  const t = useT()
  const { lang } = useI18n()
  return (
    <li>
      <Card>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
            <div className="min-w-0">
              <p className="font-medium break-words">{row.student_name ?? t('teacher.summaries.unnamed')}</p>
              <p className="text-muted-foreground text-sm break-all">{row.student_email}</p>
            </div>
            <div className="sm:text-right">
              <p className="text-sm font-medium break-words">{row.title}</p>
              {row.ended_at ? (
                <p className="text-muted-foreground text-xs">
                  {t('teacher.summaries.endedAt', {
                    date: formatDate(row.ended_at, lang),
                  })}
                </p>
              ) : null}
            </div>
          </div>
          {row.claim ? <p className="text-sm font-medium break-words">{row.claim}</p> : null}
          <FrameworkBadge framework={row.framework} />
          <dl className="space-y-2">
            <Field label={t('teacher.summaries.stance')} value={row.stance} />
            <Field label={t('teacher.summaries.reasons')} value={row.reasons} />
          </dl>
        </CardContent>
      </Card>
    </li>
  )
}

/** 教師：學生的 AI 總結列表。只有總結，沒有也不請求對話原文（S-02.1）。依後端的不透明游標前後翻頁。 */
export function SummariesPage() {
  const [cursors, setCursors] = useState<(string | null)[]>([null])
  const cursor = cursors[cursors.length - 1]
  return (
    <SummaryList
      key={cursor ?? ''}
      cursor={cursor}
      onNext={(next) => setCursors((previous) => [...previous, next])}
      onPrevious={cursors.length > 1 ? () => setCursors((previous) => previous.slice(0, -1)) : undefined}
    />
  )
}

function SummaryList({ cursor, onNext, onPrevious }: { cursor: string | null; onNext: (cursor: string) => void; onPrevious?: () => void }) {
  const t = useT()
  const summaries = useFetch<PageResult<TeacherSummary>>(`/api/summaries${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`)
  const [query, setQuery] = useState('')
  const data = summaries.data?.items
  const nextCursor = summaries.data?.next_cursor

  // 搜尋只套用在目前這一頁
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return data ?? []
    return (data ?? []).filter((r) => [r.student_name ?? '', r.student_email, r.title].some((s) => s.toLowerCase().includes(q)))
  }, [data, query])

  return (
    <Page title={t('teacher.summaries.title')}>
      <Alert>
        <AlertDescription>{t('teacher.summaries.privacyNote')}</AlertDescription>
      </Alert>
      {summaries.loading && !data ? <LoadingBlock /> : null}
      {summaries.error ? <ErrorBlock error={summaries.error} onRetry={summaries.reload} /> : null}
      {data && data.length === 0 ? <EmptyBlock>{t('teacher.summaries.empty')}</EmptyBlock> : null}
      {data && data.length > 0 ? (
        <>
          <div className="max-w-sm space-y-1.5">
            <Label htmlFor="summaries-search">{t('teacher.summaries.searchLabel')}</Label>
            <Input id="summaries-search" type="search" value={query} onChange={(e) => setQuery(e.target.value)} aria-describedby="summaries-search-scope" />
            <p id="summaries-search-scope" className="text-muted-foreground text-xs">
              {t('teacher.summaries.searchScope')}
            </p>
          </div>
          {filtered.length === 0 ? (
            <EmptyBlock>{t('teacher.summaries.noMatch')}</EmptyBlock>
          ) : (
            <ul className="space-y-3">
              {filtered.map((r) => (
                <SummaryCard key={r.conversation_id} row={r} />
              ))}
            </ul>
          )}
        </>
      ) : null}
      {onPrevious || nextCursor ? (
        <div className="flex gap-2">
          {onPrevious ? (
            <Button variant="outline" onClick={onPrevious}>
              {t('teacher.summaries.previous')}
            </Button>
          ) : null}
          {nextCursor ? (
            <Button variant="outline" disabled={summaries.loading} onClick={() => onNext(nextCursor)}>
              {t('teacher.summaries.next')}
            </Button>
          ) : null}
        </div>
      ) : null}
    </Page>
  )
}
