import { useAuth, useMe } from '@/auth/auth-context'
import { AuthCard, AuthFrame, AuthHeading } from '@/components/auth-frame'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'

/** 名單外、非教師、非管理者的帳號看到的「尚未開通」頁（S-01.4、S-08.2）。 */
export function PendingPage() {
  const t = useT()
  const me = useMe()
  const { refresh, logout } = useAuth()
  return (
    <AuthFrame>
      <AuthCard role="alertdialog" aria-labelledby="pending-title">
        <div className="flex flex-col gap-2">
          <AuthHeading id="pending-title" title={t('pending.title')} description={t('pending.body')} />
          <p className="text-muted-foreground text-xs">{t('pending.email', { email: me.email })}</p>
        </div>
        <div className="flex flex-col gap-3">
          <Button size="lg" className="w-full" onClick={() => void refresh()}>
            {t('pending.recheck')}
          </Button>
          <Button variant="outline" size="lg" className="w-full" onClick={() => void logout()}>
            {t('pending.switch')}
          </Button>
        </div>
      </AuthCard>
    </AuthFrame>
  )
}
