import { createContext, useContext, type ReactNode } from 'react'
import { Link } from 'react-router'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { errorText, useT } from '@/i18n'
import { isApiError } from '@/lib/api'

/** 教師／管理者工作區的名稱；學生外框沒有工作區，值為 null。由 Layout 提供。 */
export const WorkspaceContext = createContext<string | null>(null)

/**
 * 頁面標題列。工作區頁面（教師、管理者）有「工作區／頁名」麵包屑與 30px 粗體標題；
 * 學生頁面是 24px 標題，說明文字放在標題下方。
 */
export type Crumb = { label: string; to?: string }

export function PageHeader({ title, description, actions, crumbs }: { title: string; description?: ReactNode; actions?: ReactNode; crumbs?: Crumb[] }) {
  const workspace = useContext(WorkspaceContext)
  return (
    <div className="flex flex-wrap items-end justify-between gap-4">
      <div className="flex min-w-0 flex-col gap-1">
        {crumbs && crumbs.length > 0 ? (
          <nav aria-label="breadcrumb" className="text-muted-foreground flex flex-wrap items-center gap-x-1.5 text-xs">
            {workspace ? <span>{workspace}</span> : null}
            {crumbs.map((c, i) => (
              <span key={`${i}-${c.label}`} className="flex items-center gap-x-1.5">
                {workspace || i > 0 ? <span aria-hidden>／</span> : null}
                {c.to ? (
                  <Link to={c.to} className="hover:text-foreground underline-offset-4 hover:underline">
                    {c.label}
                  </Link>
                ) : (
                  <span aria-current="page">{c.label}</span>
                )}
              </span>
            ))}
          </nav>
        ) : workspace ? (
          <p className="text-muted-foreground text-xs">{`${workspace}／${title}`}</p>
        ) : null}
        <h1 className={workspace ? 'text-3xl leading-9 font-bold' : 'text-2xl leading-8 font-medium'}>{title}</h1>
        {description ? <p className="text-muted-foreground text-xs">{description}</p> : null}
      </div>
      {actions}
    </div>
  )
}

/** 頁面標題＋內容的共用外框。 */
export function Page({
  title,
  description,
  actions,
  crumbs,
  children,
}: {
  title: string
  description?: ReactNode
  actions?: ReactNode
  crumbs?: Crumb[]
  children: ReactNode
}) {
  return (
    <section className="space-y-6">
      <PageHeader title={title} description={description} actions={actions} crumbs={crumbs} />
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
