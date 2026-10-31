import { useEffect, useId, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { errorText, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { Available, Conversation, Page } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type Source = { activity_id: string } | { topic_id: string }

type Item = {
  key: string
  title: string
  description: string
  category?: string | null
  source: Source
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

  async function start(item: Item) {
    if (busy.current) return // 防止連點建立兩場對話
    busy.current = true
    setStartingKey(item.key)
    setStartError(undefined)
    try {
      const created = await api.post<Conversation>('/api/conversations', item.source)
      navigate(`/conversations/${created.id}`)
    } catch (e) {
      // 這個活動的對話被自己封存了：清單預設看不到它，直接帶去「已封存」檢視
      if (isApiError(e) && e.code === 'activity_already_started' && 'activity_id' in item.source) {
        if (await hasArchivedConversation(item.source.activity_id)) {
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
  const matches = (i: Item) =>
    !needle || [i.title, i.description, i.category ?? ''].some((v) => v.toLowerCase().includes(needle))
  const activities: Item[] = (data?.activities ?? [])
    .map((a) => ({ key: `a:${a.id}`, title: a.title, description: a.description, source: { activity_id: a.id } }))
    .filter(matches)
  const topics: Item[] = (data?.topics ?? [])
    .map((x) => ({ key: `t:${x.id}`, title: x.title, description: x.description, category: x.category, source: { topic_id: x.id } }))
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
  onStart: (item: Item) => void
  layout: 'grid' | 'list'
}

/** 教室活動用兩欄卡片（標題、狀態、說明、主按鈕）；個人練習是單欄卡片（標題、題庫與分類、次按鈕）。 */
function Section({ title, emptyText, items, startingKey, onStart, layout }: SectionProps) {
  const t = useT()
  const headingId = useId()
  const activity = layout === 'grid'
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3">
      <h2 id={headingId} className="text-lg leading-7 font-medium">
        {title}
      </h2>
      {items.length === 0 ? (
        <EmptyBlock>{emptyText}</EmptyBlock>
      ) : (
        <ul className={activity ? 'grid gap-4 md:grid-cols-2' : 'flex flex-col gap-4'}>
          {items.map((item) => (
            <li key={item.key} className="min-w-0">
              <article className="bg-background flex h-full flex-col justify-between gap-3 rounded-xl border p-5 break-words">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 flex-col gap-1">
                    <span className="font-medium">{item.title}</span>
                    {activity ? (
                      item.description ? (
                        <span className="text-muted-foreground text-xs whitespace-pre-line">{item.description}</span>
                      ) : null
                    ) : (
                      <span className="text-muted-foreground text-xs">
                        {item.category ? t('student.available.bankCategory', { category: item.category }) : t('student.available.bank')}
                      </span>
                    )}
                  </div>
                  {activity ? <span className="text-muted-foreground shrink-0 text-xs">{t('student.available.openNow')}</span> : null}
                </div>
                <div className="flex justify-end">
                  <Button
                    variant={activity ? 'default' : 'outline'}
                    onClick={() => onStart(item)}
                    disabled={startingKey !== undefined}
                    aria-label={t('student.available.startNamed', { title: item.title })}
                  >
                    {startingKey === item.key
                      ? t('student.available.starting')
                      : t(activity ? 'student.available.start' : 'student.available.startPractice')}
                  </Button>
                </div>
              </article>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
