import { EmptyBlock, Page } from '@/components/page'
import { useT } from '@/i18n'

/** 佔位頁：由 admin 功能切片實作（新增／移除教師）。 */
export function TeachersPage() {
  const t = useT()
  return (
    <Page title={t('admin.teachers.title')}>
      <EmptyBlock />
    </Page>
  )
}
