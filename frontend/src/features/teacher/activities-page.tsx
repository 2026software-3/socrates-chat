import { useRef, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { formatDate } from '@/features/teacher/format'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Activity, ActivityStatus, Available, Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

const STATUS_VARIANT: Record<ActivityStatus, 'secondary' | 'default' | 'outline'> = {
  draft: 'secondary',
  published: 'default',
  closed: 'outline',
}

type Values = { title: string; description: string; topicId: string }

const selectClass =
  'border-input bg-background h-9 w-full rounded-md border px-3 text-sm focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 outline-none disabled:opacity-50'

/** 建立與編輯共用的表單。 */
function ActivityForm({
  idPrefix,
  initial,
  topics,
  topicsFailed,
  submitLabel,
  resetOnSuccess = false,
  onSubmit,
}: {
  idPrefix: string
  initial: Values
  topics: Topic[]
  topicsFailed: boolean
  submitLabel: string
  /** 成功後是否清空表單（建立表單用）。 */
  resetOnSuccess?: boolean
  /** 失敗時 throw。 */
  onSubmit: (v: Values) => Promise<void>
}) {
  const t = useT()
  const [values, setValues] = useState<Values>(initial)
  const [pending, setPending] = useState(false)
  const [titleError, setTitleError] = useState(false)
  const [serverError, setServerError] = useState<string>()
  const busy = useRef(false)
  const hasInitialTopic = initial.topicId !== ''

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (busy.current) return
    if (!values.title.trim()) {
      setTitleError(true)
      return
    }
    setTitleError(false)
    setServerError(undefined)
    busy.current = true
    setPending(true)
    try {
      await onSubmit({ ...values, title: values.title.trim() })
      if (resetOnSuccess) setValues(initial)
    } catch (err) {
      setServerError(errorText(t, err))
    } finally {
      busy.current = false
      setPending(false)
    }
  }

  return (
    <form onSubmit={submit} noValidate className="space-y-3">
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-title`}>{t('teacher.activities.titleLabel')}</Label>
        <Input
          id={`${idPrefix}-title`}
          value={values.title}
          aria-invalid={titleError}
          aria-describedby={titleError ? `${idPrefix}-title-error` : undefined}
          onChange={(e) => setValues({ ...values, title: e.target.value })}
        />
        {titleError ? (
          <p id={`${idPrefix}-title-error`} role="alert" className="text-destructive text-sm">
            {t('teacher.activities.titleRequired')}
          </p>
        ) : null}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-desc`}>{t('teacher.activities.descriptionLabel')}</Label>
        <Textarea
          id={`${idPrefix}-desc`}
          value={values.description}
          onChange={(e) => setValues({ ...values, description: e.target.value })}
        />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-topic`}>{t('teacher.activities.topicLabel')}</Label>
        <select
          id={`${idPrefix}-topic`}
          className={selectClass}
          value={values.topicId}
          onChange={(e) => setValues({ ...values, topicId: e.target.value })}
        >
          <option value="">{t('teacher.activities.topicNone')}</option>
          {hasInitialTopic && !topics.some((tp) => tp.id === initial.topicId) ? (
            <option value={initial.topicId}>{t('teacher.activities.topicUnavailable')}</option>
          ) : null}
          {topics.map((tp) => (
            <option key={tp.id} value={tp.id}>
              {tp.title}
            </option>
          ))}
        </select>
        {topicsFailed ? <p className="text-muted-foreground text-sm">{t('teacher.activities.topicsFailed')}</p> : null}
      </div>
      {serverError ? (
        <Alert variant="destructive" role="alert">
          <AlertDescription>{serverError}</AlertDescription>
        </Alert>
      ) : null}
      <Button type="submit" disabled={pending}>
        {pending ? t('teacher.working') : submitLabel}
      </Button>
    </form>
  )
}

function ActivityRow({
  activity,
  topic,
  busy,
  error,
  onEdit,
  onToggle,
}: {
  activity: Activity
  topic: Topic | undefined
  busy: boolean
  error: string | undefined
  onEdit: () => void
  onToggle: () => void
}) {
  const t = useT()
  const { lang } = useI18n()
  const toggleKey = {
    draft: 'teacher.activities.publish',
    published: 'teacher.activities.close',
    closed: 'teacher.activities.reopen',
  } as const
  return (
    <li>
      <Card>
        <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="font-medium break-words">{activity.title}</h3>
              <Badge variant={STATUS_VARIANT[activity.status]}>{t(`teacher.activities.status.${activity.status}`)}</Badge>
            </div>
            {activity.description ? (
              <p className="text-muted-foreground text-sm break-words whitespace-pre-line">{activity.description}</p>
            ) : null}
            <p className="text-muted-foreground flex flex-wrap gap-x-3 text-xs">
              {topic ? <span>{t('teacher.activities.topicOf', { title: topic.title })}</span> : null}
              {!topic && activity.topic_id ? (
                <span>
                  {t('teacher.activities.topicOf', {
                    title: t('teacher.activities.topicUnavailable'),
                  })}
                </span>
              ) : null}
              <span>
                {t('teacher.activities.createdAt', {
                  date: formatDate(activity.created_at, lang),
                })}
              </span>
            </p>
          </div>
          <div className="flex shrink-0 gap-2">
            <Button variant="outline" size="sm" onClick={onEdit} disabled={busy}>
              {t('teacher.activities.edit')}
            </Button>
            <Button size="sm" onClick={onToggle} disabled={busy}>
              {t(toggleKey[activity.status])}
            </Button>
          </div>
          {error ? (
            <Alert variant="destructive" role="alert" className="sm:basis-full">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
        </CardContent>
      </Card>
    </li>
  )
}

/** 教師／管理者：建立、編輯、發布、停止開放討論活動。 */
export function ActivitiesPage() {
  const t = useT()
  const list = useFetch<Activity[]>('/api/activities')
  // 教師不能呼叫 /api/admin/topics，題目清單來自 /api/available
  const available = useFetch<Available>('/api/available')
  const topics = available.data?.topics ?? []
  const [editing, setEditing] = useState<Activity | null>(null)
  const [busyIds, setBusyIds] = useState<ReadonlySet<string>>(new Set())
  const busyRef = useRef(new Set<string>())
  // 以活動 id 記錄列內錯誤，顯示在失敗的那一列
  const [rowError, setRowError] = useState<{ id: string; message: string }>()

  async function create(v: Values) {
    await api.post<Activity>('/api/activities', {
      title: v.title,
      ...(v.description.trim() ? { description: v.description.trim() } : {}),
      ...(v.topicId ? { topic_id: v.topicId } : {}),
    })
    toast.success(t('teacher.activities.created'))
    setRowError(undefined)
    list.reload()
  }

  async function update(a: Activity, v: Values) {
    // 只在題目有變動時才送 topic_id；選「不指定題目」送 null 以清除
    const topicChanged = v.topicId !== (a.topic_id ?? '')
    await api.patch<Activity>(`/api/activities/${a.id}`, {
      title: v.title,
      description: v.description.trim(),
      ...(topicChanged ? { topic_id: v.topicId === '' ? null : v.topicId } : {}),
    })
    toast.success(t('teacher.activities.updated'))
    setEditing(null)
    setRowError(undefined)
    list.reload()
  }

  async function toggle(a: Activity) {
    if (busyRef.current.has(a.id)) return
    busyRef.current.add(a.id)
    setBusyIds(new Set(busyRef.current))
    setRowError(undefined)
    const action = a.status === 'published' ? 'close' : 'publish'
    try {
      await api.post<Activity>(`/api/activities/${a.id}/${action}`)
      toast.success(t(action === 'close' ? 'teacher.activities.closedToast' : 'teacher.activities.published'))
      list.reload()
    } catch (err) {
      setRowError({ id: a.id, message: errorText(t, err) })
    } finally {
      busyRef.current.delete(a.id)
      setBusyIds(new Set(busyRef.current))
    }
  }

  return (
    <Page title={t('teacher.activities.title')}>
      <Card>
        <CardContent className="space-y-3">
          <h2 className="text-lg font-medium">{t('teacher.activities.createHeading')}</h2>
          <ActivityForm
            idPrefix="create"
            initial={{ title: '', description: '', topicId: '' }}
            topics={topics}
            topicsFailed={!!available.error}
            submitLabel={t('teacher.activities.createButton')}
            resetOnSuccess
            onSubmit={create}
          />
        </CardContent>
      </Card>

      <h2 className="text-lg font-medium">{t('teacher.activities.listHeading')}</h2>
      {list.loading && !list.data ? <LoadingBlock /> : null}
      {list.error ? <ErrorBlock error={list.error} onRetry={list.reload} /> : null}
      {list.data && list.data.length === 0 ? <EmptyBlock>{t('teacher.activities.empty')}</EmptyBlock> : null}
      {list.data && list.data.length > 0 ? (
        <ul className="space-y-3">
          {list.data.map((a) => (
            <ActivityRow
              key={a.id}
              activity={a}
              topic={topics.find((tp) => tp.id === a.topic_id)}
              busy={busyIds.has(a.id)}
              error={rowError?.id === a.id ? rowError.message : undefined}
              onEdit={() => setEditing(a)}
              onToggle={() => void toggle(a)}
            />
          ))}
        </ul>
      ) : null}

      <Dialog open={editing !== null} onOpenChange={(o) => !o && setEditing(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('teacher.activities.editHeading')}</DialogTitle>
            <DialogDescription>{t('teacher.activities.editDescription')}</DialogDescription>
          </DialogHeader>
          {editing ? (
            <ActivityForm
              idPrefix="edit"
              initial={{
                title: editing.title,
                description: editing.description,
                topicId: editing.topic_id ?? '',
              }}
              topics={topics}
              topicsFailed={!!available.error}
              submitLabel={t('common.save')}
              onSubmit={(v) => update(editing, v)}
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </Page>
  )
}
