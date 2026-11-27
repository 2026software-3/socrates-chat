import { KeyRound } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { useAuth } from '@/auth/auth-context'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ResetPasswordDialog, TemporaryPasswordDialog } from '@/features/admin/reset-password-dialog'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { AdminList } from '@/lib/types'

export function AdminsPage() {
  const t = useT()
  const { lang } = useI18n()
  const auth = useAuth()
  const { data, error, loading, reload } = useFetch<AdminList>('/api/admin/admins')

  const [email, setEmail] = useState('')
  const [adding, setAdding] = useState(false)
  const [addError, setAddError] = useState<unknown>()
  const [notice, setNotice] = useState<string>()
  const [issued, setIssued] = useState<{ label: string; password: string }>()
  const [toReset, setToReset] = useState<string>()

  const emails = useMemo(
    () => [...new Set(data?.emails ?? [])].sort((a, b) => a.localeCompare(b)),
    [data],
  )
  const primaryEmail = data?.primary_email?.toLowerCase() ?? null
  const myEmail = auth.status === 'authed' ? (auth.me.email?.toLowerCase() ?? null) : null
  const num = useMemo(() => new Intl.NumberFormat(lang), [lang])

  async function onAdd(e: FormEvent) {
    e.preventDefault()
    const value = email.trim()
    if (!value || adding) return
    setAdding(true)
    setAddError(undefined)
    setNotice(undefined)
    try {
      const res = await api.post<{ email: string; temporary_password: string | null }>('/api/admin/admins', {
        email: value,
      })
      setEmail('')
      if (res.temporary_password) {
        setIssued({ label: res.email, password: res.temporary_password })
      } else {
        setNotice(t('admin.admins.promoted', { email: res.email }))
      }
      reload()
    } catch (err) {
      setAddError(err)
    } finally {
      setAdding(false)
    }
  }

  return (
    <Page title={t('admin.admins.title')}>
      <p className="text-muted-foreground text-sm">{t('admin.admins.intro')}</p>

      <form onSubmit={onAdd} className="flex flex-col gap-2 sm:flex-row sm:items-end" noValidate>
        <div className="flex-1 space-y-1.5">
          <Label htmlFor="admin-email">{t('admin.admins.emailLabel')}</Label>
          <Input
            id="admin-email"
            type="email"
            autoComplete="off"
            value={email}
            placeholder={t('admin.admins.emailPlaceholder')}
            onChange={(e) => setEmail(e.target.value)}
            aria-invalid={addError ? true : undefined}
          />
        </div>
        <Button type="submit" disabled={adding || !email.trim()}>
          {adding ? t('admin.admins.adding') : t('admin.admins.add')}
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

      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? (
        <div className="space-y-3">
          <p className="text-sm font-medium" aria-live="polite">
            {t('admin.admins.count', { count: num.format(emails.length) })}
          </p>
          {emails.length === 0 ? (
            <EmptyBlock>{t('admin.admins.empty')}</EmptyBlock>
          ) : (
            <ul aria-label={t('admin.admins.list')} className="divide-y rounded-md border">
              {emails.map((e) => {
                const isPrimary = primaryEmail !== null && e.toLowerCase() === primaryEmail
                const canReset = !isPrimary || myEmail === primaryEmail
                return (
                  <li key={e} className="flex items-center justify-between gap-2 px-3 py-2">
                    <span className="flex min-w-0 flex-wrap items-center gap-2 text-sm">
                      <span className="break-all">{e}</span>
                      {isPrimary ? <Badge variant="secondary">{t('admin.admins.primary')}</Badge> : null}
                    </span>
                    {canReset ? (
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t('admin.reset.resetPassword', { email: e })}
                        onClick={() => setToReset(e)}
                      >
                        <KeyRound aria-hidden />
                      </Button>
                    ) : null}
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      ) : null}

      <TemporaryPasswordDialog
        issued={issued}
        title={t('admin.admins.tempTitle')}
        description={(who) => t('admin.admins.tempDesc', { who })}
        onClose={() => setIssued(undefined)}
      />
      <ResetPasswordDialog
        target={toReset === undefined ? undefined : { kind: 'email', email: toReset }}
        onClose={() => setToReset(undefined)}
      />
    </Page>
  )
}
