import { Pencil } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useAuth } from '@/auth/auth-context'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
}: {
  idPrefix: string
  initial: ClassroomValues
  submitLabel: string
  pendingLabel: string
  onSubmit: (values: ClassroomValues) => Promise<void>
  onCancel?: () => void
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
          value={values.name}
          aria-invalid={error ? true : undefined}
          onChange={(event) => setValues({ ...values, name: event.target.value })}
        />
      </div>
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
  const auth = useAuth()
  const canCreate = auth.status === 'authed' && auth.me.roles.teacher
  const { data, error, loading, reload } = useFetch<Classroom[]>('/api/classrooms')
  const [editing, setEditing] = useState<Classroom>()
  const [notice, setNotice] = useState('')
  const [createFormVersion, setCreateFormVersion] = useState(0)

  async function create(values: ClassroomValues) {
    setNotice('')
    await api.post<Classroom>('/api/classrooms', {
      name: values.name,
      year_label: values.year_label || null,
      description: values.description || null,
    })
    setNotice(t('classrooms.created'))
    setCreateFormVersion((version) => version + 1)
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

  const title = canCreate ? t('classrooms.title.teacher') : t('classrooms.title.admin')

  return (
    <Page title={title}>
      <p className="text-muted-foreground text-sm">{t('classrooms.intro')}</p>
      {notice ? <p role="status" className="text-sm text-green-700 dark:text-green-400">{notice}</p> : null}
      {canCreate ? (
        <section aria-label={t('classrooms.create')} className="rounded-md border p-4">
          <ClassroomForm
            idPrefix="create-classroom"
            key={createFormVersion}
            initial={classroomValues()}
            submitLabel={t('classrooms.create')}
            pendingLabel={t('classrooms.creating')}
            onSubmit={create}
          />
        </section>
      ) : null}

      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data && data.length === 0 ? (
        <EmptyBlock>{canCreate ? t('classrooms.empty') : t('classrooms.empty.admin')}</EmptyBlock>
      ) : null}
      {data && data.length > 0 ? (
        <ul aria-label={title} className="divide-y rounded-md border">
          {data.map((classroom) => (
            <li key={classroom.id} aria-label={classroom.name} className="flex items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 space-y-1">
                <h2 className="font-medium">{classroom.name}</h2>
                {classroom.year_label ? <p className="text-muted-foreground text-sm">{classroom.year_label}</p> : null}
                {classroom.description ? <p className="text-sm">{classroom.description}</p> : null}
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={t('classrooms.edit', { name: classroom.name })}
                onClick={() => {
                  setNotice('')
                  setEditing(classroom)
                }}
              >
                <Pencil aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      ) : null}

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
