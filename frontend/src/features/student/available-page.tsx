import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 student 功能切片實作。 */
export function AvailablePage() {
  const t = useT()
  return (
    <Page title={t('student.available.title')}>
      <EmptyBlock />
    </Page>
  )
}
