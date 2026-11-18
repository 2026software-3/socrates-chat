import { FlaskConical, RefreshCw, Trash2 } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { nativeSelectClass } from '@/components/native-select-class'
import { ErrorBlock, LoadingBlock, Page } from '@/components/page'
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
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ModelFields } from '@/features/keys/ai-settings-page'
import { StatusBadge } from '@/features/keys/keys-page'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { SystemAiKey, SystemAiOverview } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

const BASE = '/api/admin/system-ai'

/** 管理者在介面管理系統 AI：供應商、模型與多把系統 key（加密存資料庫，沒設定時用主機的 .apikeylist）。 */
export function SystemAiPage() {
  const t = useT()
  const { lang } = useI18n()
  const data = useFetch<SystemAiOverview>(BASE)
  const nf = useMemo(() => new Intl.NumberFormat(lang), [lang])
  const [provider, setProvider] = useState('')
  const [secret, setSecret] = useState('')
  const [model, setModel] = useState<string>()
  const [busy, setBusy] = useState<string>()
  const [error, setError] = useState<unknown>()
  const [notice, setNotice] = useState<string>()
  const [toReplace, setToReplace] = useState<SystemAiKey>()
  const [replaceSecret, setReplaceSecret] = useState('')
  const [toDelete, setToDelete] = useState<SystemAiKey>()
  const [results, setResults] = useState<Record<string, string>>({})

  const overview = data.data
  const settingModel = model ?? overview?.setting?.model ?? ''
  const settingProvider = provider || overview?.setting?.provider || ''

  async function run(name: string, action: () => Promise<unknown>, done?: string, close?: () => void) {
    if (busy) return
    setBusy(name)
    setError(undefined)
    setNotice(undefined)
    try {
      await action()
      if (done) setNotice(done)
      close?.()
      data.reload()
    } catch (err) {
      setError(err)
    } finally {
      setBusy(undefined)
    }
  }

  function onAdd(e: FormEvent) {
    e.preventDefault()
    void run('add', () => api.post(`${BASE}/keys`, { provider, key: secret.trim() }), t('systemAi.added'), () => setSecret(''))
  }

  function onSaveModel(e: FormEvent) {
    e.preventDefault()
    void run(
      'model',
      () => api.put(`${BASE}/model`, { provider: settingProvider, model: settingModel.trim() }),
      t('systemAi.modelSaved'),
      () => setModel(undefined),
    )
  }

  async function onTest(k: SystemAiKey) {
    setResults((r) => ({ ...r, [k.id]: '…' }))
    try {
      const res = await api.post<{ result: string }>(`${BASE}/keys/${encodeURIComponent(k.id)}/test`)
      setResults((r) => ({ ...r, [k.id]: res.result === 'usable' ? t('keys.test.usable') : t('keys.test.failed') }))
      data.reload()
    } catch (err) {
      setResults((r) => ({ ...r, [k.id]: errorText(t, err) }))
    }
  }

  return (
    <Page title={t('systemAi.title')} description={t('systemAi.intro')}>
      <div aria-live="polite" className="space-y-2">
        {notice ? (
          <p role="status" className="text-sm text-green-700 dark:text-green-400">
            {notice}
          </p>
        ) : null}
        {error && !toDelete && !toReplace ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, error)}</AlertTitle>
          </Alert>
        ) : null}
      </div>
      {data.loading && !overview ? <LoadingBlock /> : null}
      {data.error ? <ErrorBlock error={data.error} onRetry={data.reload} /> : null}
      {overview ? (
        <>
          <p className="text-sm">
            {overview.source === 'database'
              ? t('systemAi.source.database', { provider: overview.setting?.provider_name ?? '', model: overview.setting?.model ?? '' })
              : t('systemAi.source.host', { model: overview.host_model })}
          </p>
          {!overview.routing_enabled || !overview.encryption_configured ? (
            <Alert role="status">
              <AlertTitle>{t('systemAi.disabled')}</AlertTitle>
            </Alert>
          ) : null}

          <form onSubmit={onSaveModel} className="space-y-3" aria-label={t('systemAi.model.title')}>
            <h2 className="text-base font-medium">{t('systemAi.model.title')}</h2>
            <ModelFields
              listPath={`${BASE}/models`}
              idPrefix="sys"
              value={{ provider: settingProvider, model: settingModel }}
              providers={overview.providers}
              disabled={busy !== undefined}
              onChange={(patch) => {
                if (patch.provider !== undefined) setProvider(patch.provider)
                if (patch.model !== undefined) setModel(patch.model)
              }}
            />
            <div className="flex flex-wrap gap-2">
              <Button type="submit" disabled={busy !== undefined || !settingProvider || !settingModel.trim()}>
                {busy === 'model' ? t('systemAi.model.saving') : t('systemAi.model.save')}
              </Button>
              {overview.setting ? (
                <Button type="button" variant="outline" disabled={busy !== undefined} onClick={() => void run('clear', () => api.delete(`${BASE}/model`), t('systemAi.modelCleared'))}>
                  {t('systemAi.model.clear')}
                </Button>
              ) : null}
            </div>
            <p className="text-muted-foreground text-xs">{t('systemAi.model.note')}</p>
          </form>

          <section className="space-y-2">
            <h2 className="text-base font-medium">{t('systemAi.keys.title')}</h2>
            {overview.keys.length === 0 ? (
              <p className="text-muted-foreground text-sm">{t('systemAi.keys.empty')}</p>
            ) : (
              <ul aria-label={t('systemAi.keys.title')} className="divide-y rounded-md border">
                {overview.keys.map((k) => (
                  <li key={k.id} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
                    <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
                      <span className="font-medium">{k.provider_name}</span>
                      <code className="text-muted-foreground text-sm">{k.mask}</code>
                      <StatusBadge status={k.status} />
                      <span className="text-muted-foreground text-xs">
                        {t('systemAi.usage', { requests: nf.format(k.requests_30d), input: nf.format(k.input_tokens_30d), output: nf.format(k.output_tokens_30d) })}
                      </span>
                      {results[k.id] ? <span className="text-xs">{results[k.id]}</span> : null}
                    </div>
                    <div className="flex gap-1">
                      <Button variant="ghost" size="icon" aria-label={t('keys.test.label', { provider: k.provider_name, mask: k.mask })} onClick={() => void onTest(k)}>
                        <FlaskConical aria-hidden />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t('keys.replace.label', { provider: k.provider_name, mask: k.mask })}
                        onClick={() => {
                          setError(undefined)
                          setReplaceSecret('')
                          setToReplace(k)
                        }}
                      >
                        <RefreshCw aria-hidden />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t('keys.delete.label', { provider: k.provider_name, mask: k.mask })}
                        onClick={() => {
                          setError(undefined)
                          setToDelete(k)
                        }}
                      >
                        <Trash2 aria-hidden />
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <form onSubmit={onAdd} className="space-y-3" aria-label={t('keys.add.title')}>
            <h2 className="text-base font-medium">{t('keys.add.title')}</h2>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor="sys-add-provider">{t('keys.add.provider')}</Label>
                <select id="sys-add-provider" className={nativeSelectClass} value={provider} onChange={(e) => setProvider(e.target.value)}>
                  <option value="">{t('keys.add.providerPlaceholder')}</option>
                  {overview.providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="sys-add-key">{t('keys.add.key')}</Label>
                <Input id="sys-add-key" type="password" autoComplete="off" spellCheck={false} maxLength={512} value={secret} onChange={(e) => setSecret(e.target.value)} />
              </div>
            </div>
            <Button type="submit" disabled={busy !== undefined || !provider || !secret.trim()}>
              {busy === 'add' ? t('keys.add.submitting') : t('keys.add.submit')}
            </Button>
          </form>
        </>
      ) : null}

      <Dialog open={toReplace !== undefined} onOpenChange={(o) => !o && busy === undefined && setToReplace(undefined)}>
        <DialogContent>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              if (!toReplace) return
              void run('replace', () => api.put(`${BASE}/keys/${encodeURIComponent(toReplace.id)}`, { key: replaceSecret.trim() }), t('keys.replace.done', { provider: toReplace.provider_name }), () => setToReplace(undefined))
            }}
          >
            <DialogHeader>
              <DialogTitle>{t('keys.replace.title', { provider: toReplace?.provider_name ?? '' })}</DialogTitle>
              <DialogDescription>{t('keys.replace.providerFixed')}</DialogDescription>
            </DialogHeader>
            <div className="space-y-1.5 py-3">
              <Label htmlFor="sys-replace-key">{t('keys.add.key')}</Label>
              <Input id="sys-replace-key" type="password" autoComplete="off" spellCheck={false} maxLength={512} value={replaceSecret} onChange={(e) => setReplaceSecret(e.target.value)} />
            </div>
            {error && toReplace ? (
              <Alert variant="destructive" role="alert">
                <AlertTitle>{errorText(t, error)}</AlertTitle>
              </Alert>
            ) : null}
            <DialogFooter>
              <Button type="submit" disabled={busy !== undefined || !replaceSecret.trim()}>
                {busy === 'replace' ? t('keys.replace.submitting') : t('keys.replace.submit')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog open={toDelete !== undefined} onOpenChange={(o) => !o && busy === undefined && setToDelete(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('keys.delete.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('systemAi.delete.body')}</AlertDialogDescription>
          </AlertDialogHeader>
          {error && toDelete ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, error)}</AlertTitle>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy !== undefined}>{t('common.cancel')}</AlertDialogCancel>
            <Button
              variant="destructive"
              disabled={busy !== undefined}
              onClick={() => toDelete && void run('delete', () => api.delete(`${BASE}/keys/${encodeURIComponent(toDelete.id)}`), t('systemAi.deleted'), () => setToDelete(undefined))}
            >
              {busy === 'delete' ? t('keys.delete.deleting') : t('keys.delete.confirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  )
}
