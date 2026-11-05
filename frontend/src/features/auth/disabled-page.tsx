import { useAuth } from '@/auth/auth-context'
import { AuthCard, AuthFrame, AuthHeading } from '@/components/auth-frame'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'

/** 帳號已被管理者停用（S-01.3）：只看得到說明與登出，其他功能一律不可用。 */
export function DisabledPage() {
  const t = useT()
  const auth = useAuth()
  return (
    <AuthFrame>
      <AuthCard role="alertdialog" aria-labelledby="disabled-title">
        <AuthHeading id="disabled-title" title={t('disabled.title')} description={t('disabled.body')} />
        <Button size="lg" className="w-full" onClick={() => void auth.logout()}>
          {t('disabled.back')}
        </Button>
      </AuthCard>
    </AuthFrame>
  )
}
