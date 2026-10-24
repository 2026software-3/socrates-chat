import { useRef, useState, type ChangeEvent, type FormEvent, type ReactNode } from 'react'
import { Link } from 'react-router'
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
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { useAuth } from '@/auth/auth-context'
import { ResetPasswordDialog } from '@/features/admin/reset-password-dialog'
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard'
import { LoadMore } from '@/components/load-more'
import { useMediaQuery } from '@/hooks/use-media-query'
import { errorText, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { Classroom, RosterImportResult, StudentPage, StudentRow } from '@/lib/types'
import { useFetch, usePagedFetch } from '@/lib/use-fetch'

/** 匯入表單：貼上文字或載入檔案內容到文字框（檔案本身不上傳）。 */
function ImportPanel({ classroomId, onImported }: { classroomId: string; onImported: () => void }) {
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
      const r = await api.post<RosterImportResult>(`/api/classrooms/${classroomId}/roster/import`, {
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
                    onClick={() => copyToClipboard((result.credentials ?? []).map((c) => `${c.email},${c.temporary_password}`).join('\n'))}
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

type AccountAction = { kind: 'remove'; email: string } | { kind: 'disable'; email: string; id: string } | { kind: 'delete'; email: string }

function statusOf(s: StudentRow): 'disabled' | 'pending' | 'active' {
  return s.disabled ? 'disabled' : s.has_account ? 'active' : 'pending'
}

function StatusBadge({ status }: { status: 'disabled' | 'pending' | 'active' }) {
  const t = useT()
  return (
    <Badge variant={status === 'active' ? 'secondary' : status === 'disabled' ? 'destructive' : 'outline'}>
      {t(`teacher.roster.status.${status}`)}
    </Badge>
  )
}

const selectClass =
  'border-input bg-background h-9 w-full rounded-md border px-3 text-sm shadow-xs focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none'

/** 教師與管理者：學生帳號管理——先選教室，再看該教室的名單與帳號狀態、匯入、移出名單、刪除帳號；管理者另可重設密碼、停用與復原。 */
export function RosterPage() {
  const t = useT()
  const auth = useAuth()
  const classrooms = useFetch<Classroom[]>('/api/classrooms')
  const [selected, setSelected] = useState<string>()
  const list = classrooms.data
  const current = list?.find((c) => c.id === selected) ?? list?.[0]
  const isTeacher = auth.status === 'authed' && auth.me.roles.teacher

  return (
    <Page title={t('teacher.roster.title')}>
      {classrooms.loading && !list ? <LoadingBlock /> : null}
      {classrooms.error ? <ErrorBlock error={classrooms.error} onRetry={classrooms.reload} /> : null}
      {list && list.length === 0 ? (
        <EmptyBlock>
          <p>{isTeacher ? t('teacher.roster.noClassrooms') : t('teacher.roster.noClassroomsAdmin')}</p>
          {isTeacher ? (
            <Link to="/teacher/classrooms" className="mt-2 inline-block underline">
              {t('teacher.roster.goToClassrooms')}
            </Link>
          ) : null}
        </EmptyBlock>
      ) : null}
      {list && list.length > 0 && current ? (
        <>
          <div className="max-w-sm space-y-1.5">
            <Label htmlFor="roster-classroom">{t('teacher.roster.classroomLabel')}</Label>
            <select id="roster-classroom" className={selectClass} value={current.id} onChange={(e) => setSelected(e.target.value)}>
              {list.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.year_label ? `${c.name}（${c.year_label}）` : c.name}
                </option>
              ))}
            </select>
          </div>
          {/* 以教室為 key：換教室時重設搜尋、對話框與匯入結果 */}
          <ClassroomRoster key={current.id} classroom={current} />
        </>
      ) : null}
    </Page>
  )
}

/** 單一教室的學生名單與操作。 */
function ClassroomRoster({ classroom }: { classroom: Classroom }) {
  const t = useT()
  // 搜尋與人數都由後端計算：送出搜尋後從第一頁重新載入，其餘頁按「載入更多」才取
  const [input, setInput] = useState('')
  const [query, setQuery] = useState('')
  const roster = usePagedFetch<StudentRow, StudentPage>(
    `/api/classrooms/${classroom.id}/students${query ? `?q=${encodeURIComponent(query)}` : ''}`,
  )
  const auth = useAuth()
  const isAdmin = auth.status === 'authed' && auth.me.roles.admin
  const [toReset, setToReset] = useState<string>()
  const [action, setAction] = useState<AccountAction | null>(null)
  const [reason, setReason] = useState('')
  const [confirmText, setConfirmText] = useState('')
  const [pending, setPending] = useState(false)
  const [actionError, setActionError] = useState<string>()
  const busy = useRef(false)
  // 手機用卡片、桌面用表格：只渲染其中一種，避免同一份資料在畫面上出現兩次
  const wide = useMediaQuery('(min-width: 768px)')

  const students = roster.items
  const total = roster.page?.total ?? 0

  function open(a: AccountAction) {
    setReason('')
    setConfirmText('')
    setActionError(undefined)
    setAction(a)
  }

  /** 送出一個帳號操作；成功或 404（已不存在）都重新整理。 */
  async function run(call: () => Promise<unknown>, successKey: Parameters<typeof t>[0]) {
    if (busy.current) return
    busy.current = true
    setPending(true)
    setActionError(undefined)
    try {
      await call()
      toast.success(t(successKey))
      setAction(null)
    } catch (err) {
      setActionError(errorText(t, err))
      if (isApiError(err) && err.status === 404) setAction(null)
    } finally {
      roster.reload()
      busy.current = false
      setPending(false)
    }
  }

  async function enable(id: string) {
    await run(() => api.put(`/api/admin/users/${encodeURIComponent(id)}/status`, { disabled: false }), 'teacher.roster.enabledToast')
  }

  /** 每位學生可做的操作：管理者另有重設密碼、停用與復原。 */
  const rowActions = (s: StudentRow) => (
    <>
      {isAdmin ? (
        <Button
          variant="outline"
          size="sm"
          aria-label={t('admin.reset.resetPassword', { email: s.email })}
          onClick={() => setToReset(s.email)}
        >
          {t('admin.reset.resetConfirm')}
        </Button>
      ) : null}
      {isAdmin && s.id ? (
        s.disabled ? (
          <Button
            variant="outline"
            size="sm"
            disabled={pending}
            aria-label={t('teacher.roster.enableLabel', { email: s.email })}
            onClick={() => void enable(s.id!)}
          >
            {t('teacher.roster.enable')}
          </Button>
        ) : (
          <Button
            variant="outline"
            size="sm"
            aria-label={t('teacher.roster.disableLabel', { email: s.email })}
            onClick={() => open({ kind: 'disable', email: s.email, id: s.id! })}
          >
            {t('teacher.roster.disable')}
          </Button>
        )
      ) : null}
      <Button
        variant="outline"
        size="sm"
        aria-label={t('teacher.roster.removeLabel', { email: s.email })}
        onClick={() => open({ kind: 'remove', email: s.email })}
      >
        {t('teacher.roster.remove')}
      </Button>
      <Button
        variant="destructive"
        size="sm"
        aria-label={t('teacher.roster.deleteLabel', { email: s.email })}
        onClick={() => open({ kind: 'delete', email: s.email })}
      >
        {t('teacher.roster.deleteAccount')}
      </Button>
    </>
  )

  const target = action?.email ?? ''
  const closeDialog = (o: boolean) => {
    if (!o && !pending) setAction(null)
  }

  return (
    <div className="space-y-4">
      <h2 className="text-xl font-medium">{t('teacher.roster.heading', { name: classroom.name })}</h2>
      {roster.loading && !students ? <LoadingBlock /> : null}
      {roster.error ? <ErrorBlock error={roster.error} onRetry={roster.reload} /> : null}
      {actionError && action === null ? (
        <Alert variant="destructive" role="alert">
          <AlertDescription>{actionError}</AlertDescription>
        </Alert>
      ) : null}
      {students && total === 0 && !query ? <EmptyBlock>{t('teacher.roster.empty')}</EmptyBlock> : null}
      {students && (total > 0 || query) ? (
        <div className="space-y-3">
          <p className="text-sm font-medium" aria-live="polite">
            {t('teacher.roster.count', { count: total })}
          </p>
          <form
            className="flex max-w-sm items-end gap-2"
            role="search"
            onSubmit={(e) => {
              e.preventDefault()
              setQuery(input.trim())
            }}
          >
            <div className="flex-1 space-y-1.5">
              <Label htmlFor="roster-search">{t('teacher.roster.searchLabel')}</Label>
              <Input id="roster-search" type="search" maxLength={100} value={input} onChange={(e) => setInput(e.target.value)} />
            </div>
            <Button type="submit" variant="outline">
              {t('teacher.roster.searchButton')}
            </Button>
          </form>
          {students.length === 0 ? (
            <EmptyBlock>{t('teacher.roster.noMatch')}</EmptyBlock>
          ) : (
            <>
              {!wide ? (
                /* 手機：每位學生一張卡片，操作按鈕不會被擠到畫面外 */
                <ul className="space-y-3" aria-label={t('teacher.roster.title')}>
                  {students.map((s) => {
                    const status = statusOf(s)
                    return (
                      <li key={s.email} className="space-y-3 rounded-lg border p-3">
                        <div className="space-y-1">
                          <p className="font-medium break-all">{s.email}</p>
                          {s.display_name ? <p className="text-muted-foreground text-sm">{s.display_name}</p> : null}
                          <div className="flex flex-wrap items-center gap-2 text-sm">
                            <StatusBadge status={status} />
                            <span className="text-muted-foreground">
                              {t('teacher.roster.completedColumn')}：{s.completed_conversations}
                            </span>
                          </div>
                        </div>
                        <div className="flex flex-wrap gap-2">{rowActions(s)}</div>
                      </li>
                    )
                  })}
                </ul>
              ) : (
                <div className="overflow-x-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t('teacher.roster.emailColumn')}</TableHead>
                        <TableHead>{t('teacher.roster.statusColumn')}</TableHead>
                        <TableHead className="text-right">{t('teacher.roster.completedColumn')}</TableHead>
                        <TableHead className="text-right">{t('teacher.roster.actionsColumn')}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {students.map((s) => (
                        <TableRow key={s.email}>
                          <TableCell className="break-all">
                            {s.email}
                            {s.display_name ? <span className="text-muted-foreground block text-xs">{s.display_name}</span> : null}
                          </TableCell>
                          <TableCell>
                            <StatusBadge status={statusOf(s)} />
                          </TableCell>
                          <TableCell className="text-right tabular-nums">{s.completed_conversations}</TableCell>
                          <TableCell>
                            <div className="flex flex-wrap justify-end gap-2">{rowActions(s)}</div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}
            </>
          )}
          <LoadMore hasMore={roster.hasMore} loading={roster.loadingMore} error={roster.moreError} onLoadMore={roster.loadMore} />
        </div>
      ) : null}

      <ImportPanel
        classroomId={classroom.id}
        onImported={() => {
          setActionError(undefined)
          roster.reload()
        }}
      />

      <ResetPasswordDialog email={toReset} onClose={() => setToReset(undefined)} />

      <AccountDialog
        open={action?.kind === 'remove'}
        onOpenChange={closeDialog}
        title={t('teacher.roster.removeTitle')}
        description={t('teacher.roster.removeBody', { email: target, classroom: classroom.name })}
        confirmLabel={t('teacher.roster.remove')}
        pending={pending}
        error={action?.kind === 'remove' ? actionError : undefined}
        onConfirm={() => void run(() => api.delete(`/api/classrooms/${classroom.id}/roster/${encodeURIComponent(target)}`), 'teacher.roster.removed')}
      />

      <AccountDialog
        open={action?.kind === 'disable'}
        onOpenChange={closeDialog}
        title={t('teacher.roster.disableTitle')}
        description={t('teacher.roster.disableBody', { email: target })}
        confirmLabel={t('teacher.roster.disable')}
        pending={pending}
        error={action?.kind === 'disable' ? actionError : undefined}
        onConfirm={() =>
          void run(
            () => api.put(`/api/admin/users/${encodeURIComponent(action?.kind === 'disable' ? action.id : '')}/status`, { disabled: true, ...(reason.trim() ? { reason: reason.trim() } : {}) }),
            'teacher.roster.disabledToast',
          )
        }
      >
        <div className="space-y-1.5">
          <Label htmlFor="disable-reason">{t('teacher.roster.reasonLabel')}</Label>
          <Input id="disable-reason" value={reason} maxLength={500} onChange={(e) => setReason(e.target.value)} />
        </div>
      </AccountDialog>

      <AccountDialog
        open={action?.kind === 'delete'}
        onOpenChange={closeDialog}
        title={t('teacher.roster.deleteTitle')}
        description={t('teacher.roster.deleteBody', { email: target })}
        confirmLabel={t('teacher.roster.deleteConfirm')}
        pending={pending}
        // 再次確認（S-02.5）：必須輸入對方的電子郵件才能刪除
        confirmDisabled={confirmText.trim().toLowerCase() !== target.toLowerCase()}
        error={action?.kind === 'delete' ? actionError : undefined}
        onConfirm={() =>
          void run(
            () => api.delete(`/api/students/${encodeURIComponent(target)}`, { confirm_email: confirmText.trim() }),
            'teacher.roster.deletedToast',
          )
        }
      >
        <div className="space-y-1.5">
          <Label htmlFor="delete-confirm">{t('teacher.roster.deleteConfirmLabel', { email: target })}</Label>
          <Input id="delete-confirm" value={confirmText} autoComplete="off" onChange={(e) => setConfirmText(e.target.value)} />
        </div>
      </AccountDialog>
    </div>
  )
}

/** 帳號操作共用的確認對話框：標題、說明、（選填）輸入欄位、錯誤與確認按鈕。 */
function AccountDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  pending,
  confirmDisabled = false,
  error,
  onConfirm,
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  confirmLabel: string
  pending: boolean
  confirmDisabled?: boolean
  error?: string
  onConfirm: () => void
  children?: ReactNode
}) {
  const t = useT()
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        {children}
        {error ? (
          <Alert variant="destructive" role="alert">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            disabled={pending || confirmDisabled}
            onClick={(e) => {
              e.preventDefault()
              onConfirm()
            }}
          >
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
