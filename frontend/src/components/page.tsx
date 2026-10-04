import type { ReactNode } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { errorText, useT } from '@/i18n'
import { isApiError } from '@/lib/api'

/** 頁面標題＋內容的共用外框。 */
export function Page({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {actions}
      </div>
      {children}
    </section>
  )
}

/** 等待狀態。 */
export function LoadingBlock() {
  const t = useT()
  return (
    <div role="status" aria-live="polite" className="space-y-2">
      <span className="sr-only">{t('common.loading')}</span>
      <Skeleton className="h-6 w-1/3" />
      <Skeleton className="h-24 w-full" />
    </div>
  )
}

/** 錯誤狀態：顯示翻譯後的訊息與 request ID，並可重試。 */
export function ErrorBlock({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const t = useT()
  return (
    <Alert variant="destructive" role="alert">
      <AlertTitle>{errorText(t, error)}</AlertTitle>
      {isApiError(error) && error.requestId ? (
        <AlertDescription>{t('common.requestId', { id: error.requestId })}</AlertDescription>
      ) : null}
      {onRetry ? (
        <Button variant="outline" size="sm" className="mt-2" onClick={onRetry}>
          {t('common.retry')}
        </Button>
      ) : null}
    </Alert>
  )
}

/** 空狀態。 */
export function EmptyBlock({ children }: { children?: ReactNode }) {
  const t = useT()
  return <p className="text-muted-foreground rounded-md border border-dashed p-6 text-center text-sm">{children ?? t('common.empty')}</p>
}
