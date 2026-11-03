import { useState } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'

/**
 * 要重設的帳號：有電子郵件的帳號以信箱指定；沒有電子郵件的學生以學號指定（#413），
 * 對話框會顯示姓名與所屬教室供管理者核對。
 */
export type ResetTarget =
  | { kind: 'email'; email: string }
  | { kind: 'student'; studentId: string; name: string | null; classroom: string }

/** 對話框中指稱這個帳號的文字：有姓名就「姓名（學號）」。 */
function labelOf(target: ResetTarget): string {
  if (target.kind === 'email') return target.email
  return target.name ? `${target.name}（${target.studentId}）` : target.studentId
}

/**
 * 管理者重設某個帳號的密碼：先確認，再把臨時密碼顯示一次（教師與學生共用）。
 * `target` 為 undefined 時不顯示；結束（取消或已記下）時呼叫 `onClose`。
 */
export function ResetPasswordDialog({ target, onClose }: { target: ResetTarget | undefined; onClose: () => void }) {
  const t = useT()
  const [resetting, setResetting] = useState(false)
  const [error, setError] = useState<unknown>()
  const [issued, setIssued] = useState<{ label: string; password: string; byStudentId: boolean }>()
  const { isCopied, copyToClipboard } = useCopyToClipboard()

  async function onReset() {
    if (!target || resetting) return
    setResetting(true)
    setError(undefined)
    try {
      const body = target.kind === 'email' ? { email: target.email } : { student_id: target.studentId }
      const res = await api.post<{ email: string | null; temporary_password: string }>(
        '/api/admin/users/reset-password',
        body,
      )
      setIssued({ label: labelOf(target), password: res.temporary_password, byStudentId: target.kind === 'student' })
    } catch (err) {
      setError(err)
    } finally {
      setResetting(false)
    }
  }

  function close() {
    setIssued(undefined)
    setError(undefined)
    onClose()
  }

  return (
    <>
      <AlertDialog open={target !== undefined && !issued} onOpenChange={(o) => !o && !resetting && close()}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('admin.reset.resetTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {target?.kind === 'student'
                ? t('admin.reset.studentDesc', { who: labelOf(target), classroom: target.classroom })
                : t('admin.reset.resetDesc', { email: target ? labelOf(target) : '' })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {error ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, error)}</AlertTitle>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={resetting}>{t('common.cancel')}</AlertDialogCancel>
            <Button disabled={resetting} onClick={onReset}>
              {resetting ? t('admin.reset.resetting') : t('admin.reset.resetConfirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 臨時密碼只出現這一次：不能點空白處或按 Esc 關閉，要按按鈕確認已記下 */}
      <AlertDialog open={issued !== undefined}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('admin.reset.tempTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {issued?.byStudentId
                ? t('admin.reset.tempDescWho', { who: issued.label })
                : t('admin.reset.tempDesc', { email: issued?.label ?? '' })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="space-y-1.5">
            <Label htmlFor="temp-password">{t('admin.reset.tempLabel')}</Label>
            <div className="flex gap-2">
              <Input
                id="temp-password"
                readOnly
                className="font-mono"
                value={issued?.password ?? ''}
                onFocus={(e) => e.target.select()}
              />
              <Button type="button" variant="outline" onClick={() => copyToClipboard(issued?.password ?? '')}>
                {isCopied ? t('admin.reset.copied') : t('admin.reset.copy')}
              </Button>
            </div>
          </div>
          <AlertDialogFooter>
            <Button onClick={close}>{t('admin.reset.tempClose')}</Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
