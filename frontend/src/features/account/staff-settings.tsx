import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { usePasswordChange } from '@/features/dashboard/use-password-change'
import { useT } from '@/i18n'
import type { Me } from '@/lib/types'

/**
 * 教師與管理者的「個人設定」卡片（S-18）：與學生同一個標題與版型，內容是唯讀的帳號資訊
 * （顯示名稱、電子郵件、角色）與「更改密碼」按鈕。顯示名稱只有學生能改、Google 連結只給有學號的帳號，
 * 所以這裡沒有可輸入的欄位。不能改密碼的帳號在按鈕本來的位置顯示說明。
 */
export function StaffSettings({ me }: { me: Me }) {
  const t = useT()
  const password = usePasswordChange()
  const roles = (['admin', 'teacher', 'student'] as const)
    .filter((r) => me.roles[r])
    .map((r) => t(`role.${r}`))
    .join(' / ')
  const rows = [
    [t('profile.name'), me.display_name ?? '—'],
    [t('account.email'), me.email ?? '—'],
    [t('account.roles'), roles],
  ] as const
  return (
    <>
      <Card>
        <CardContent>
          <section aria-labelledby="staff-settings-title" className="max-w-sm space-y-3">
            <h2 id="staff-settings-title" className="text-lg font-medium">
              {t('profile.title')}
            </h2>
            <dl className="space-y-3 text-sm">
              {rows.map(([label, value]) => (
                <div key={label} className="space-y-1">
                  <dt className="text-muted-foreground text-xs">{label}</dt>
                  <dd className="break-words">{value}</dd>
                </div>
              ))}
            </dl>
            <div className="flex flex-wrap items-center gap-3">
              {me.can_change_password ? (
                <Button type="button" variant="outline" onClick={password.start}>
                  {t('profile.password.title')}
                </Button>
              ) : (
                <p className="text-muted-foreground text-sm">{t('account.unavailable')}</p>
              )}
              {password.saved ? (
                <p role="status" className="text-sm">
                  {t('profile.password.saved')}
                </p>
              ) : null}
            </div>
          </section>
        </CardContent>
      </Card>
      {me.can_change_password ? password.dialog : null}
    </>
  )
}
