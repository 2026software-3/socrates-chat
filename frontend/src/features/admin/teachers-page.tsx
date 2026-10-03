import { Trash2 } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useI18n, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { EmailList } from '@/lib/types'

/** 教師管理：新增／移除教師身分（只有管理者進得來）。 */
export function TeachersPage() {
  const t = useT()
  const { lang } = useI18n()
  const { data, error, loading, reload } = useFetch<EmailList>('/api/admin/teachers')

  const [email, setEmail] = useState('')
  const [adding, setAdding] = useState(false)
  const [addError, setAddError] = useState<unknown>()
  const [notice, setNotice] = useState<string>()
  const [query, setQuery] = useState('')
  const [toRemove, setToRemove] = useState<string>()
  const [removing, setRemoving] = useState(false)
  const [removeError, setRemoveError] = useState<unknown>()

  const emails = useMemo(
    () => [...new Set(data?.emails ?? [])].sort((a, b) => a.localeCompare(b)),
    [data],
  )
  const q = query.trim().toLowerCase()
  const shown = q ? emails.filter((e) => e.toLowerCase().includes(q)) : emails
  const num = new Intl.NumberFormat(lang)

  async function onAdd(e: FormEvent) {
    e.preventDefault()
    const value = email.trim()
    if (!value || adding) return
    setAdding(true)
    setAddError(undefined)
    setNotice(undefined)
    try {
      await api.post('/api/admin/teachers', { email: value })
      setEmail('')
      setNotice(t('admin.teachers.added', { email: value }))
      reload()
    } catch (err) {
      setAddError(err)
    } finally {
      setAdding(false)
    }
  }

  async function onRemove() {
    if (!toRemove || removing) return
    setRemoving(true)
    setRemoveError(undefined)
    setNotice(undefined)
    try {
      await api.delete(`/api/admin/teachers/${encodeURIComponent(toRemove)}`)
      setToRemove(undefined)
      reload()
    } catch (err) {
      setRemoveError(err)
      // 404：已經不在清單裡，關閉對話框並重新載入；其他錯誤留在對話框內，方便重試
      if (isApiError(err) && err.status === 404) {
        setToRemove(undefined)
        reload()
      }
    } finally {
      setRemoving(false)
    }
  }

  return (
    <Page title={t('admin.teachers.title')}>
      <p className="text-muted-foreground text-sm">{t('admin.teachers.intro')}</p>

      <form onSubmit={onAdd} className="flex flex-col gap-2 sm:flex-row sm:items-end" noValidate>
        <div className="flex-1 space-y-1.5">
          <Label htmlFor="teacher-email">{t('admin.teachers.emailLabel')}</Label>
          <Input
            id="teacher-email"
            type="email"
            autoComplete="off"
            value={email}
            placeholder={t('admin.teachers.emailPlaceholder')}
            onChange={(e) => setEmail(e.target.value)}
            aria-invalid={addError ? true : undefined}
          />
        </div>
        <Button type="submit" disabled={adding || !email.trim()}>
          {adding ? t('admin.teachers.adding') : t('admin.teachers.add')}
        </Button>
      </form>
      <div aria-live="polite">
        {addError ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, addError)}</AlertTitle>
          </Alert>
        ) : null}
        {notice ? (
          <p role="status" className="text-sm text-green-700 dark:text-green-400">
            {notice}
          </p>
        ) : null}
      </div>

      {removeError && toRemove === undefined ? (
        <Alert variant="destructive" role="alert">
          <AlertTitle>{errorText(t, removeError)}</AlertTitle>
        </Alert>
      ) : null}

      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? (
        <div className="space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <p className="text-sm font-medium" aria-live="polite">
              {q
                ? t('admin.teachers.countFiltered', { shown: num.format(shown.length), count: num.format(emails.length) })
                : t('admin.teachers.count', { count: num.format(emails.length) })}
            </p>
            {emails.length > 0 ? (
              <div className="sm:w-64">
                <Label htmlFor="teacher-search" className="sr-only">
                  {t('admin.teachers.searchLabel')}
                </Label>
                <Input
                  id="teacher-search"
                  type="search"
                  value={query}
                  placeholder={t('admin.teachers.searchLabel')}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </div>
            ) : null}
          </div>
          {emails.length === 0 ? (
            <EmptyBlock>{t('admin.teachers.empty')}</EmptyBlock>
          ) : shown.length === 0 ? (
            <EmptyBlock>{t('admin.teachers.noMatch')}</EmptyBlock>
          ) : (
            <ul aria-label={t('admin.teachers.list')} className="divide-y rounded-md border">
              {shown.map((e) => (
                <li key={e} className="flex items-center justify-between gap-2 px-3 py-2">
                  <span className="min-w-0 break-all text-sm">{e}</span>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={t('admin.teachers.remove', { email: e })}
                    onClick={() => {
                      setRemoveError(undefined)
                      setToRemove(e)
                    }}
                  >
                    <Trash2 aria-hidden />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}

      <AlertDialog open={toRemove !== undefined} onOpenChange={(o) => !o && !removing && setToRemove(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('admin.teachers.removeTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('admin.teachers.removeDesc', { email: toRemove ?? '' })}</AlertDialogDescription>
          </AlertDialogHeader>
          {removeError && toRemove !== undefined ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, removeError)}</AlertTitle>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={removing}>{t('common.cancel')}</AlertDialogCancel>
            <Button variant="destructive" disabled={removing} onClick={onRemove}>
              {removing ? t('admin.teachers.removing') : t('admin.teachers.removeConfirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  )
}
