import { useState, type FormEvent } from 'react'
import { Navigate, useSearchParams } from 'react-router'
import { useAuth } from '@/auth/auth-context'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { LangSwitcher } from '@/components/lang-switcher'
import { dictionaries, errorText, useT, type MessageKey } from '@/i18n'
import { api } from '@/lib/api'

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
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<unknown>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setSubmitting(true)
    setFormError(null)
    try {
      await api.post('/api/auth/login', { email: email.trim(), password })
      // 登入成功後讀取登入者；auth 變成 authed 就會導向首頁（需改密碼者再由外框導向改密碼頁）
      await auth.refresh()
    } catch (err) {
      setFormError(err)
      setSubmitting(false)
    }
  }

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
          <form onSubmit={(e) => void submit(e)} className="space-y-3">
            {formError ? (
              <Alert variant="destructive" role="alert">
                <AlertTitle>{errorText(t, formError)}</AlertTitle>
              </Alert>
            ) : null}
            <div className="space-y-1.5">
              <Label htmlFor="login-email">{t('login.email')}</Label>
              <Input
                id="login-email"
                type="email"
                autoComplete="username"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="login-password">{t('login.password')}</Label>
              <Input
                id="login-password"
                type="password"
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            <Button type="submit" className="w-full" disabled={submitting}>
              {t('login.submit')}
            </Button>
          </form>
          <div className="text-muted-foreground flex items-center gap-2 text-xs" aria-hidden="true">
            <span className="bg-border h-px flex-1" />
            {t('login.or')}
            <span className="bg-border h-px flex-1" />
          </div>
          {/* 整頁導向：由後端與 Google 完成 OAuth，前端不處理任何 token */}
          <Button variant="outline" className="w-full" onClick={() => window.location.assign('/api/auth/google/login')}>
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
