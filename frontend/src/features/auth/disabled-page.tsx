import { useAuth } from '@/auth/auth-context'
import { LangSwitcher } from '@/components/lang-switcher'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useT } from '@/i18n'

/** 帳號已被管理者停用（S-01.3）：只看得到說明與登出，其他功能一律不可用。 */
export function DisabledPage() {
  const t = useT()
  const auth = useAuth()
  return (
    <main className="bg-muted/40 flex min-h-svh items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle className="text-xl">{t('disabled.title')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <p>{t('disabled.body')}</p>
          <div className="flex items-center justify-between gap-2">
            <LangSwitcher />
            <Button variant="outline" onClick={() => void auth.logout()}>
              {t('nav.logout')}
            </Button>
          </div>
        </CardContent>
      </Card>
    </main>
  )
}
