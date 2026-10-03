import { useId } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { ErrorBlock } from '@/components/page'
import { useDateFormat } from '@/features/chat/format'
import { useSummary } from '@/features/chat/use-summary'
import { errorText, useI18n, useT } from '@/i18n'

type Props = { conversationId: string; pollMs?: number; maxPollMs?: number }

/** 對話結束後的 AI 總結：產生中輪詢、完成顯示（學生不可編輯）、失敗可重試。 */
export function SummaryPanel({ conversationId, pollMs, maxPollMs }: Props) {
  const t = useT()
  const { lang } = useI18n()
  const formatDate = useDateFormat(lang)
  const titleId = useId()
  const { state, check, retry, retrying, retryError } = useSummary(conversationId, { pollMs, maxPollMs })

  const completed = state.kind === 'ready' && state.summary.completed_at ? formatDate(state.summary.completed_at) : ''

  return (
    <Card role="region" aria-labelledby={titleId}>
      <CardHeader>
        <CardTitle id={titleId} className="text-lg">
          {t('chat.summary.title')}
        </CardTitle>
        <CardDescription>{t('chat.summary.note')}</CardDescription>
      </CardHeader>
      <CardContent aria-live="polite" className="space-y-3">
        {state.kind === 'pending' ? (
          <div role="status" className="space-y-2">
            <p className="text-muted-foreground text-sm">{t('chat.summary.pending')}</p>
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-4 w-1/2" />
          </div>
        ) : null}

        {state.kind === 'timeout' ? (
          <Alert role="status">
            <AlertTitle>{t('chat.summary.slow')}</AlertTitle>
            <Button variant="outline" size="sm" className="mt-2" onClick={check}>
              {t('chat.summary.check')}
            </Button>
          </Alert>
        ) : null}

        {state.kind === 'error' ? <ErrorBlock error={state.error} onRetry={check} /> : null}

        {state.kind === 'failed' ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{t('chat.summary.failed')}</AlertTitle>
            {retryError ? <AlertDescription>{errorText(t, retryError)}</AlertDescription> : null}
            <Button variant="outline" size="sm" className="mt-2" disabled={retrying} onClick={retry}>
              {t('chat.summary.retry')}
            </Button>
          </Alert>
        ) : null}

        {state.kind === 'ready' ? (
          <>
            <dl className="space-y-4">
              <SummaryItem label={t('chat.summary.stance')} text={state.summary.stance} />
              <SummaryItem label={t('chat.summary.reasons')} text={state.summary.reasons} />
              <SummaryItem label={t('chat.summary.turning')} text={state.summary.turning_points} />
            </dl>
            {completed ? (
              <p className="text-muted-foreground text-xs">{t('chat.summary.completedAt', { date: completed })}</p>
            ) : null}
          </>
        ) : null}
      </CardContent>
    </Card>
  )
}

function SummaryItem({ label, text }: { label: string; text: string | null }) {
  if (!text) return null
  return (
    <div>
      <dt className="text-sm font-medium">{label}</dt>
      <dd className="mt-1 text-sm whitespace-pre-wrap break-words">{text}</dd>
    </div>
  )
}
