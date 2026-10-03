import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Conversation, Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 學生直接選擇一個題目開始討論（F-04.1）。 */
export function AvailablePage() {
  const t = useT()
  const navigate = useNavigate()
  const { data, error, loading, reload } = useFetch<Topic[]>('/api/available')
  const [startingKey, setStartingKey] = useState<string>()
  const [startError, setStartError] = useState<unknown>()
  const busy = useRef(false)
  const errorRef = useRef<HTMLDivElement>(null)

  // 錯誤出現時捲到可見處（卡片可能在畫面外）
  useEffect(() => {
    if (startError) errorRef.current?.scrollIntoView?.({ block: 'nearest' })
  }, [startError])

  async function start(topic: Topic) {
    if (busy.current) return // 防止連點建立兩場對話
    busy.current = true
    setStartingKey(topic.id)
    setStartError(undefined)
    try {
      const created = await api.post<Conversation>('/api/conversations', { topic_id: topic.id })
      navigate(`/conversations/${created.id}`)
    } catch (e) {
      setStartError(e)
      reload() // 題目可能剛被停用，重新載入清單
    } finally {
      busy.current = false
      setStartingKey(undefined)
    }
  }

  let body
  if (!data && loading) body = <LoadingBlock />
  else if (!data) body = <ErrorBlock error={error} onRetry={reload} />
  else if (data.length === 0) body = <EmptyBlock>{t('student.available.empty')}</EmptyBlock>
  else
    body = (
      <div className="space-y-4">
        {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
        <TopicList topics={data} startingKey={startingKey} onStart={start} />
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

type TopicListProps = {
  topics: Topic[]
  startingKey: string | undefined
  onStart: (topic: Topic) => void
}

function TopicList({ topics, startingKey, onStart }: TopicListProps) {
  const t = useT()
  return (
    <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {topics.map((topic) => (
        <li key={topic.id} className="min-w-0">
          <Card className="h-full justify-between break-words">
            <CardHeader>
              {topic.category ? (
                <div className="flex flex-wrap gap-2">
                  <Badge variant="outline">{topic.category}</Badge>
                </div>
              ) : null}
              <CardTitle>{topic.title}</CardTitle>
              <CardDescription className="whitespace-pre-line">{topic.description}</CardDescription>
            </CardHeader>
            <CardFooter>
              <Button
                onClick={() => onStart(topic)}
                disabled={startingKey !== undefined}
                aria-label={t('student.available.startNamed', { title: topic.title })}
              >
                {startingKey === topic.id ? t('student.available.starting') : t('student.available.start')}
              </Button>
            </CardFooter>
          </Card>
        </li>
      ))}
    </ul>
  )
}
