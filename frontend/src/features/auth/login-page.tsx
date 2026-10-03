import { Navigate, useSearchParams } from 'react-router'
import { useAuth } from '@/auth/auth-context'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { LangSwitcher } from '@/components/lang-switcher'
import { dictionaries, useT, type MessageKey } from '@/i18n'

/** 後端登入失敗時導回 `/login?error=<code>`（S-08.2）。 */
function loginErrorKey(code: string | null): MessageKey | null {
  if (!code) return null
  const key = `login.error.${code}` as MessageKey
  return dictionaries['zh-TW'][key] ? key : 'login.error.login_failed'
}

export function LoginPage() {
  const t = useT()
  const auth = useAuth()
  const [params] = useSearchParams()
  const errorKey = loginErrorKey(params.get('error'))

  if (auth.status === 'authed') return <Navigate to="/" replace />

  return (
    <main className="bg-muted/40 flex min-h-svh items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle className="text-xl">{t('app.name')}</CardTitle>
          <CardDescription>{t('login.subtitle')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {errorKey ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{t(errorKey)}</AlertTitle>
            </Alert>
          ) : null}
          {/* 整頁導向：由後端與 Google 完成 OAuth，前端不處理任何 token */}
          <Button className="w-full" onClick={() => window.location.assign('/api/auth/google/login')}>
            {t('login.google')}
          </Button>
          <div className="flex justify-end">
            <LangSwitcher />
          </div>
        </CardContent>
      </Card>
    </main>
  )
}
