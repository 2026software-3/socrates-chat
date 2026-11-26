import { useState, type FormEvent } from 'react'
import { Link } from 'react-router'
import { useAccountLanguage } from '@/auth/auth-context'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useClassroomsBase } from '@/features/classrooms/use-base'
import { api, isApiError, isInvalidCursor } from '@/lib/api'
import { translateInBrowser } from '@/lib/browser-translator'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import type { Classroom, Page as PageResult, StudentPage, SummaryTranslation, TeacherSummary, Topic } from '@/lib/types'
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
  const [showOriginal, setShowOriginal] = useState(false)
  const [translating, setTranslating] = useState(false)
  const [translationError, setTranslationError] = useState<unknown>(null)
  const saved = row.translation?.language === lang ? row.translation : null
  const current = saved ?? (translation?.language === lang ? translation : null)
  const shown = showOriginal ? null : current
  const visible = shown ?? row
  const masked = row.stance === 'message' && row.reasons === 'message'

  async function translate() {
    setTranslating(true)
    setTranslationError(null)
    try {
      const local = await translateInBrowser(row.language, lang, [row.stance ?? '', row.reasons ?? ''])
      const result = local
        ? { language: lang, stance: local[0], reasons: local[1] }
        : await api.post<SummaryTranslation>(`/api/summaries/${row.conversation_id}/translation`)
      setTranslation(result)
      setShowOriginal(false)
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
                  <Link to={`${base}/${row.classroom_id}/students/${row.student_user_id}`} className="underline-offset-4 hover:underline">
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
          {row.translation_pending && !current ? (
            <p role="status" className="text-muted-foreground text-sm">{t('chat.summary.translationPending')}</p>
          ) : null}
          <Link
            to={`${base}/${row.classroom_id}/conversations/${row.conversation_id}`}
            className="text-sm underline-offset-4 hover:underline"
          >
            {t('teacher.summaries.viewTranscript')}
          </Link>
          {translationError ? (
            <p role="alert" className="text-destructive text-sm">
              {errorText(t, translationError)}
            </p>
          ) : null}
          {shown ? (
            <Button type="button" variant="outline" size="sm" onClick={() => setShowOriginal(true)}>
              {t('teacher.summaries.showOriginal')}
            </Button>
          ) : row.language !== lang && !masked ? (
            <Button type="button" variant="outline" size="sm" disabled={translating} onClick={() => current ? setShowOriginal(false) : void translate()}>
              {translating ? t('teacher.summaries.translating') : t('teacher.summaries.translate')}
            </Button>
          ) : null}
        </CardContent>
      </Card>
    </li>
  )
}

/** 篩選條件：`student` 是學生的使用者 UUID（不是電子郵件或學號），`topic` 是題目標題（後端完全相符、不分大小寫）。空字串代表不篩選。 */
type Filters = { classroom: string; student: string; topic: string; search: string }

const NO_FILTERS: Filters = { classroom: '', student: '', topic: '', search: '' }

const selectClass =
  'border-input bg-background h-9 w-full rounded-md border px-3 text-sm shadow-xs focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none disabled:bg-transparent disabled:border-ink-soft disabled:text-ink-soft disabled:cursor-not-allowed'

/** 教師：學生的 AI 總結列表。列表只載入總結，對話原文要點進去才請求（S-02.1）。依後端的不透明游標前後翻頁；搜尋（`q`）與教室、學生、題目篩選都交給後端，涵蓋所有頁。 */
export function SummariesPage() {
  const t = useT()
  const accountLanguage = useAccountLanguage()
  const [draft, setDraft] = useState('')
  const [filters, setFilters] = useState<Filters>(NO_FILTERS)
  const [cursors, setCursors] = useState<(string | null)[]>([null])
  const cursor = cursors[cursors.length - 1]
  const filtered = Object.values(filters).some(Boolean)

  // 換條件時游標失去意義（後端也會拒絕沿用舊游標），從第一頁開始
  function apply(next: Partial<Filters>) {
    setFilters((previous) => ({ ...previous, ...next }))
    setCursors([null])
  }

  function clear() {
    setFilters(NO_FILTERS)
    setDraft('')
    setCursors([null])
  }

  function submit(e: FormEvent) {
    e.preventDefault()
    apply({ search: draft.trim() })
  }

  return (
    <Page title={t('teacher.summaries.title')}>
      <Alert>
        <AlertDescription>{t('teacher.summaries.privacyNote')}</AlertDescription>
      </Alert>
      <SummaryFilters filters={filters} onChange={apply} />
      <div className="flex flex-wrap items-end gap-2">
        <form role="search" onSubmit={submit} className="flex w-full max-w-sm items-end gap-2">
          <div className="flex-1 space-y-1.5">
            <Label htmlFor="summaries-search">{t('teacher.summaries.searchLabel')}</Label>
            <Input id="summaries-search" type="search" value={draft} onChange={(e) => setDraft(e.target.value)} />
          </div>
          <Button type="submit" variant="outline">
            {t('teacher.summaries.search')}
          </Button>
        </form>
        {filtered ? (
          <Button type="button" variant="ghost" onClick={clear}>
            {t('teacher.summaries.clearFilters')}
          </Button>
        ) : null}
      </div>
      {accountLanguage ? (
        <SummaryList
          key={`${JSON.stringify(filters)}\n${cursor ?? ''}\n${accountLanguage}`}
          filters={filters}
          cursor={cursor}
          onNext={(next) => setCursors((previous) => [...previous, next])}
          onPrevious={cursors.length > 1 ? () => setCursors((previous) => previous.slice(0, -1)) : undefined}
          onRestart={() => setCursors([null])}
        />
      ) : <LoadingBlock />}
    </Page>
  )
}

