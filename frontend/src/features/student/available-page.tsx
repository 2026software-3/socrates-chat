import { useEffect, useId, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { errorText, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { Available, Conversation, Page, StudentActivity, TopicProgress } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type Source = { activity_id: string; topic_id: string } | { topic_id: string }

type Item =
  | { kind: 'activity'; key: string; activity: StudentActivity }
  | { kind: 'topic'; key: string; title: string; description: string; category: string | null; source: Source }

/** 搜尋比對的文字：活動含每題標題。 */
function searchTexts(i: Item): string[] {
  return i.kind === 'activity'
    ? [i.activity.title, i.activity.description, ...i.activity.topics.map((x) => x.title)]
    : [i.title, i.description, i.category ?? '']
}

/** 自己是否有封存的對話屬於這個活動；查不到（含請求失敗）就當作沒有。 */
async function hasArchivedConversation(activityId: string): Promise<boolean> {
  try {
    const page = await api.get<Page<Conversation>>(
      `/api/conversations?archived=true&activity_id=${encodeURIComponent(activityId)}&limit=1`,
    )
    return page.items.length > 0
  } catch {
    return false
  }
}

/** 學生選擇討論來源：教師活動與題庫分成兩區（F-04.1）。 */
export function AvailablePage() {
  const t = useT()
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const { data, error, loading, reload } = useFetch<Available>('/api/catalog')
  const [startingKey, setStartingKey] = useState<string>()
  const [startError, setStartError] = useState<unknown>()
  const busy = useRef(false)
  const errorRef = useRef<HTMLDivElement>(null)

  // 錯誤出現時捲到可見處（卡片可能在畫面外）
  useEffect(() => {
    if (startError) errorRef.current?.scrollIntoView?.({ block: 'nearest' })
  }, [startError])

  async function start(key: string, source: Source) {
    if (busy.current) return // 防止連點建立兩場對話
    busy.current = true
    setStartingKey(key)
    setStartError(undefined)
    try {
      const created = await api.post<Conversation>('/api/conversations', source)
      navigate(`/conversations/${created.id}`)
    } catch (e) {
      // 這個活動的對話被自己封存了：清單預設看不到它，直接帶去「已封存」檢視
      if (isApiError(e) && e.code === 'activity_already_started' && 'activity_id' in source) {
        if (await hasArchivedConversation(source.activity_id)) {
          navigate('/conversations?archived=true')
          return
        }
      }
      setStartError(e)
      reload() // 來源可能剛被關閉，重新載入清單
    } finally {
      busy.current = false
      setStartingKey(undefined)
    }
  }

  const needle = query.trim().toLowerCase()
  const matches = (i: Item) => !needle || searchTexts(i).some((v) => v.toLowerCase().includes(needle))
  const activities = (data?.activities ?? [])
    .map((a): Item => ({ kind: 'activity', key: `a:${a.id}`, activity: a }))
    .filter(matches)
  const topics = (data?.topics ?? [])
    .map(
      (x): Item => ({
        kind: 'topic',
        key: `t:${x.id}`,
        title: x.title,
        description: x.description,
        category: x.category,
        source: { topic_id: x.id },
      }),
    )
    .filter(matches)

  let body
  if (!data && loading) body = <LoadingBlock />
  else if (!data) body = <ErrorBlock error={error} onRetry={reload} />
  else if (data.activities.length === 0 && data.topics.length === 0)
    body = <EmptyBlock>{t('student.available.empty')}</EmptyBlock>
  else
    body = (
      <div className="flex flex-col gap-6">
        {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
        <Section
          title={t('student.available.activities')}
          emptyText={needle ? t('student.available.noMatch') : t('student.available.noActivities')}
          items={activities}
          startingKey={startingKey}
          onStart={start}
          layout="grid"
        />
        <Section
          title={t('student.available.topics')}
          emptyText={needle ? t('student.available.noMatch') : t('student.available.noTopics')}
          items={topics}
          startingKey={startingKey}
          onStart={start}
          layout="list"
        />
      </div>
    )

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-1">
        <h1 className="text-2xl leading-8 font-medium">{t('student.available.title')}</h1>
        <p className="text-muted-foreground text-xs">{t('student.available.subtitle')}</p>
      </div>
      <div>
        <label htmlFor="available-search" className="sr-only">
          {t('student.available.searchLabel')}
        </label>
        <Input
          id="available-search"
          type="search"
          className="h-10 max-w-[360px]"
          placeholder={t('student.available.searchPlaceholder')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      {startError ? (
        <div ref={errorRef}>
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, startError)}</AlertTitle>
          </Alert>
        </div>
      ) : null}
      {body}
    </div>
  )
}

type SectionProps = {
  title: string
  emptyText: string
  items: Item[]
  startingKey: string | undefined
  onStart: (key: string, source: Source) => void
  layout: 'grid' | 'list'
}

/** 教室活動用兩欄卡片（標題、狀態、說明、主按鈕）；個人練習是單欄卡片（標題、題庫與分類、次按鈕）。 */
function Section({ title, emptyText, items, startingKey, onStart, layout }: SectionProps) {
  const t = useT()
  const headingId = useId()
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3">
      <h2 id={headingId} className="text-lg leading-7 font-medium">
        {title}
      </h2>
      {items.length === 0 ? (
        <EmptyBlock>{emptyText}</EmptyBlock>
      ) : (
        <ul className={layout === 'grid' ? 'grid gap-4 md:grid-cols-2' : 'flex flex-col gap-4'}>
          {items.map((item) => (
            <li key={item.key} className="min-w-0">
              {item.kind === 'activity' ? (
                <ActivityCard activity={item.activity} startingKey={startingKey} onStart={onStart} />
              ) : (
                <article className="bg-background flex h-full flex-col justify-between gap-3 rounded-xl border p-5 break-words">
                  <div className="flex min-w-0 flex-col gap-1">
                    <span className="font-medium">{item.title}</span>
                    <span className="text-muted-foreground text-xs">
                      {item.category ? t('student.available.bankCategory', { category: item.category }) : t('student.available.bank')}
                    </span>
                  </div>
                  <div className="flex justify-end">
                    <Button
                      variant="outline"
                      onClick={() => onStart(item.key, item.source)}
                      disabled={startingKey !== undefined}
                      aria-label={t('student.available.startNamed', { title: item.title })}
                    >
                      {startingKey === item.key ? t('student.available.starting') : t('student.available.startPractice')}
                    </Button>
                  </div>
                </article>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

type ActivityCardProps = {
  activity: StudentActivity
  startingKey: string | undefined
  onStart: (key: string, source: Source) => void
}

/** 教室活動卡片：標題、整體進度與完成狀態，下面每題一列，各自開始、繼續或查看總結（F-29.6、S-04.7）。 */
function ActivityCard({ activity, startingKey, onStart }: ActivityCardProps) {
  const t = useT()
  const navigate = useNavigate()
  const done = activity.topics.filter((x) => x.summary_ready).length
  return (
    <article className="bg-background flex h-full flex-col gap-3 rounded-xl border p-5 break-words">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <span className="font-medium">{activity.title}</span>
          {activity.description ? (
            <span className="text-muted-foreground text-xs whitespace-pre-line">{activity.description}</span>
          ) : null}
        </div>
        <span className="text-muted-foreground shrink-0 text-xs">
          {t(activity.completed ? 'student.available.completed' : 'student.available.openNow')}
        </span>
      </div>
      <p className="text-xs font-medium">{t('student.available.progress', { done, total: activity.topics.length })}</p>
      <ul className="flex flex-col gap-2">
        {activity.topics.map((topic) => {
          const key = `a:${activity.id}:${topic.topic_id}`
          return (
            <li key={topic.topic_id} className="flex items-center justify-between gap-3 rounded-lg border p-3">
              <div className="flex min-w-0 flex-col gap-0.5">
                <span className="text-sm">{topic.title}</span>
                <span className="text-muted-foreground text-xs">{t(topicStatusKey(topic))}</span>
              </div>
              <TopicAction
                topic={topic}
                starting={startingKey === key}
                disabled={startingKey !== undefined}
                onStart={() => onStart(key, { activity_id: activity.id, topic_id: topic.topic_id })}
                onOpen={(id) => navigate(`/conversations/${id}`)}
              />
            </li>
          )
        })}
      </ul>
    </article>
  )
}

function topicStatusKey(topic: TopicProgress) {
  if (topic.summary_ready) return 'student.available.topicDone'
  if (topic.conversation_status === 'ended') return 'student.available.topicEnded'
  if (topic.conversation_id) return 'chat.status.active'
  return 'student.available.topicNotStarted'
}

/** 每題只有一場對話：沒開始才能開始；已有對話只能繼續（進行中）或查看總結（已結束），不能重開。 */
function TopicAction({
  topic,
  starting,
  disabled,
  onStart,
  onOpen,
}: {
  topic: TopicProgress
  starting: boolean
  disabled: boolean
  onStart: () => void
  onOpen: (conversationId: string) => void
}) {
  const t = useT()
  const conversationId = topic.conversation_id
  if (!conversationId) {
    return (
      <Button size="sm" onClick={onStart} disabled={disabled} aria-label={t('student.available.startNamed', { title: topic.title })}>
        {starting ? t('student.available.starting') : t('student.available.start')}
      </Button>
    )
  }
  const ended = topic.conversation_status === 'ended'
  return (
    <Button
      size="sm"
      variant="outline"
      onClick={() => onOpen(conversationId)}
      aria-label={t(ended ? 'student.available.viewSummaryNamed' : 'student.available.continueNamed', { title: topic.title })}
    >
      {t(ended ? 'chat.list.viewSummary' : 'student.available.continue')}
    </Button>
  )
}
