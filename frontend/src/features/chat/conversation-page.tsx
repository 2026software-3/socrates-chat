import { Link, useParams } from 'react-router'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { Button } from '@/components/ui/button'
import { ConversationView, type SummaryTiming } from '@/features/chat/conversation-view'
import { useT } from '@/i18n'
import type { ConversationDetail } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 對話畫面：載入對話與歷史訊息；404（含「不是你的」）顯示找不到。 */
export function ConversationPage(timing: SummaryTiming) {
  const { id = '' } = useParams()
  const t = useT()
  const { data, error, loading, reload } = useFetch<ConversationDetail>(`/api/conversations/${id}`)

  // 路由參數換了但元件沒卸載時，useFetch 仍留著舊資料；id 不符就當作還沒載入
  if (data && data.id === id) return <ConversationView key={data.id} detail={data} {...timing} />
  if (loading) return <LoadingBlock />
  if (error?.status === 404) {
    return (
      <div role="alert" className="space-y-3 rounded-md border border-dashed p-6 text-center">
        <p>{t('chat.page.notFound')}</p>
        <Button asChild variant="outline">
          <Link to="/conversations">{t('chat.page.back')}</Link>
        </Button>
      </div>
    )
  }
  return <ErrorBlock error={error} onRetry={reload} />
}
