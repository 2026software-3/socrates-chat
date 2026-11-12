import { useAuth } from '@/auth/auth-context'
import { Page } from '@/components/page'
import { StaffSettings } from '@/features/account/staff-settings'
import { PersonalDashboardContent } from '@/features/dashboard/personal-dashboard-page'
import { ProfileForm } from '@/features/dashboard/profile-form'
import { useT } from '@/i18n'

/**
 * 帳號頁（S-18）：所有角色的個人頁，從右上角頭像進入。
 * - 學生：個人設定（顯示名稱、Google 帳號，以及個人設定裡「儲存」旁的「更改密碼」按鈕）加上原個人儀表板的內容。
 * - 教師與管理者：同樣的「個人設定」卡片，內容是唯讀的帳號資訊與「更改密碼」按鈕；不能改的帳號（只用 Google 登入沒有內建密碼，或示範帳號）在按鈕的位置顯示說明。
 */
export function AccountPage() {
  const t = useT()
  const auth = useAuth()
  // 登入者載入完成前不顯示（外框保證實際使用時一定已登入）
  if (auth.status !== 'authed') return null
  if (auth.me.roles.student) {
    return (
      <Page title={t('account.title')}>
        <ProfileForm />
        <PersonalDashboardContent />
      </Page>
    )
  }
  return (
    <Page title={t('account.title')}>
      <StaffSettings me={auth.me} />
    </Page>
  )
}
