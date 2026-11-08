import { useState } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'

type Props = {
  title: string
  description: string
  /** 使用者必須輸入的名稱（比對時去除前後空白）。 */
  name: string
  onConfirm: () => Promise<void>
  onCancel: () => void
}

/** 永久刪除前的確認：輸入名稱才能按下刪除（S-14）。 */
export function NameConfirmDialog({ title, description, name, onConfirm, onCancel }: Props) {
  const t = useT()
  const [typed, setTyped] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()

  async function confirm() {
    if (pending) return
    setPending(true)
    setError(undefined)
    try {
      await onConfirm()
    } catch (caught) {
      setError(caught)
      setPending(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && !pending && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label htmlFor="confirm-name">{t('confirmName.label', { name })}</Label>
          <Input id="confirm-name" autoComplete="off" value={typed} onChange={(event) => setTyped(event.target.value)} />
        </div>
        {error ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, error)}</AlertTitle>
          </Alert>
        ) : null}
        <DialogFooter>
          <Button type="button" variant="outline" disabled={pending} onClick={onCancel}>
            {t('common.cancel')}
          </Button>
          <Button type="button" variant="destructive" disabled={pending || typed.trim() !== name.trim()} onClick={() => void confirm()}>
            {t('confirmName.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
