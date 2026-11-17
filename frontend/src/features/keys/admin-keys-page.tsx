import { Trash2 } from 'lucide-react'
import { useMemo, useState } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
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
import { StatusBadge } from '@/features/keys/keys-page'
import { errorText, useI18n, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { AdminTeacherKey } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/**
 * 管理者檢視各教師 key 的設定狀態並刪除（F-40.9，S-03.1）：
 * 看不到用量與完整 key，也不能新增或更換；刪除會通知教師並寫入稽核紀錄。
 */
export function AdminKeysPage() {
  const t = useT()
  const { lang } = useI18n()
  const keys = useFetch<AdminTeacherKey[]>('/api/admin/teacher-keys')
  const time = useMemo(() => new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeStyle: 'short' }), [lang])

  const [toDelete, setToDelete] = useState<AdminTeacherKey>()
  const [deleting, setDeleting] = useState(false)
  const [error, setError] = useState<unknown>()
  const [notice, setNotice] = useState<string>()

  async function onDelete() {
    if (!toDelete || deleting) return
    setDeleting(true)
    setError(undefined)
    setNotice(undefined)
    try {
      await api.delete(`/api/admin/teacher-keys/${encodeURIComponent(toDelete.id)}`)
      setNotice(t('adminKeys.deleted', { email: toDelete.owner_email }))
      setToDelete(undefined)
      keys.reload()
    } catch (err) {
      setError(err)
      // 404：已經不在了，關閉對話框並重新載入；其他錯誤留在對話框內方便重試
      if (isApiError(err) && err.status === 404) {
        setToDelete(undefined)
        keys.reload()
      }
    } finally {
      setDeleting(false)
    }
  }

  return (
    <Page title={t('adminKeys.title')} description={t('adminKeys.intro')}>
      <div aria-live="polite">
        {notice ? (
          <p role="status" className="text-sm text-green-700 dark:text-green-400">
            {notice}
          </p>
        ) : null}
        {error && toDelete === undefined ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, error)}</AlertTitle>
          </Alert>
        ) : null}
      </div>
      {keys.loading && !keys.data ? <LoadingBlock /> : null}
      {keys.error ? <ErrorBlock error={keys.error} onRetry={keys.reload} /> : null}
      {keys.data ? (
        keys.data.length === 0 ? (
          <EmptyBlock>{t('adminKeys.empty')}</EmptyBlock>
        ) : (
          <ul aria-label={t('adminKeys.list')} className="divide-y rounded-md border">
            {keys.data.map((k) => (
              <li key={k.id} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
                <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="min-w-0 text-sm break-all">{k.owner_email}</span>
                  <span className="font-medium">{k.provider_name}</span>
                  <code className="text-muted-foreground text-sm">{k.mask}</code>
                  <StatusBadge status={k.status} />
                  {k.status !== 'ok' && k.status_at ? (
                    <span className="text-muted-foreground text-xs">{t('keys.status.since', { time: time.format(new Date(k.status_at)) })}</span>
                  ) : null}
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={t('adminKeys.delete.label', { email: k.owner_email, provider: k.provider_name, mask: k.mask })}
                  onClick={() => {
                    setError(undefined)
                    setToDelete(k)
                  }}
                >
                  <Trash2 aria-hidden />
                </Button>
              </li>
            ))}
          </ul>
        )
      ) : null}

      <AlertDialog open={toDelete !== undefined} onOpenChange={(o) => !o && !deleting && setToDelete(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('adminKeys.delete.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('adminKeys.delete.body', { email: toDelete?.owner_email ?? '', provider: toDelete?.provider_name ?? '' })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {error && toDelete !== undefined ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, error)}</AlertTitle>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>{t('common.cancel')}</AlertDialogCancel>
            <Button variant="destructive" disabled={deleting} onClick={() => void onDelete()}>
              {deleting ? t('adminKeys.delete.deleting') : t('adminKeys.delete.confirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  )
}
