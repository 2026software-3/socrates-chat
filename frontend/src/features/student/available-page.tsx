import { useEffect, useId, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Available, Conversation } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type Source = { activity_id: string } | { topic_id: string }

type Item = {
  key: string
  title: string
  description: string
  category?: string | null
  source: Source
}

/** 學生選擇討論來源：教師活動與題庫分成兩區（F-04.1）。 */
export function AvailablePage() {
  const t = useT()
  const navigate = useNavigate()
  const { data, error, loading, reload } = useFetch<Available>('/api/available')
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
      setStartError(e)
      reload() // 來源可能剛被關閉，重新載入清單
    } finally {
      busy.current = false
      setStartingKey(undefined)
    }
  }

  const activities: Item[] = (data?.activities ?? []).map((a) => ({
    key: `a:${a.id}`,
    title: a.title,
    description: a.description,
    source: { activity_id: a.id },
  }))
  const topics: Item[] = (data?.topics ?? []).map((x) => ({
    key: `t:${x.id}`,
    title: x.title,
    description: x.description,
    category: x.category,
    source: { topic_id: x.id },
  }))

  let body
  if (!data && loading) body = <LoadingBlock />
  else if (!data) body = <ErrorBlock error={error} onRetry={reload} />
  else if (activities.length === 0 && topics.length === 0)
    body = <EmptyBlock>{t('student.available.empty')}</EmptyBlock>
  else
    body = (
      <div className="space-y-8">
        {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
        <Section
          title={t('student.available.activities')}
          hint={t('student.available.activitiesHint')}
          emptyText={t('student.available.noActivities')}
          badge={t('student.available.badgeActivity')}
          items={activities}
          startingKey={startingKey}
          onStart={start}
        />
        <Section
          title={t('student.available.topics')}
          hint={t('student.available.topicsHint')}
          emptyText={t('student.available.noTopics')}
          badge={t('student.available.badgeTopic')}
          items={topics}
          startingKey={startingKey}
          onStart={start}
        />
      </div>
    )

  return (
    <Page
      title={t('student.available.title')}
      actions={
        <Button asChild variant="outline" size="sm">
          <Link to="/conversations">{t('student.available.myConversations')}</Link>
        </Button>
      }
    >
      {startError ? (
        <div ref={errorRef} className="mb-4">
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, startError)}</AlertTitle>
          </Alert>
        </div>
      ) : null}
      {body}
    </Page>
  )
}

type SectionProps = {
  title: string
  hint: string
  emptyText: string
  badge: string
  items: Item[]
  startingKey: string | undefined
  onStart: (item: Item) => void
}

function Section({ title, hint, emptyText, badge, items, startingKey, onStart }: SectionProps) {
  const t = useT()
  const headingId = useId()
  return (
    <section aria-labelledby={headingId} className="space-y-3">
      <div>
        <h2 id={headingId} className="text-lg font-semibold">
          {title}
        </h2>
        <p className="text-muted-foreground text-sm">{hint}</p>
      </div>
      {items.length === 0 ? (
        <EmptyBlock>{emptyText}</EmptyBlock>
      ) : (
        <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {items.map((item) => (
            <li key={item.key} className="min-w-0">
              <Card className="h-full justify-between break-words">
                <CardHeader>
                  <div className="flex flex-wrap gap-2">
                    <Badge variant="secondary">{badge}</Badge>
                    {item.category ? <Badge variant="outline">{item.category}</Badge> : null}
                  </div>
                  <CardTitle>{item.title}</CardTitle>
                  <CardDescription className="whitespace-pre-line">{item.description}</CardDescription>
                </CardHeader>
                <CardFooter>
                  <Button
                    onClick={() => onStart(item)}
                    disabled={startingKey !== undefined}
                    aria-label={t('student.available.startNamed', {
                      title: item.title,
                    })}
                  >
                    {startingKey === item.key ? t('student.available.starting') : t('student.available.start')}
                  </Button>
                </CardFooter>
              </Card>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
