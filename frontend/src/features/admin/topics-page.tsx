import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 admin 功能切片實作（題目庫維護）。 */
export function TopicsPage() {
  const t = useT()
  return (
    <Page title={t('admin.topics.title')}>
      <EmptyBlock />
    </Page>
  )
}
