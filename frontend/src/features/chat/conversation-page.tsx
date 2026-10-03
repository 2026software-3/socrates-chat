import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 chat 功能切片實作（對話畫面、串流、結束與總結）。 */
export function ConversationPage() {
  const t = useT()
  return (
    <Page title={t('chat.list.title')}>
      <EmptyBlock />
    </Page>
  )
}
