import { useState, type FormEvent } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'

type Result = { kind: 'mismatch' } | { kind: 'error'; error: unknown } | null

/**
 * 帳號頁的「密碼」列（S-18）：平常只有一個「更改密碼」按鈕，表單在彈出視窗裡，不攤開在頁面上。
 * 成功後視窗關閉，確認訊息留在列下方；再次開啟時重新開始（輸入與錯誤都不保留）。
 */
export function ChangePasswordSection() {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [saved, setSaved] = useState(false)
  return (
    <Card>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-medium">{t('profile.password.heading')}</h2>
          <Button
            variant="outline"
            onClick={() => {
              setSaved(false)
              setOpen(true)
            }}
          >
            {t('profile.password.title')}
          </Button>
        </div>
        {saved ? (
          <Alert role="status">
            <AlertTitle>{t('profile.password.saved')}</AlertTitle>
          </Alert>
        ) : null}
      </CardContent>
      <Dialog open={open} onOpenChange={setOpen}>
        {/* 內容只在開啟時才掛載：關閉就丟掉輸入，密碼不留在元件裡 */}
        <DialogContent aria-describedby={undefined}>
          <DialogHeader>
            <DialogTitle>{t('profile.password.title')}</DialogTitle>
          </DialogHeader>
          <PasswordForm
            onSaved={() => {
              setOpen(false)
              setSaved(true)
            }}
          />
        </DialogContent>
      </Dialog>
    </Card>
  )
}

/** 同一個 `POST /api/auth/change-password`，沿用強制改密碼頁的欄位與規則；失敗時保留輸入。 */
function PasswordForm({ onSaved }: { onSaved: () => void }) {
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
      onSaved()
    } catch (error) {
      setResult({ kind: 'error', error })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-3">
      {result ? (
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
    </div>
  )
}
