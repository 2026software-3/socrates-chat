import { useState, type FormEvent } from 'react'
import { useAuth } from '@/auth/auth-context'
import { AuthCard, AuthFrame, AuthHeading } from '@/components/auth-frame'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'

/** 用臨時密碼登入後必須先改密碼（S-08.2）；成功後重新讀取登入者，外框就會放行。 */
export function ChangePasswordPage() {
  const t = useT()
  const { refresh } = useAuth()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const [mismatch, setMismatch] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setError(null)
    setMismatch(next !== confirm)
    if (next !== confirm) return
    setSubmitting(true)
    try {
      await api.post('/api/auth/change-password', { current_password: current, new_password: next })
      await refresh()
    } catch (err) {
      setError(err)
      setSubmitting(false)
    }
  }

  return (
    <AuthFrame>
      <AuthCard>
        <AuthHeading title={t('password.title')} description={t('password.intro')} />
        <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-4">
          {mismatch || error ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{mismatch ? t('password.mismatch') : errorText(t, error)}</AlertTitle>
            </Alert>
          ) : null}
          <div className="flex flex-col gap-2">
            <Label htmlFor="pw-current">{t('password.current')}</Label>
            <Input id="pw-current" type="password" autoComplete="current-password" required value={current} onChange={(e) => setCurrent(e.target.value)} />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="pw-new">{t('password.new')}</Label>
            <Input
              id="pw-new"
              type="password"
              autoComplete="new-password"
              required
              minLength={8}
              maxLength={128}
              aria-describedby="pw-hint"
              value={next}
              onChange={(e) => setNext(e.target.value)}
            />
            <p id="pw-hint" className="text-muted-foreground text-xs">
              {t('password.hint')}
            </p>
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="pw-confirm">{t('password.confirm')}</Label>
            <Input id="pw-confirm" type="password" autoComplete="new-password" required value={confirm} onChange={(e) => setConfirm(e.target.value)} />
          </div>
          <Button type="submit" size="lg" className="mt-2 w-full" disabled={submitting}>
            {t('password.submit')}
          </Button>
        </form>
      </AuthCard>
    </AuthFrame>
  )
}
