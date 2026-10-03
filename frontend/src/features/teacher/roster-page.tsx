import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 teacher 功能切片實作（修課名單匯入／移除）。 */
export function RosterPage() {
  const t = useT()
  return (
    <Page title={t('teacher.roster.title')}>
      <EmptyBlock />
    </Page>
  )
}
