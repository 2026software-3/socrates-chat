import { useEffect, useMemo, useRef } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useI18n, useT } from '@/i18n'
import { useFetch } from '@/lib/use-fetch'

type RequestLogEntry = {
  created_at: string
  request_id: string
  method: string
  route: string
  status: number
  duration_ms: number
  error_code: string | null
}

type RequestLogPage = {
  items: RequestLogEntry[]
  capacity: number
}

/** 表頭固定在捲動區頂端，捲動長記錄時仍看得到欄位名稱。 */
const STICKY_HEAD = 'sticky top-0 z-10 bg-background'

/** 一次讀取超過這個時間仍沒有回應，就視為卡住並重新發出。 */
const STALL_MS = 30_000

function statusVariant(status: number): 'destructive' | 'outline' | 'secondary' {
  if (status >= 500) return 'destructive'
  if (status >= 400) return 'outline'
  return 'secondary'
}

/** 管理者暫時維運頁：每 5 秒讀取目前程序記憶體中的安全 request log。 */
export function LogsPage() {
  const t = useT()
  const { lang } = useI18n()
  const { data, error, loading, reload } = useFetch<RequestLogPage>('/api/admin/logs')
  const date = useMemo(
    () => new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'medium' }),
    [lang],
  )

  // 工作階段被拒絕（過期或失去管理者角色）後輪詢只會一再被拒絕，停止到使用者按重試為止。
  const stopped = error?.status === 401 || error?.status === 403

  // 目前這次讀取開始的時間；沒有進行中的讀取時為 null。
  const loadingSince = useRef<number | null>(null)
  useEffect(() => {
    if (!loading) loadingSince.current = null
    else loadingSince.current ??= Date.now()
  }, [loading])

  useEffect(() => {
    if (stopped) return
    const poll = () => {
      if (document.hidden) return
      // 重新載入會取消進行中的請求，慢於 5 秒的回應就永遠到不了：讀取還在進行時不發新的，
      // 只有卡住太久（沒有回應也沒有錯誤）才放棄它重來。
      const since = loadingSince.current
      if (since !== null && Date.now() - since < STALL_MS) return
      loadingSince.current = Date.now()
      reload()
    }
    const id = window.setInterval(poll, 5000)
    document.addEventListener('visibilitychange', poll)
    return () => {
      window.clearInterval(id)
      document.removeEventListener('visibilitychange', poll)
    }
  }, [reload, stopped])

  return (
    <Page title={t('admin.logs.title')}>
      <div className="text-muted-foreground space-y-1 text-sm">
        <p>{t('admin.logs.intro', { capacity: data?.capacity ?? 500 })}</p>
        <p>{t('admin.logs.refresh')}</p>
      </div>

      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data?.items.length === 0 ? <EmptyBlock>{t('admin.logs.empty')}</EmptyBlock> : null}
      {data?.items.length ? (
        <ScrollArea type="auto" className="h-[60vh] rounded-md border">
          <Table aria-label={t('admin.logs.table')}>
            <TableHeader>
              <TableRow>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.time')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.method')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.route')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.status')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.duration')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.errorCode')}</TableHead>
                <TableHead className={STICKY_HEAD}>{t('admin.logs.requestId')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.items.map((entry) => (
                <TableRow key={entry.request_id}>
                  <TableCell>
                    <time dateTime={entry.created_at}>{date.format(new Date(entry.created_at))}</time>
                  </TableCell>
                  <TableCell className="font-mono">{entry.method}</TableCell>
                  <TableCell>
                    <code>{entry.route}</code>
                  </TableCell>
                  <TableCell>
                    <Badge variant={statusVariant(entry.status)}>{entry.status}</Badge>
                  </TableCell>
                  <TableCell>{entry.duration_ms} ms</TableCell>
                  <TableCell>
                    <code>{entry.error_code ?? '—'}</code>
                  </TableCell>
                  <TableCell>
                    <code>{entry.request_id}</code>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </ScrollArea>
      ) : null}
    </Page>
  )
}
