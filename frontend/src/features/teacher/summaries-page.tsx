import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 teacher 功能切片實作（學生總結列表）。 */
export function SummariesPage() {
  const t = useT()
  return (
    <Page title={t('teacher.summaries.title')}>
      <EmptyBlock />
    </Page>
  )
}
