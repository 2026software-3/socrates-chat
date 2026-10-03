import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 teacher 功能切片實作（活動建立／編輯／發布）。 */
export function ActivitiesPage() {
  const t = useT()
  return (
    <Page title={t('teacher.activities.title')}>
      <EmptyBlock />
    </Page>
  )
}
