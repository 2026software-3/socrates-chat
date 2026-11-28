import { useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router'
import { ArrowLeft, Calendar, Clock } from 'lucide-react'
import { EmptyBlock, Page } from '@/components/page'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import { StatusChip } from '@/components/vedute/status-chip'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Available, Conversation, StudentActivity } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

function activityStatusChip(activity: StudentActivity) {
  if (activity.completed) return { tone: 'inactive' as const, labelKey: 'student.available.completed' as const }
  if (activity.status === 'closed') return { tone: 'inactive' as const, labelKey: 'student.detail.statusClosed' as const }
  if (activity.status === 'draft') return { tone: 'draft' as const, labelKey: 'student.detail.statusDraft' as const }
  return { tone: 'active' as const, labelKey: 'student.available.openNow' as const }
}

/**
 * 教室活動・活動詳細與題目詳情頁（S-04.8、F-41.17 R02）：
 * - 從教室活動進來時帶 `?activity=<id>`：顯示所屬教室、活動名稱、狀態標籤、發布/截止時間、活動說明、引導問題與主操作按鈕。
 * - 從個人練習題庫進來時（不帶 `activity`）：顯示題目標題、引導問題與開始討論按鈕。
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

  if (!data && loading) {
    return (
      <Page title={t('student.available.title')}>
        <SkeletonRegion loadingLabel={t('student.detail.loading')} className="max-w-3xl space-y-5">
          <Skeleton className="h-5 w-48" />
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-32 w-full" />
        </SkeletonRegion>
      </Page>
    )
  }

  if (!data) {
    return (
      <Page title={t('student.available.title')}>
        <Alert kind="error" className="flex flex-wrap items-center justify-between gap-3">
          <span>{errorText(t, error)}</span>
          <Button variant="quiet" onClick={reload}>
            {t('common.retry')}
          </Button>
        </Alert>
      </Page>
    )
  }

  const topic = data.topics.find((x) => x.id === topicId)
  const activity = activityId ? data.activities.find((a) => a.id === activityId) : undefined

  if (!topic) {
    return (
      <Page
        title={t('student.available.title')}
        crumbs={[{ label: t('student.detail.back'), to: '/' }]}
      >
        <div className="max-w-3xl space-y-4">
          <Link to="/" className="type-body inline-flex min-h-11 items-center gap-1.5 text-ink-soft underline hover:text-chalk">
            <ArrowLeft className="size-4" aria-hidden="true" />
            {t('student.detail.back')}
          </Link>
          <EmptyBlock>{t('student.detail.notFound')}</EmptyBlock>
        </div>
      </Page>
    )
  }

  const questions = topic.guiding_questions ?? []
  const progress = activity?.topics.find((p) => p.topic_id === topicId)
  const existing = progress?.conversation_id
    ? {
        conversation_id: progress.conversation_id,
        ended: progress.conversation_status === 'ended',
      }
    : undefined
  const statusInfo = activity ? activityStatusChip(activity) : null

  return (
    <Page
      title={topic.title}
      description={activity ? activity.title : undefined}
      crumbs={[
        { label: activity ? activity.classroom_name : t('student.detail.back'), to: '/' },
        ...(activity ? [{ label: activity.title }] : []),
      ]}
      actions={
        statusInfo ? <StatusChip tone={statusInfo.tone}>{t(statusInfo.labelKey)}</StatusChip> : undefined
      }
    >
      <div className="flex max-w-3xl flex-col gap-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <Link
            to="/"
            className="type-body inline-flex min-h-11 items-center gap-1.5 text-ink-soft no-underline hover:text-chalk hover:underline"
          >
            <ArrowLeft className="size-4" aria-hidden="true" />
            {t('student.detail.back')}
          </Link>
          {topic.title_pending && (
            <span role="status" className="type-note text-ochra">
              {t('admin.dimensions.translationPending')}
            </span>
          )}
        </div>

        {/* R02 活動 Meta 資訊列（教室名稱、發布時間、截止時間） */}
        {activity && (
          <div className="type-note flex flex-wrap items-center gap-x-5 gap-y-2 border-b border-ink-faint pb-4 text-ink-soft">
            <span>{activity.classroom_name}</span>
            {activity.opens_at && (
              <span className="inline-flex items-center gap-1.5">
                <Calendar className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
                {t('student.detail.opensAt', { date: formatDate(activity.opens_at, lang) })}
              </span>
            )}
            {activity.closes_at && (
              <span className="inline-flex items-center gap-1.5">
                <Clock className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
                {t('student.detail.closesAt', { date: formatDate(activity.closes_at, lang) })}
              </span>
            )}
          </div>
        )}

        {/* 教師填寫的活動說明（僅在教室活動且有填寫說明時顯示） */}
        {activity?.description && (
          <section
            aria-labelledby="activity-description-heading"
            className="space-y-2 rounded-sm border border-ink-faint bg-surface p-4 md:p-5"
          >
            <h2 id="activity-description-heading" className="type-h3 text-ink">
              {t('student.detail.activityDescription')}
            </h2>
            <p className="type-body break-words whitespace-pre-line text-ink">{activity.description}</p>
          </section>
        )}

        {/* 本題引導問題 */}
        {questions.length > 0 && (
          <section
            aria-labelledby="topic-guiding-heading"
            className="space-y-3 rounded-sm border border-ink-faint bg-surface p-4 md:p-5"
          >
            <h2 id="topic-guiding-heading" className="type-h3 text-ink">
              {t('student.detail.guiding')}
            </h2>
            <ol className="type-body flex list-decimal flex-col gap-2.5 pl-5 text-ink">
              {questions.map((q, i) => (
                <li key={i} className="break-words whitespace-pre-line">
                  {q}
                  {topic.pending_questions?.includes(i) && (
                    <span role="status" className="type-note ml-2 text-ochra">
                      {t('admin.dimensions.translationPending')}
                    </span>
                  )}
                </li>
              ))}
            </ol>
          </section>
        )}

        {startError ? <Alert kind="error">{errorText(t, startError)}</Alert> : null}

        {/* 主操作按鈕（桌機在左下，手機全寬） */}
        <div className="pt-1">
          {existing ? (
            <Button
              variant="quiet"
              className="w-full md:w-auto"
              onClick={() => navigate(`/conversations/${existing.conversation_id}`)}
              aria-label={t(
                existing.ended ? 'student.available.viewSummaryNamed' : 'student.available.continueNamed',
                { title: topic.title },
              )}
            >
              {t(existing.ended ? 'chat.list.viewSummary' : 'student.available.continue')}
            </Button>
          ) : (
            <Button className="w-full md:w-auto" onClick={() => void start()} disabled={starting}>
              {starting ? t('student.available.starting') : t('student.available.start')}
            </Button>
          )}
        </div>
      </div>
    </Page>
  )
}