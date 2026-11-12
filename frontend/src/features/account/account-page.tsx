import { useAuth } from '@/auth/auth-context'
import { Page } from '@/components/page'
import { ChangePasswordSection } from '@/features/dashboard/change-password-section'
import { useT } from '@/i18n'

/**
 * 帳號頁（S-18）：教師與管理者沒有個人設定頁（`/dashboard` 只給學生），在這裡更改自己的密碼。
 * 只放「更改密碼」；不能改的帳號（只用 Google 登入沒有內建密碼，或示範帳號）只顯示一句說明。
 */
export function AccountPage() {
  const t = useT()
  const auth = useAuth()
  // 登入者載入完成前不顯示（外框保證實際使用時一定已登入）
  if (auth.status !== 'authed') return null
  return (
    <Page title={t('account.title')}>
      {auth.me.can_change_password ? <ChangePasswordSection /> : <p className="text-muted-foreground text-sm">{t('account.unavailable')}</p>}
    </Page>
  )
}