/**
 * 教室、學生、題目的篩選選單。學生選項來自所選教室的名單（`/api/classrooms/{id}/students`，一頁最多 200 人），
 * 所以要先選教室；還沒有帳號的學生沒有使用者 UUID，不列入。題目選項是題庫標題。選項載入失敗時只剩「全部」。
 */
function SummaryFilters({ filters, onChange }: { filters: Filters; onChange: (next: Partial<Filters>) => void }) {
  const t = useT()
  const classrooms = useFetch<Classroom[]>('/api/classrooms')
  const topics = useFetch<Topic[]>('/api/topics')
  const students = useFetch<StudentPage>(
    filters.classroom ? `/api/classrooms/${encodeURIComponent(filters.classroom)}/students?limit=200` : null,
  )
  const topicTitles = [...new Set((topics.data ?? []).map((topic) => topic.title))]
  const studentOptions = filters.classroom
    ? (students.data?.items ?? []).flatMap((s) => (s.id ? [{ id: s.id, label: `${s.display_name ?? s.email ?? s.student_number}（${s.student_number}）` }] : []))
    : []

  return (
    <div className="grid gap-3 sm:grid-cols-3">
      <div className="space-y-1.5">
        <Label htmlFor="summaries-classroom">{t('teacher.summaries.filterClassroom')}</Label>
        <select
          id="summaries-classroom"
          className={selectClass}
          value={filters.classroom}
          // 學生選項屬於教室，換教室時一併清掉學生條件
          onChange={(e) => onChange({ classroom: e.target.value, student: '' })}
        >
          <option value="">{t('teacher.summaries.allClassrooms')}</option>
          {(classrooms.data ?? []).map((c) => (
            <option key={c.id} value={c.id}>
              {c.year_label ? `${c.name}（${c.year_label}）` : c.name}
            </option>
          ))}
        </select>
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="summaries-student">{t('teacher.summaries.filterStudent')}</Label>
        <select
          id="summaries-student"
          className={selectClass}
          value={filters.student}
          disabled={!filters.classroom}
          aria-describedby={filters.classroom ? undefined : 'summaries-student-hint'}
          onChange={(e) => onChange({ student: e.target.value })}
        >
          <option value="">{t('teacher.summaries.allStudents')}</option>
          {studentOptions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.label}
            </option>
          ))}
        </select>
        {filters.classroom ? null : (
          <p id="summaries-student-hint" className="text-muted-foreground text-xs">
            {t('teacher.summaries.studentNeedsClassroom')}
          </p>
        )}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="summaries-topic">{t('teacher.summaries.filterTopic')}</Label>
        <select id="summaries-topic" className={selectClass} value={filters.topic} onChange={(e) => onChange({ topic: e.target.value })}>
          <option value="">{t('teacher.summaries.allTopics')}</option>
          {topicTitles.map((title) => (
            <option key={title} value={title}>
              {title}
            </option>
          ))}
        </select>
      </div>
    </div>
  )
}

function summariesPath(filters: Filters, cursor: string | null): string {
  const params = new URLSearchParams()
  if (filters.classroom) params.set('classroom_id', filters.classroom)
  if (filters.student) params.set('student', filters.student)
  if (filters.topic) params.set('topic', filters.topic)
  if (filters.search) params.set('q', filters.search)
  if (cursor) params.set('cursor', cursor)
  const query = params.toString()
  return `/api/summaries${query ? `?${query}` : ''}`
}

function SummaryList({
  filters,
  cursor,
  onNext,
  onPrevious,
  onRestart,
}: {
  filters: Filters
  cursor: string | null
  onNext: (cursor: string) => void
  onPrevious?: () => void
  onRestart: () => void
}) {
  const t = useT()
  const summaries = useFetch<PageResult<TeacherSummary>>(summariesPath(filters, cursor))
  const data = summaries.data?.items
  const nextCursor = summaries.data?.next_cursor
  const filtered = Object.values(filters).some(Boolean)
  // 篩選條件不正確時重試也不會成功，出路是上方的「清除條件」
  const invalidFilter = isApiError(summaries.error) && summaries.error.code === 'invalid_filter'

  return (
    <>
      {summaries.loading && !data ? <LoadingBlock /> : null}
      {/* 失效的游標（例如部署後游標版本改變）重試也不會成功，改從第一頁開始 */}
      {summaries.error ? (
        <ErrorBlock
          error={summaries.error}
          onRetry={invalidFilter ? undefined : isInvalidCursor(summaries.error) ? onRestart : summaries.reload}
        />
      ) : null}
      {data && data.length === 0 ? <EmptyBlock>{filtered ? t('teacher.summaries.noMatch') : t('teacher.summaries.empty')}</EmptyBlock> : null}
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
