import { useState, type FormEvent } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'

type Result = { kind: 'mismatch' } | { kind: 'error'; error: unknown } | null

/**
 * 「更改密碼」的彈出視窗（S-18）：表單不攤開在頁面上，按下按鈕才出現。
 * 內容只在開啟時才掛載：關閉（含 Esc）就丟掉輸入與錯誤，再次開啟從空白開始，密碼不留在元件裡。
 * 注意：視窗要放在任何 `<form>` 的外面。React 的事件會穿過 portal 往上冒泡，
 * 放在個人設定表單裡面，視窗內送出密碼會連帶觸發外層表單的送出。
 */
export function ChangePasswordDialog({ open, onOpenChange, onSaved }: { open: boolean; onOpenChange: (open: boolean) => void; onSaved: () => void }) {
  const t = useT()
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby={undefined}>
        <DialogHeader>
          <DialogTitle>{t('profile.password.title')}</DialogTitle>
        </DialogHeader>
        <PasswordForm
          onSaved={() => {
            onOpenChange(false)
            onSaved()
          }}
        />
      </DialogContent>
    </Dialog>
  )
}

/** 教師與管理者的帳號頁：沒有個人設定，只有這一個按鈕；成功後確認訊息留在按鈕旁。 */
export function ChangePasswordButton() {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [saved, setSaved] = useState(false)
  return (
    <div className="flex flex-wrap items-center gap-3">
      <Button
        variant="outline"
        onClick={() => {
          setSaved(false)
          setOpen(true)
        }}
      >
        {t('profile.password.title')}
      </Button>
      {saved ? (
        <p role="status" className="text-sm">
          {t('profile.password.saved')}
        </p>
      ) : null}
      <ChangePasswordDialog open={open} onOpenChange={setOpen} onSaved={() => setSaved(true)} />
    </div>
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
