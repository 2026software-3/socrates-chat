import { useId, type ReactNode } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { ErrorBlock } from '@/components/page'
import { useRadar } from '@/features/chat/use-radar'
import { errorText, useT } from '@/i18n'

type Props = { conversationId: string; pollMs?: number; maxPollMs?: number }

/**
 * 單場雷達圖的計算狀態與手動重試（F-24.1）。總結成功後才放進畫面，自己一張卡片，
 * 所以計算中或失敗都不影響總結的閱讀。圖形呈現屬於 F-24.2。
 */
export function RadarPanel({ conversationId, pollMs, maxPollMs }: Props) {
  const t = useT()
  const titleId = useId()
  const { state, check, start, starting, startError } = useRadar(conversationId, { pollMs, maxPollMs })
  const radar = state.kind === 'loaded' ? state.radar : null

  // 第一次讀取完成前不知道有沒有東西可顯示；總結尚未成功時沒有雷達圖可談
  if ((state.kind === 'loading' && state.initial) || radar?.status === 'summary_not_ready') return null

  const startButton = (label: string) => (
    <Button variant="outline" size="sm" className="mt-2" disabled={starting} onClick={() => void start()}>
      {label}
    </Button>
  )
  const startFailure = startError ? <AlertDescription>{errorText(t, startError)}</AlertDescription> : null
  const computing = state.kind === 'loading' || (radar?.status === 'pending' && !radar.retryable)

  let body: ReactNode = null
  if (computing) {
    body = (
      <div role="status" className="space-y-2">
        <p className="text-muted-foreground text-sm">{t('chat.radar.pending')}</p>
        <Skeleton className="h-4 w-1/2" />
      </div>
    )
  } else if (state.kind === 'timeout') {
    body = (
      <Alert role="status">
        <AlertTitle>{t('chat.radar.slow')}</AlertTitle>
        <Button variant="outline" size="sm" className="mt-2" onClick={check}>
          {t('chat.radar.check')}
        </Button>
      </Alert>
    )
  } else if (state.kind === 'error') {
    body = <ErrorBlock error={state.error} onRetry={check} />
  } else if (radar?.status === 'failed') {
    body = (
      <Alert variant="destructive" role="alert">
        <AlertTitle>{t('chat.radar.failed')}</AlertTitle>
        {startFailure}
        {radar.retryable ? startButton(t('chat.radar.retry')) : null}
      </Alert>
    )
  } else if (radar?.status === 'pending') {
    // 計算超過逾時上限（例如服務重啟）：後端允許重新計算
    body = (
      <Alert role="status">
        <AlertTitle>{t('chat.radar.stuck')}</AlertTitle>
        {startFailure}
        {startButton(t('chat.radar.retry'))}
      </Alert>
    )
  } else if (radar?.status === 'not_started') {
    body = (
      <Alert role="status">
        <AlertTitle>{t('chat.radar.notStarted')}</AlertTitle>
        {startFailure}
        {radar.retryable ? startButton(t('chat.radar.start')) : null}
      </Alert>
    )
  } else if (radar?.status === 'dimensions_pending') {
    body = (
      <Alert role="status">
        <AlertTitle>{t('chat.radar.dimensionsPending')}</AlertTitle>
        <Button variant="outline" size="sm" className="mt-2" onClick={check}>
          {t('chat.radar.check')}
        </Button>
      </Alert>
    )
  } else if (radar?.status === 'unavailable') {
    body = <p className="text-muted-foreground text-sm">{t('chat.radar.unavailable')}</p>
  } else if (radar?.status === 'ready') {
    body = (
      <p role="status" className="text-sm">
        {t('chat.radar.ready')}
      </p>
    )
  }

  return (
    <Card role="region" aria-labelledby={titleId}>
      <CardHeader>
        <CardTitle id={titleId} className="text-lg">
          {t('chat.radar.title')}
        </CardTitle>
      </CardHeader>
      <CardContent aria-live="polite" className="space-y-3">
        {body}
      </CardContent>
    </Card>
  )
}
