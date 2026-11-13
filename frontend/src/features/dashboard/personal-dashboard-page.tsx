import { Link } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Stat } from '@/features/dashboard/distribution'
import { formatDate } from '@/lib/format'
import { useI18n, useT } from '@/i18n'
import type { PersonalDashboard } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 學生帳號頁（S-18）裡的儀表板內容：自己的對話數量、最近完成的討論（主張、立場、回合數）。只算自己的資料。 */
export function PersonalDashboardContent() {
  const { data, error, loading, reload } = useFetch<PersonalDashboard>('/api/dashboard/me')

  return (
    <>
      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? <Body d={data} /> : null}
    </>
  )
}

function Body({ d }: { d: PersonalDashboard }) {
  const t = useT()
  const { lang } = useI18n()
  return (
    <div className="space-y-6">
      <div className="grid grid-cols-3 gap-2 sm:gap-3">
        <Stat label={t('dashboard.me.conversations')} value={d.total_conversations} />
        <Stat label={t('dashboard.completed')} value={d.completed} />
        <Stat label={t('dashboard.me.turns')} value={d.total_turns} />
      </div>
      {d.completed === 0 ? (
        <div className="space-y-3">
          <EmptyBlock>{t('dashboard.me.empty')}</EmptyBlock>
          <div className="text-center">
            <Button asChild>
              <Link to="/">{t('dashboard.me.start')}</Link>
            </Button>
          </div>
        </div>
      ) : (
        <section className="space-y-3" aria-labelledby="me-recent">
          <h2 id="me-recent" className="text-lg font-medium">
            {t('dashboard.me.recent')}
          </h2>
          <ul className="space-y-3">
            {d.recent.map((r) => (
              <li key={r.conversation_id}>
                <Card>
                  <CardContent className="space-y-2">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <Link to={`/conversations/${r.conversation_id}`} className="font-medium break-words underline-offset-4 hover:underline">
                        {r.title}
                      </Link>
                      <span className="text-muted-foreground text-xs">{formatDate(r.ended_at, lang)}</span>
                    </div>
                    {r.claim ? <p className="font-medium break-words">{r.claim}</p> : null}
                    {r.stance ? <p className="text-muted-foreground text-sm break-words">{r.stance}</p> : null}
                    <div className="flex flex-wrap gap-2">
                      <Badge variant="outline">{t('dashboard.me.turnsOf', { count: r.turn_count })}</Badge>
                    </div>
                  </CardContent>
                </Card>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}
