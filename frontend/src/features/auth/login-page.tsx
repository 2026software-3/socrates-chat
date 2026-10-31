import { useState, type FormEvent } from 'react'
import { Navigate, useSearchParams } from 'react-router'
import { useAuth } from '@/auth/auth-context'
import { AuthCard, AuthFrame, AuthHeading } from '@/components/auth-frame'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { dictionaries, errorText, useT, type MessageKey } from '@/i18n'
import { api } from '@/lib/api'
import { DemoAccounts } from './demo-accounts'

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
  const [account, setAccount] = useState('')
  const [password, setPassword] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<unknown>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setSubmitting(true)
    setFormError(null)
    try {
      await api.post('/api/auth/login', { account: account.trim(), password })
      // 登入成功後讀取登入者；auth 變成 authed 就會導向首頁（需改密碼者再由外框導向改密碼頁）
      await auth.refresh()
    } catch (err) {
      setFormError(err)
      setSubmitting(false)
    }
  }

  if (auth.status === 'authed') return <Navigate to="/" replace />

  return (
    <AuthFrame footer={t('login.footer')}>
      <AuthCard>
        <AuthHeading title={t('login.title')} description={t('login.subtitle')} />
        {errorKey ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{t(errorKey)}</AlertTitle>
          </Alert>
        ) : null}
        <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-4">
          {formError ? (
            <p role="alert" id="login-form-error" className="text-destructive text-sm">
              {errorText(t, formError)}
            </p>
          ) : null}
          <div className="flex flex-col gap-2">
            <Label htmlFor="login-account">{t('login.account')}</Label>
            <Input
              id="login-account"
              type="text"
              autoComplete="username"
              autoCapitalize="off"
              spellCheck={false}
              required
              value={account}
              onChange={(e) => setAccount(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="login-password">{t('login.password')}</Label>
            <Input
              id="login-password"
              type="password"
              autoComplete="current-password"
              required
              aria-invalid={formError ? true : undefined}
              aria-describedby={formError ? 'login-form-error' : undefined}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            <p className="text-muted-foreground text-xs">{t('login.forgot')}</p>
          </div>
          <Button type="submit" size="lg" className="mt-2 w-full" disabled={submitting}>
            {t('login.submit')}
          </Button>
        </form>
        <div className="text-muted-foreground flex items-center gap-3 text-xs" aria-hidden="true">
          <span className="bg-border h-px flex-1" />
          {t('login.or')}
          <span className="bg-border h-px flex-1" />
        </div>
        {/* 整頁導向：由後端與 Google 完成 OAuth，前端不處理任何 token */}
        <Button variant="outline" size="lg" className="w-full" onClick={() => window.location.assign('/api/auth/google/login')}>
          {t('login.google')}
        </Button>
        <p className="text-muted-foreground text-center text-xs">{t('login.noSignup')}</p>
        <DemoAccounts
          onPick={(a, p) => {
            setAccount(a)
            setPassword(p)
            setFormError(null)
          }}
        />
      </AuthCard>
    </AuthFrame>
  )
}
