import { ErrorBlock } from '@/components/page'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import type { ApiError } from '@/lib/api'

/** 分頁列表底部：還有下一頁時顯示「載入更多」；載入失敗時保留已載入的項目並顯示重試。 */
export function LoadMore({
  hasMore,
  loading,
  error,
  onLoadMore,
}: {
  hasMore: boolean
  loading: boolean
  error?: ApiError
  onLoadMore: () => void
}) {
  const t = useT()
  if (error) return <ErrorBlock error={error} onRetry={onLoadMore} />
  if (!hasMore) return null
  return (
    <div>
      <Button variant="outline" disabled={loading} onClick={onLoadMore}>
        {loading ? t('common.loadingMore') : t('common.loadMore')}
      </Button>
    </div>
  )
}
