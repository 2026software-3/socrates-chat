import { useAuth } from '@/auth/auth-context'
import { Page } from '@/components/page'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/vedute/tabs'
import { PersonalDashboardContent } from '@/features/dashboard/personal-dashboard-page'
import { PrivacySettingsSection } from '@/features/dashboard/privacy-settings'
import { ProfileForm } from '@/features/dashboard/profile-form'
import { useT } from '@/i18n'

/**
 * 個人檔案／帳號頁（S-18、F-41.17）：
 * - 學生：主標題為「個人檔案」，下方以 Vedute Tabs 分成「個人設定」「統計資料」「隱私設定」三個頁籤。
 * - 教師與管理者：主標題維持「帳號」，維持原本的「個人設定」表單（顯示名稱、更改密碼與唯讀帳號資訊）。
 */
export function AccountPage() {
  const t = useT()
  const auth = useAuth()
  if (auth.status !== 'authed') return null

  if (auth.me.roles.student) {
    return (
      <Page title={t('account.profileTitle')}>
        <Tabs defaultValue="settings" className="space-y-6">
          <TabsList aria-label={t('account.tabs.label')}>
            <TabsTrigger value="settings">{t('account.tabs.settings')}</TabsTrigger>
            <TabsTrigger value="stats">{t('account.tabs.stats')}</TabsTrigger>
            <TabsTrigger value="privacy">{t('account.tabs.privacy')}</TabsTrigger>
          </TabsList>

          <TabsContent value="settings" className="space-y-6">
            <ProfileForm />
          </TabsContent>

          <TabsContent value="stats">
            <PersonalDashboardContent />
          </TabsContent>

          <TabsContent value="privacy">
            <PrivacySettingsSection />
          </TabsContent>
        </Tabs>
      </Page>
    )
  }

  return (
    <Page title={t('account.title')}>
      <ProfileForm />
    </Page>
  )
}