import { BarChart3, FlaskConical, RefreshCw, Trash2 } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { nativeSelectClass } from '@/components/native-select-class'
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
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { UsagePanel } from '@/features/keys/usage-panel'
import { errorText, useI18n, useT, type MessageKey } from '@/i18n'
import { api } from '@/lib/api'
import type { AiProvider, KeyStatus, TeacherKey } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 狀態徽章：`ok` 之外都是無法使用或受限制的原因類別。 */
export function StatusBadge({ status }: { status: KeyStatus }) {
  const t = useT()
  return <Badge variant={status === 'ok' ? 'secondary' : 'destructive'}>{t(`keys.status.${status}` as MessageKey)}</Badge>
}

/** 教師管理自己的 API key：新增、更換、刪除、測試、看用量（F-40.1，S-03.1）。 */
export function KeysPage() {
  const t = useT()
  const { lang } = useI18n()
  const providers = useFetch<AiProvider[]>('/api/teacher/ai-providers')
  const keys = useFetch<TeacherKey[]>('/api/teacher/keys')
  const names = useMemo(() => new Map((providers.data ?? []).map((p) => [p.id, p.name])), [providers.data])
  const nameOf = (id: string) => names.get(id) ?? id
  const time = useMemo(() => new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeStyle: 'short' }), [lang])

  const [provider, setProvider] = useState('')
  const [secret, setSecret] = useState('')
  const [adding, setAdding] = useState(false)
  const [addError, setAddError] = useState<unknown>()
  const [notice, setNotice] = useState<string>()

  const [toReplace, setToReplace] = useState<TeacherKey>()
  const [replaceSecret, setReplaceSecret] = useState('')
  const [replacing, setReplacing] = useState(false)
  const [replaceError, setReplaceError] = useState<unknown>()

  const [toDelete, setToDelete] = useState<TeacherKey>()
  const [deleting, setDeleting] = useState(false)
  const [deleteError, setDeleteError] = useState<unknown>()

  const [testing, setTesting] = useState<string>()
  const [results, setResults] = useState<Record<string, 'usable' | 'connection_failed' | { error: unknown }>>({})
  const [usageOf, setUsageOf] = useState<string>()

  async function onAdd(e: FormEvent) {
    e.preventDefault()
    if (!provider || !secret.trim() || adding) return
    setAdding(true)
    setAddError(undefined)
    setNotice(undefined)
    try {
      await api.post('/api/teacher/keys', { provider, key: secret })
      setSecret('')
      setNotice(t('keys.add.done', { provider: nameOf(provider) }))
      keys.reload()
    } catch (err) {
      setAddError(err)
    } finally {
      setAdding(false)
    }
  }

  async function onReplace(e: FormEvent) {
    e.preventDefault()
    if (!toReplace || !replaceSecret.trim() || replacing) return
    setReplacing(true)
    setReplaceError(undefined)
    try {
      await api.put(`/api/teacher/keys/${encodeURIComponent(toReplace.id)}`, { key: replaceSecret })
      setNotice(t('keys.replace.done', { provider: nameOf(toReplace.provider) }))
      setToReplace(undefined)
      setReplaceSecret('')
      keys.reload()
    } catch (err) {
      setReplaceError(err)
    } finally {
      setReplacing(false)
    }
  }

  async function onDelete() {
    if (!toDelete || deleting) return
    setDeleting(true)
    setDeleteError(undefined)
    try {
      await api.delete(`/api/teacher/keys/${encodeURIComponent(toDelete.id)}`)
      setToDelete(undefined)
      keys.reload()
    } catch (err) {
      setDeleteError(err)
    } finally {
      setDeleting(false)
    }
  }

  async function onTest(key: TeacherKey) {
    if (testing) return
    setTesting(key.id)
    try {
      const res = await api.post<{ result: 'usable' | 'connection_failed' }>(`/api/teacher/keys/${encodeURIComponent(key.id)}/test`)
      setResults((r) => ({ ...r, [key.id]: res.result }))
    } catch (err) {
      setResults((r) => ({ ...r, [key.id]: { error: err } }))
    } finally {
      setTesting(undefined)
    }
  }

  return (
    <Page title={t('keys.title')} description={t('keys.intro')}>
      {providers.error ? <ErrorBlock error={providers.error} onRetry={providers.reload} /> : null}

      <form onSubmit={onAdd} className="space-y-3 rounded-md border p-4" noValidate aria-labelledby="keys-add-title">
        <h2 id="keys-add-title" className="text-base font-medium">
          {t('keys.add.title')}
        </h2>
        <div className="grid gap-3 sm:grid-cols-[14rem_1fr_auto] sm:items-end">
          <div className="space-y-1.5">
            <Label htmlFor="key-provider">{t('keys.add.provider')}</Label>
            <select id="key-provider" className={nativeSelectClass} value={provider} onChange={(e) => setProvider(e.target.value)}>
              <option value="">{t('keys.add.providerPlaceholder')}</option>
              {(providers.data ?? []).map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="key-secret">{t('keys.add.key')}</Label>
            <Input
              id="key-secret"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
              aria-invalid={addError ? true : undefined}
            />
          </div>
          <Button type="submit" disabled={adding || !provider || !secret.trim()}>
            {adding ? t('keys.add.submitting') : t('keys.add.submit')}
          </Button>
        </div>
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
      </form>

      {keys.loading && !keys.data ? <LoadingBlock /> : null}
      {keys.error ? <ErrorBlock error={keys.error} onRetry={keys.reload} /> : null}
      {keys.data ? (
        keys.data.length === 0 ? (
          <EmptyBlock>{t('keys.empty')}</EmptyBlock>
        ) : (
          <ul aria-label={t('keys.list')} className="divide-y rounded-md border">
            {keys.data.map((k) => {
              const result = results[k.id]
              return (
                <li key={k.id} className="space-y-3 px-3 py-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
                      <span className="font-medium">{nameOf(k.provider)}</span>
                      <code className="text-muted-foreground text-sm">{k.mask}</code>
                      <StatusBadge status={k.status} />
                      {k.status !== 'ok' && k.status_at ? (
                        <span className="text-muted-foreground text-xs">{t('keys.status.since', { time: time.format(new Date(k.status_at)) })}</span>
                      ) : null}
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-1">
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={t('keys.test.label', { provider: nameOf(k.provider), mask: k.mask })}
                        disabled={testing !== undefined}
                        onClick={() => void onTest(k)}
                      >
                        <FlaskConical aria-hidden />
                        {testing === k.id ? t('keys.test.running') : t('keys.test')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={t('keys.usage.label', { provider: nameOf(k.provider), mask: k.mask })}
                        aria-expanded={usageOf === k.id}
                        onClick={() => setUsageOf(usageOf === k.id ? undefined : k.id)}
                      >
                        <BarChart3 aria-hidden />
                        {usageOf === k.id ? t('keys.usage.hide') : t('keys.usage')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={t('keys.replace.label', { provider: nameOf(k.provider), mask: k.mask })}
                        onClick={() => {
                          setReplaceError(undefined)
                          setReplaceSecret('')
                          setToReplace(k)
                        }}
                      >
                        <RefreshCw aria-hidden />
                        {t('keys.replace')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={t('keys.delete.label', { provider: nameOf(k.provider), mask: k.mask })}
                        onClick={() => {
                          setDeleteError(undefined)
                          setToDelete(k)
                        }}
                      >
                        <Trash2 aria-hidden />
                        {t('keys.delete')}
                      </Button>
                    </div>
                  </div>
                  <p className="text-muted-foreground text-xs">{t('keys.test.notice')}</p>
                  <div aria-live="polite">
                    {result === 'usable' ? (
                      <p role="status" className="text-sm text-green-700 dark:text-green-400">
                        {t('keys.test.usable')}
                      </p>
                    ) : result === 'connection_failed' ? (
                      <p role="status" className="text-destructive text-sm">
                        {t('keys.test.failed')}
                      </p>
                    ) : result ? (
                      <Alert variant="destructive" role="alert">
                        <AlertTitle>{errorText(t, result.error)}</AlertTitle>
                      </Alert>
                    ) : null}
                  </div>
                  {usageOf === k.id ? <UsagePanel idPrefix={`key-${k.id}`} path={`/api/teacher/keys/${encodeURIComponent(k.id)}/usage`} providers={providers.data} /> : null}
                </li>
              )
            })}
          </ul>
        )
      ) : null}

      <Dialog open={toReplace !== undefined} onOpenChange={(o) => !o && !replacing && setToReplace(undefined)}>
        <DialogContent>
          <form onSubmit={onReplace} className="space-y-4" noValidate>
            <DialogHeader>
              <DialogTitle>{t('keys.replace.title', { provider: toReplace ? nameOf(toReplace.provider) : '' })}</DialogTitle>
              <DialogDescription>{t('keys.replace.providerFixed')}</DialogDescription>
            </DialogHeader>
            <div className="space-y-1.5">
              <Label htmlFor="replace-provider">{t('keys.add.provider')}</Label>
              <Input id="replace-provider" readOnly value={toReplace ? nameOf(toReplace.provider) : ''} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="replace-secret">{t('keys.add.key')}</Label>
              <Input
                id="replace-secret"
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={replaceSecret}
                onChange={(e) => setReplaceSecret(e.target.value)}
              />
            </div>
            {replaceError ? (
              <Alert variant="destructive" role="alert">
                <AlertTitle>{errorText(t, replaceError)}</AlertTitle>
              </Alert>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="outline" disabled={replacing} onClick={() => setToReplace(undefined)}>
                {t('common.cancel')}
              </Button>
              <Button type="submit" disabled={replacing || !replaceSecret.trim()}>
                {replacing ? t('keys.replace.submitting') : t('keys.replace.submit')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog open={toDelete !== undefined} onOpenChange={(o) => !o && !deleting && setToDelete(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('keys.delete.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('keys.delete.body')}</AlertDialogDescription>
          </AlertDialogHeader>
          {deleteError ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, deleteError)}</AlertTitle>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>{t('common.cancel')}</AlertDialogCancel>
            <Button variant="destructive" disabled={deleting} onClick={() => void onDelete()}>
              {deleting ? t('keys.delete.deleting') : t('keys.delete.confirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  )
}
