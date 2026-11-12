import { useState, type FormEvent } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'

type Result = { kind: 'saved' } | { kind: 'mismatch' } | { kind: 'error'; error: unknown } | null

/**
 * 個人設定的「更改密碼」（S-18）：同一個 `POST /api/auth/change-password`，沿用強制改密碼頁的欄位與規則。
 * 成功後目前的登入保持、其他裝置由後端登出；失敗時保留輸入，只清掉成功後的欄位。
 */
export function ChangePasswordSection() {
  const t = useT()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<Result>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setResult(null)
    if (next !== confirm) {
      setResult({ kind: 'mismatch' })
      return
    }
    setBusy(true)
    try {
      await api.post('/api/auth/change-password', { current_password: current, new_password: next })
      setCurrent('')
      setNext('')
      setConfirm('')
      setResult({ kind: 'saved' })
    } catch (error) {
      setResult({ kind: 'error', error })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card>
      <CardContent>
        <section aria-labelledby="change-password-title" className="max-w-sm space-y-3">
          <h2 id="change-password-title" className="text-lg font-medium">
            {t('profile.password.title')}
          </h2>
          {result?.kind === 'saved' ? (
            <Alert role="status">
              <AlertTitle>{t('profile.password.saved')}</AlertTitle>
            </Alert>
          ) : null}
          {result && result.kind !== 'saved' ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{result.kind === 'mismatch' ? t('password.mismatch') : errorText(t, result.error)}</AlertTitle>
            </Alert>
          ) : null}
          <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-3">
            <div className="flex flex-col gap-2">
              <Label htmlFor="settings-pw-current">{t('password.current')}</Label>
              <Input
                id="settings-pw-current"
                type="password"
                autoComplete="current-password"
                required
                value={current}
                onChange={(e) => setCurrent(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="settings-pw-new">{t('password.new')}</Label>
              <Input
                id="settings-pw-new"
                type="password"
                autoComplete="new-password"
                required
                minLength={8}
                maxLength={128}
                aria-describedby="settings-pw-hint"
                value={next}
                onChange={(e) => setNext(e.target.value)}
              />
              <p id="settings-pw-hint" className="text-muted-foreground text-xs">
                {t('password.hint')}
              </p>
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="settings-pw-confirm">{t('password.confirm')}</Label>
              <Input
                id="settings-pw-confirm"
                type="password"
                autoComplete="new-password"
                required
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
              />
            </div>
            <Button type="submit" className="self-start" disabled={busy}>
              {t('password.submit')}
            </Button>
          </form>
        </section>
      </CardContent>
    </Card>
  )
}
