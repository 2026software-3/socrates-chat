import { useState, type FormEvent } from 'react'
import { Link } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useClassroomsBase } from '@/features/classrooms/use-base'
import { api, isApiError } from '@/lib/api'
import { translateInBrowser } from '@/lib/browser-translator'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import type { Page as PageResult, SummaryTranslation, TeacherSummary } from '@/lib/types'
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

/** 一場對話的總結卡片；學生姓名預設連到該學生在這間教室的討論情形。 */
export function SummaryCard({ row, linkStudent = true }: { row: TeacherSummary; linkStudent?: boolean }) {
  const t = useT()
  const base = useClassroomsBase()
  const { lang } = useI18n()
  const [translation, setTranslation] = useState<SummaryTranslation | null>(null)
  const [showTranslation, setShowTranslation] = useState(false)
  const [translating, setTranslating] = useState(false)
  const [translationError, setTranslationError] = useState<unknown>(null)
  const shown = showTranslation && translation?.language === lang ? translation : null
  const visible = shown ?? row

  async function translate() {
    setTranslating(true)
    setTranslationError(null)
    try {
      const local = await translateInBrowser(row.language, lang, [row.stance ?? '', row.reasons ?? ''])
      const result = local
        ? { language: lang, stance: local[0], reasons: local[1] }
        : await api.post<SummaryTranslation>(`/api/summaries/${row.conversation_id}/translation`)
      setTranslation(result)
      setShowTranslation(true)
    } catch (error) {
      setTranslationError(error)
    } finally {
      setTranslating(false)
    }
  }

  return (
    <li>
      <Card>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
            <div className="min-w-0">
              <p className="font-medium break-words">
                {linkStudent ? (
                  <Link to={`${base}/${row.classroom_id}/students/${row.student_id}`} className="underline-offset-4 hover:underline">
                    {row.student_name ?? t('teacher.summaries.unnamed')}
                  </Link>
                ) : (
                  (row.student_name ?? t('teacher.summaries.unnamed'))
                )}
              </p>
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
          <dl className="space-y-2">
            <Field label={t('teacher.summaries.stance')} value={visible.stance} />
            <Field label={t('teacher.summaries.reasons')} value={visible.reasons} />
          </dl>
          {translationError ? (
            <p role="alert" className="text-destructive text-sm">
              {errorText(t, translationError)}
            </p>
          ) : null}
          {shown ? (
            <Button type="button" variant="outline" size="sm" onClick={() => setShowTranslation(false)}>
              {t('teacher.summaries.showOriginal')}
            </Button>
          ) : row.language !== lang ? (
            <Button type="button" variant="outline" size="sm" disabled={translating} onClick={() => void translate()}>
              {translating ? t('teacher.summaries.translating') : t('teacher.summaries.translate')}
            </Button>
          ) : null}
        </CardContent>
      </Card>
    </li>
  )
}

/** 教師：學生的 AI 總結列表。只有總結，沒有也不請求對話原文（S-02.1）。依後端的不透明游標前後翻頁；搜尋交給後端（`q`），涵蓋所有頁。 */
export function SummariesPage() {
  const t = useT()
  const [draft, setDraft] = useState('')
  const [search, setSearch] = useState('')
  const [cursors, setCursors] = useState<(string | null)[]>([null])
  const cursor = cursors[cursors.length - 1]

  // 換搜尋條件時游標失去意義，從第一頁開始
  function submit(e: FormEvent) {
    e.preventDefault()
    setSearch(draft.trim())
    setCursors([null])
  }

  return (
    <Page title={t('teacher.summaries.title')}>
      <Alert>
        <AlertDescription>{t('teacher.summaries.privacyNote')}</AlertDescription>
      </Alert>
      <form role="search" onSubmit={submit} className="flex max-w-sm items-end gap-2">
        <div className="flex-1 space-y-1.5">
          <Label htmlFor="summaries-search">{t('teacher.summaries.searchLabel')}</Label>
          <Input id="summaries-search" type="search" value={draft} onChange={(e) => setDraft(e.target.value)} />
        </div>
        <Button type="submit" variant="outline">
          {t('teacher.summaries.search')}
        </Button>
      </form>
      <SummaryList
        key={`${search}\n${cursor ?? ''}`}
        search={search}
        cursor={cursor}
        onNext={(next) => setCursors((previous) => [...previous, next])}
        onPrevious={cursors.length > 1 ? () => setCursors((previous) => previous.slice(0, -1)) : undefined}
        onRestart={() => setCursors([null])}
      />
    </Page>
  )
}

function summariesPath(search: string, cursor: string | null): string {
  const params = new URLSearchParams()
  if (search) params.set('q', search)
  if (cursor) params.set('cursor', cursor)
  const query = params.toString()
  return `/api/summaries${query ? `?${query}` : ''}`
}

function SummaryList({
  search,
  cursor,
  onNext,
  onPrevious,
  onRestart,
}: {
  search: string
  cursor: string | null
  onNext: (cursor: string) => void
  onPrevious?: () => void
  onRestart: () => void
}) {
  const t = useT()
  const summaries = useFetch<PageResult<TeacherSummary>>(summariesPath(search, cursor))
  const data = summaries.data?.items
  const nextCursor = summaries.data?.next_cursor
  // 失效的游標（例如部署後游標版本改變）重試也不會成功，改從第一頁開始
  const staleCursor = isApiError(summaries.error) && summaries.error.code === 'invalid_cursor'

  return (
    <>
      {summaries.loading && !data ? <LoadingBlock /> : null}
      {summaries.error ? <ErrorBlock error={summaries.error} onRetry={staleCursor ? onRestart : summaries.reload} /> : null}
      {data && data.length === 0 ? <EmptyBlock>{search ? t('teacher.summaries.noMatch') : t('teacher.summaries.empty')}</EmptyBlock> : null}
      {data && data.length > 0 ? (
        <ul className="space-y-3">
          {data.map((r) => (
            <SummaryCard key={r.conversation_id} row={r} />
          ))}
        </ul>
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
    </>
  )
}
