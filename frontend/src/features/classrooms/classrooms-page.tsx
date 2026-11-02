import { Pencil } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { Link } from 'react-router'
import { useAuth } from '@/auth/auth-context'
import { useClassroomsBase } from '@/features/classrooms/use-base'
import { ArchiveViewToggle } from '@/components/archive-view-toggle'
import { ConfirmDialog } from '@/features/chat/confirm-dialog'
import { NameConfirmDialog } from '@/components/name-confirm-dialog'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { useT, errorText } from '@/i18n'
import { api } from '@/lib/api'
import type { Classroom } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type ClassroomValues = { name: string; year_label: string; description: string }

function classroomValues(classroom?: Classroom): ClassroomValues {
  return {
    name: classroom?.name ?? '',
    year_label: classroom?.year_label ?? '',
    description: classroom?.description ?? '',
  }
}

function ClassroomForm({
  idPrefix,
  initial,
  submitLabel,
  pendingLabel,
  onSubmit,
  onCancel,
  nameOnly = false,
  placeholder,
}: {
  idPrefix: string
  initial: ClassroomValues
  submitLabel: string
  pendingLabel: string
  onSubmit: (values: ClassroomValues) => Promise<void>
  onCancel?: () => void
  /** 建立教室只需名稱（設計稿）；年度與說明可於編輯時補上。 */
  nameOnly?: boolean
  placeholder?: string
}) {
  const t = useT()
  const [values, setValues] = useState(initial)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (pending) return
    if (!values.name.trim()) {
      setError(new Error(t('classrooms.required')))
      return
    }
    setError(undefined)
    setPending(true)
    try {
      await onSubmit({
        name: values.name.trim(),
        year_label: values.year_label.trim(),
        description: values.description.trim(),
      })
    } catch (caught) {
      setError(caught)
    } finally {
      setPending(false)
    }
  }

  return (
    <form onSubmit={(event) => void submit(event)} className="space-y-3" noValidate>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-name`}>{t('classrooms.name')}</Label>
        <Input
          id={`${idPrefix}-name`}
          required
          maxLength={255}
          autoComplete="off"
          placeholder={placeholder}
          value={values.name}
          aria-invalid={error ? true : undefined}
          onChange={(event) => setValues({ ...values, name: event.target.value })}
        />
      </div>
      {nameOnly ? null : (
      <>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-year`}>{t('classrooms.year')}</Label>
        <Input
          id={`${idPrefix}-year`}
          maxLength={100}
          value={values.year_label}
          onChange={(event) => setValues({ ...values, year_label: event.target.value })}
        />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-description`}>{t('classrooms.description')}</Label>
        <Textarea
          id={`${idPrefix}-description`}
          rows={3}
          maxLength={2000}
          value={values.description}
          onChange={(event) => setValues({ ...values, description: event.target.value })}
        />
      </div>
      </>
      )}
      {error ? (
        <Alert variant="destructive" role="alert">
          <AlertTitle>{error instanceof Error && error.name !== 'ApiError' ? error.message : errorText(t, error)}</AlertTitle>
        </Alert>
      ) : null}
      <div className="flex justify-end gap-2">
        {onCancel ? (
          <Button type="button" variant="outline" disabled={pending} onClick={onCancel}>
            {t('common.cancel')}
          </Button>
        ) : null}
        <Button type="submit" disabled={pending || !values.name.trim()}>
          {pending ? pendingLabel : submitLabel}
        </Button>
      </div>
    </form>
  )
}

export function ClassroomsPage() {
  const t = useT()
  const base = useClassroomsBase()
  const auth = useAuth()
  const canCreate = auth.status === 'authed' && auth.me.roles.teacher
  const { data, error, loading, reload } = useFetch<Classroom[]>('/api/classrooms')
  const [editing, setEditing] = useState<Classroom>()
  const [showArchived, setShowArchived] = useState(false)
  const [deleting, setDeleting] = useState<Classroom>()
  const [archiving, setArchiving] = useState<string>()
  const [archiveError, setArchiveError] = useState<unknown>()
  const [notice, setNotice] = useState('')
  const [creating, setCreating] = useState(false)
  const [confirmArchive, setConfirmArchive] = useState<Classroom>()

  async function create(values: ClassroomValues) {
    setNotice('')
    await api.post<Classroom>('/api/classrooms', {
      name: values.name,
      year_label: values.year_label || null,
      description: values.description || null,
    })
    setCreating(false)
    setNotice(t('classrooms.created'))
    reload()
  }

  async function update(values: ClassroomValues) {
    if (!editing) return
    await api.patch<Classroom>(`/api/classrooms/${editing.id}`, {
      name: values.name,
      year_label: values.year_label || null,
      description: values.description || null,
    })
    setEditing(undefined)
    setNotice(t('classrooms.saved'))
    reload()
  }

  async function toggleArchive(room: Classroom) {
    if (archiving) return
    setArchiving(room.id)
    setArchiveError(undefined)
    try {
      await api.patch<Classroom>(`/api/classrooms/${room.id}`, { is_archived: !room.is_archived })
      setConfirmArchive(undefined)
      reload()
    } catch (err) { setArchiveError(err) }
    finally { setArchiving(undefined) }
  }

  async function remove(room: Classroom) {
    await api.delete(`/api/classrooms/${room.id}`, { confirm_name: room.name })
    setDeleting(undefined)
    setNotice(t('classrooms.deleted'))
    reload()
  }

  const visible = data?.filter((room) => room.is_archived === showArchived)
  const title = canCreate ? t('classrooms.title.teacher') : t('classrooms.title.admin')

  return (
    <Page
      title={title}
      actions={
        canCreate ? (
          <Button type="button" onClick={() => { setNotice(''); setCreating(true) }}>
            {t('classrooms.new')}
          </Button>
        ) : undefined
      }
    >
      <p className="text-muted-foreground text-sm">{t('classrooms.intro')}</p>
      {archiveError ? <ErrorBlock error={archiveError} /> : null}
      {notice ? <p role="status" className="text-sm text-green-700 dark:text-green-400">{notice}</p> : null}
      <ArchiveViewToggle archived={showArchived} onChange={setShowArchived} />
      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {visible && visible.length === 0 ? (
        <EmptyBlock>
          {showArchived ? t('classrooms.emptyArchived') : canCreate ? t('classrooms.empty') : t('classrooms.empty.admin')}
        </EmptyBlock>
      ) : null}
      {visible && visible.length > 0 ? (
        <div className="rounded-md border">
          <Table aria-label={title}>
            <TableHeader>
              <TableRow>
                <TableHead>{t('classrooms.col.name')}</TableHead>
                <TableHead>{t('classrooms.col.detail')}</TableHead>
                <TableHead>{t('classrooms.col.status')}</TableHead>
                <TableHead>{t('classrooms.col.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {visible.map((classroom) => (
                <TableRow key={classroom.id}>
                  <TableCell className="font-medium">
                    <Link to={`${base}/${classroom.id}`} className="underline underline-offset-2">
                      {classroom.name}
                    </Link>
                  </TableCell>
                  <TableCell className="text-muted-foreground max-w-80 whitespace-normal">
                    {[classroom.year_label, classroom.description].filter(Boolean).join(' · ') || '—'}
                  </TableCell>
                  <TableCell>{classroom.is_archived ? t('classrooms.archived') : t('classrooms.active')}</TableCell>
                  <TableCell>
                    <div className="flex flex-wrap gap-1">
                      <Button asChild variant="ghost">
                        <Link to={`${base}/${classroom.id}`}>{t(classroom.is_archived ? 'classrooms.view' : 'classrooms.open')}</Link>
                      </Button>
                      <Button
                        disabled={classroom.is_archived || !!archiving}
                        type="button"
                        variant="ghost"
                        aria-label={t('classrooms.edit', { name: classroom.name })}
                        onClick={() => {
                          setNotice('')
                          setEditing(classroom)
                        }}
                      >
                        <Pencil aria-hidden />
                      </Button>
                      <Button
                        variant="ghost"
                        disabled={!!archiving}
                        onClick={() => (classroom.is_archived ? void toggleArchive(classroom) : setConfirmArchive(classroom))}
                      >
                        {t(classroom.is_archived ? 'classrooms.restore' : 'classrooms.archive')}
                      </Button>
                      <Button variant="ghost" disabled={!!archiving} onClick={() => setDeleting(classroom)}>
                        {t('classrooms.delete')}
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      ) : null}

      {deleting ? (
        <NameConfirmDialog
          title={t('classrooms.deleteTitle')}
          description={t('classrooms.deleteBody')}
          name={deleting.name}
          onConfirm={() => remove(deleting)}
          onCancel={() => setDeleting(undefined)}
        />
      ) : null}

      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('classrooms.createTitle')}</DialogTitle>
            <DialogDescription>{t('classrooms.createBody')}</DialogDescription>
          </DialogHeader>
          <ClassroomForm
            idPrefix="create-classroom"
            initial={classroomValues()}
            nameOnly
            placeholder={t('classrooms.namePlaceholder')}
            submitLabel={t('classrooms.create')}
            pendingLabel={t('classrooms.creating')}
            onSubmit={create}
            onCancel={() => setCreating(false)}
          />
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={confirmArchive !== undefined}
        onOpenChange={(open) => !open && setConfirmArchive(undefined)}
        title={t('classrooms.archiveTitle', { name: confirmArchive?.name ?? '' })}
        description={t('classrooms.archiveBody')}
        confirmLabel={t('classrooms.archiveConfirm')}
        pending={!!archiving}
        onConfirm={() => confirmArchive && void toggleArchive(confirmArchive)}
      />

      <Dialog open={editing !== undefined} onOpenChange={(open) => !open && setEditing(undefined)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('classrooms.editTitle')}</DialogTitle>
            <DialogDescription>{editing?.name}</DialogDescription>
          </DialogHeader>
          {editing ? (
            <ClassroomForm
              idPrefix={`edit-classroom-${editing.id}`}
              key={editing.id}
              initial={classroomValues(editing)}
              submitLabel={t('classrooms.save')}
              pendingLabel={t('classrooms.saving')}
              onSubmit={update}
              onCancel={() => setEditing(undefined)}
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </Page>
  )
}
