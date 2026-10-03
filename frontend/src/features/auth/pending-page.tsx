import { useMe } from '@/auth/auth-context'
import { Page } from '@/components/page'
import { useT } from '@/i18n'

/** 名單外、非教師、非管理者的帳號看到的「尚未開通」頁（S-01.4、S-08.2）。 */
export function PendingPage() {
  const t = useT()
  const me = useMe()
  return (
    <Page title={t('pending.title')}>
      <p>{t('pending.body')}</p>
      <p className="text-muted-foreground text-sm">{t('pending.email', { email: me.email })}</p>
    </Page>
  )
}
