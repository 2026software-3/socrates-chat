import { useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Available, Conversation } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/**
 * 學生的題目詳情頁：開始討論前先看題目與引導問題（S-04.8）。
 * 引導問題只是給學生想清楚用，沒有引導問題就不顯示該區塊。
 * 從活動的題目進來時帶 `?activity=<id>`，開始討論就算在該活動底下。
 */
export function TopicDetailPage() {
  const t = useT()
  const { lang } = useI18n()
  const navigate = useNavigate()
  const { topicId = '' } = useParams()
  const [search] = useSearchParams()
  const activityId = search.get('activity')
  const { data, error, loading, reload } = useFetch<Available>(`/api/catalog?language=${lang}`)
  const [starting, setStarting] = useState(false)
  const [startError, setStartError] = useState<unknown>()

  async function start() {
    if (starting) return
    setStarting(true)
    setStartError(undefined)
    try {
      const created = await api.post<Conversation>(
        '/api/conversations',
        activityId ? { activity_id: activityId, topic_id: topicId } : { topic_id: topicId },
      )
      navigate(`/conversations/${created.id}`)
    } catch (e) {
      setStartError(e)
      setStarting(false)
    }
  }

  if (!data) return loading ? <LoadingBlock /> : <ErrorBlock error={error} onRetry={reload} />

  const topic = data.topics.find((x) => x.id === topicId)
  const back = (
    <Link to="/" className="text-muted-foreground text-sm underline">
      {t('student.detail.back')}
    </Link>
  )
  if (!topic) {
    return (
      <div className="flex flex-col gap-4">
        {back}
        <EmptyBlock>{t('student.detail.notFound')}</EmptyBlock>
      </div>
    )
  }
  const questions = topic.guiding_questions ?? []

  return (
    <div className="flex max-w-2xl flex-col gap-6">
      {back}
      <div className="flex flex-col gap-1">
        <h1 className="text-2xl leading-8 font-medium break-words">{topic.title}</h1>
        {topic.title_pending && <span role="status">{t('admin.dimensions.translationPending')}</span>}
      </div>
      {questions.length > 0 ? (
        <section aria-labelledby="topic-guiding-heading" className="flex flex-col gap-3">
          <h2 id="topic-guiding-heading" className="text-lg leading-7 font-medium">
            {t('student.detail.guiding')}
          </h2>
          <ol className="flex list-decimal flex-col gap-2 pl-5">
            {questions.map((q, i) => (
              <li key={i} className="break-words whitespace-pre-line">
                {q}
                {topic.pending_questions?.includes(i) && <span role="status"> {t('admin.dimensions.translationPending')}</span>}
              </li>
            ))}
          </ol>
        </section>
      ) : null}
      {startError ? (
        <Alert variant="destructive" role="alert">
          <AlertTitle>{errorText(t, startError)}</AlertTitle>
        </Alert>
      ) : null}
      <div>
        <Button onClick={() => void start()} disabled={starting}>
          {starting ? t('student.available.starting') : t('student.available.start')}
        </Button>
      </div>
    </div>
  )
}
