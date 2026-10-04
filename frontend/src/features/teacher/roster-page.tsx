import { useMemo, useRef, useState, type ChangeEvent, type FormEvent } from 'react'
import { toast } from 'sonner'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { useAuth } from '@/auth/auth-context'
import { ResetPasswordDialog } from '@/features/admin/reset-password-dialog'
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { EmailList, RosterImportResult } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 匯入表單：貼上文字或載入檔案內容到文字框（檔案本身不上傳）。 */
function ImportPanel({ onImported }: { onImported: () => void }) {
  const t = useT()
  const [text, setText] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string>()
  const [result, setResult] = useState<RosterImportResult>()
  const busy = useRef(false)
  const { isCopied, copyToClipboard } = useCopyToClipboard()

  async function onFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (!file) return
    try {
      setText(await file.text())
      setError(undefined)
    } catch {
      setError(t('teacher.roster.fileFailed'))
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (busy.current) return
    if (!text.trim()) {
      setError(t('teacher.roster.textRequired'))
      return
    }
    busy.current = true
    setPending(true)
    setError(undefined)
    setResult(undefined)
    try {
      const r = await api.post<RosterImportResult>('/api/roster/import', {
        text,
      })
      setResult(r)
      setText('')
      onImported()
    } catch (err) {
      setError(errorText(t, err))
    } finally {
      busy.current = false
      setPending(false)
    }
  }

  return (
    <Card>
      <CardContent className="space-y-3">
        <h2 className="text-lg font-medium">{t('teacher.roster.importHeading')}</h2>
        <p className="text-muted-foreground text-sm">{t('teacher.roster.importHelp')}</p>
        <form onSubmit={submit} noValidate className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="roster-text">{t('teacher.roster.textLabel')}</Label>
            <Textarea
              id="roster-text"
              rows={6}
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="font-mono"
              spellCheck={false}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="roster-file">{t('teacher.roster.fileLabel')}</Label>
            <Input id="roster-file" type="file" accept=".csv,.txt,text/csv,text/plain" onChange={(e) => void onFile(e)} />
          </div>
          {error ? (
            <Alert variant="destructive" role="alert">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          <Button type="submit" disabled={pending}>
            {pending ? t('teacher.working') : t('teacher.roster.importButton')}
          </Button>
        </form>
        {/* 常駐的 live region，內容出現時才會被朗讀 */}
        <div aria-live="polite">
          {result ? (
            <section aria-labelledby="roster-result-title" className="space-y-2 rounded-md border p-3">
              <h3 id="roster-result-title" className="font-medium">
                {t('teacher.roster.resultHeading')}
              </h3>
              <ul className="text-sm">
                <li>{t('teacher.roster.resultAdded', { count: result.added })}</li>
                <li>
                  {t('teacher.roster.resultExisting', {
                    count: result.existing,
                  })}
                </li>
                {result.invalid.length > 0 ? (
                  <li>
                    {t('teacher.roster.resultInvalid', {
                      count: result.invalid.length,
                    })}
                  </li>
                ) : null}
              </ul>
              {result.invalid.length > 0 ? (
                <ul className="text-destructive space-y-0.5 text-sm break-all">
                  {result.invalid.map((i) => (
                    <li key={`${i.line}-${i.value}`}>
                      {t('teacher.roster.invalidLine', {
                        line: i.line,
                        value: i.value,
                      })}
                    </li>
                  ))}
                </ul>
              ) : null}
              {result.credentials && result.credentials.length > 0 ? (
                <div className="space-y-2 border-t pt-3">
                  <h4 className="font-medium">{t('teacher.roster.credsHeading')}</h4>
                  <Alert>
                    <AlertDescription>{t('teacher.roster.credsHelp')}</AlertDescription>
                  </Alert>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t('teacher.roster.credsEmail')}</TableHead>
                        <TableHead>{t('teacher.roster.credsPassword')}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {result.credentials.map((c) => (
                        <TableRow key={c.email}>
                          <TableCell className="break-all">{c.email}</TableCell>
                          <TableCell className="font-mono">{c.temporary_password}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      copyToClipboard(
                        (result.credentials ?? []).map((c) => `${c.email},${c.temporary_password}`).join('\n'),
                      )
                    }
                  >
                    {isCopied ? t('teacher.roster.credsCopied') : t('teacher.roster.credsCopyAll')}
                  </Button>
                </div>
              ) : null}
            </section>
          ) : null}
        </div>
      </CardContent>
    </Card>
  )
}

/** 教師：檢視、匯入、移除修課名單。 */
export function RosterPage() {
  const t = useT()
  const roster = useFetch<EmailList>('/api/roster')
  const [query, setQuery] = useState('')
  const auth = useAuth()
  const isAdmin = auth.status === 'authed' && auth.me.roles.admin
  const [toReset, setToReset] = useState<string>()
  const [removing, setRemoving] = useState<string | null>(null)
  const [removePending, setRemovePending] = useState(false)
  const [removeError, setRemoveError] = useState<string>()
  const busy = useRef(false)

  const emails = roster.data?.emails
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return (emails ?? []).filter((e) => e.toLowerCase().includes(q))
  }, [emails, query])

  async function confirmRemove() {
    if (removing === null || busy.current) return
    busy.current = true
    setRemovePending(true)
    setRemoveError(undefined)
    try {
      await api.delete(`/api/roster/${encodeURIComponent(removing)}`)
      toast.success(t('teacher.roster.removed'))
    } catch (err) {
      setRemoveError(errorText(t, err))
    } finally {
      // 成功或 404（已不在名單）都重新整理
      roster.reload()
      setRemoving(null)
      busy.current = false
      setRemovePending(false)
    }
  }

  return (
    <Page title={t('teacher.roster.title')}>
      {roster.loading && !roster.data ? <LoadingBlock /> : null}
      {roster.error ? <ErrorBlock error={roster.error} onRetry={roster.reload} /> : null}
      {removeError ? (
        <Alert variant="destructive" role="alert">
          <AlertDescription>{removeError}</AlertDescription>
        </Alert>
      ) : null}
      {emails && emails.length === 0 ? <EmptyBlock>{t('teacher.roster.empty')}</EmptyBlock> : null}
      {emails && emails.length > 0 ? (
        <div className="space-y-3">
          <p className="text-sm font-medium" aria-live="polite">
            {t('teacher.roster.count', { count: emails.length })}
          </p>
          <div className="max-w-sm space-y-1.5">
            <Label htmlFor="roster-search">{t('teacher.roster.searchLabel')}</Label>
            <Input id="roster-search" type="search" value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          {filtered.length === 0 ? (
            <EmptyBlock>{t('teacher.roster.noMatch')}</EmptyBlock>
          ) : (
            <div className="overflow-x-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('teacher.roster.emailColumn')}</TableHead>
                    <TableHead className="text-right">{t('teacher.roster.actionsColumn')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filtered.map((email) => (
                    <TableRow key={email}>
                      <TableCell className="break-all">{email}</TableCell>
                      <TableCell className="space-x-2 text-right whitespace-nowrap">
                        {isAdmin ? (
                          <Button
                            variant="outline"
                            size="sm"
                            aria-label={t('admin.reset.resetPassword', { email })}
                            onClick={() => setToReset(email)}
                          >
                            {t('admin.reset.resetConfirm')}
                          </Button>
                        ) : null}
                        <Button
                          variant="outline"
                          size="sm"
                          aria-label={t('teacher.roster.removeLabel', {
                            email,
                          })}
                          onClick={() => setRemoving(email)}
                        >
                          {t('teacher.roster.remove')}
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </div>
      ) : null}

      <ImportPanel
        onImported={() => {
          setRemoveError(undefined)
          roster.reload()
        }}
      />

      <ResetPasswordDialog email={toReset} onClose={() => setToReset(undefined)} />

      <AlertDialog open={removing !== null} onOpenChange={(o) => !o && !removePending && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('teacher.roster.removeTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('teacher.roster.removeBody', { email: removing ?? '' })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={removePending}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={removePending}
              onClick={(e) => {
                e.preventDefault()
                void confirmRemove()
              }}
            >
              {t('teacher.roster.remove')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  )
}
