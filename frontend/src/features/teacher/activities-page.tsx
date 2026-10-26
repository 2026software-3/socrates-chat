import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { ArchiveViewToggle } from '@/components/archive-view-toggle'
import { NameConfirmDialog } from '@/components/name-confirm-dialog'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { LoadMore } from '@/components/load-more'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Activity, ActivityStatus, Classroom, Topic } from '@/lib/types'
import { useFetch, usePagedFetch } from '@/lib/use-fetch'

const STATUS_VARIANT: Record<ActivityStatus, 'secondary' | 'default' | 'outline'> = {
  draft: 'secondary',
  published: 'default',
  closed: 'outline',
}

type Values = { title: string; description: string; topicId: string; opensAt: string; closesAt: string }

/** datetime-local uses the browser's time zone; retain seconds and milliseconds on edit. */
function localTime(iso: string | null): string {
  if (!iso) return ''
  const d = new Date(iso)
  return new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, -1)
}

function utcTime(local: string): string | null {
  return local ? new Date(local).toISOString() : null
}

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
  const [topicError, setTopicError] = useState(false)
  const [serverError, setServerError] = useState<string>()
  const busy = useRef(false)
  const hasInitialTopic = initial.topicId !== ''

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (busy.current) return
    // 活動必須選題庫題目（S-04.7）；標題與題目都缺時一起提示
    const titleMissing = !values.title.trim()
    const topicMissing = !values.topicId
    setTitleError(titleMissing)
    setTopicError(topicMissing)
    if (titleMissing || topicMissing) return
    if ((values.opensAt && !Number.isFinite(new Date(values.opensAt).getTime())) ||
        (values.closesAt && !Number.isFinite(new Date(values.closesAt).getTime())) ||
        (values.opensAt && values.closesAt && new Date(values.closesAt) <= new Date(values.opensAt))) {
      setServerError(t('error.invalid_schedule'))
      return
    }
    setServerError(undefined)
    busy.current = true
    setPending(true)
    try {
      await onSubmit({ ...values, title: values.title.trim() })
      if (resetOnSuccess) setValues(initial)
    } catch (err) {
      // 前端自己丟的一般 Error（例如尚無可用教室）直接顯示其訊息
      setServerError(err instanceof Error && err.name !== 'ApiError' ? err.message : errorText(t, err))
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
          aria-invalid={topicError}
          aria-describedby={topicError ? `${idPrefix}-topic-error` : undefined}
          onChange={(e) => setValues({ ...values, topicId: e.target.value })}
        >
          <option value="" disabled>
            {t('teacher.activities.topicPlaceholder')}
          </option>
          {hasInitialTopic && !topics.some((tp) => tp.id === initial.topicId) ? (
            <option value={initial.topicId}>{t('teacher.activities.topicUnavailable')}</option>
          ) : null}
          {topics.map((tp) => (
            <option key={tp.id} value={tp.id}>
              {tp.title}
            </option>
          ))}
        </select>
        {topicError ? (
          <p id={`${idPrefix}-topic-error`} role="alert" className="text-destructive text-sm">
            {t('teacher.activities.topicRequired')}
          </p>
        ) : null}
        {topicsFailed ? <p className="text-muted-foreground text-sm">{t('teacher.activities.topicsFailed')}</p> : null}
      </div>
      {(['opensAt', 'closesAt'] as const).map((field) => (
        <div key={field} className="space-y-1.5">
          <Label htmlFor={`${idPrefix}-${field}`}>{t(field === 'opensAt' ? 'teacher.activities.opensAt' : 'teacher.activities.closesAt')}</Label>
          <Input id={`${idPrefix}-${field}`} type="datetime-local" step="0.001" value={values[field]}
            onChange={(e) => setValues({ ...values, [field]: e.target.value })} />
        </div>
      ))}
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
  archived,
  onEdit,
  onToggle,
  onArchive,
  onDelete,
}: {
  activity: Activity
  topic: Topic | undefined
  busy: boolean
  archived: boolean
  error: string | undefined
  onEdit: () => void
  onToggle: () => void
  onArchive: () => void
  onDelete: () => void
}) {
  const t = useT()
  const { lang } = useI18n()
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    // Refresh on the exact next boundary, then schedule the following one.
    const next = [activity.opens_at, activity.closes_at].filter((v): v is string => !!v)
      .map((v) => new Date(v).getTime()).filter((v) => v > now).sort((a, b) => a - b)[0]
    if (next === undefined) return
    const timer = window.setTimeout(() => setNow(Date.now()), Math.min(next - now, 2147483647))
    return () => window.clearTimeout(timer)
  }, [activity.opens_at, activity.closes_at, now])
  const isOpen = !archived && !activity.is_archived && activity.status === 'published' &&
    (!activity.opens_at || now >= new Date(activity.opens_at).getTime()) &&
    (!activity.closes_at || now < new Date(activity.closes_at).getTime())
  const dateTime = (iso: string) => new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(iso))
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
            <p className="text-sm">{t(isOpen ? 'teacher.activities.openNow' : 'teacher.activities.notOpen')}</p>
            <p className="text-muted-foreground text-sm">
              {t('teacher.activities.opensAt')}: {activity.opens_at ? dateTime(activity.opens_at) : t('teacher.activities.noLimit')}
              {' · '}{t('teacher.activities.closesAt')}: {activity.closes_at ? dateTime(activity.closes_at) : t('teacher.activities.noLimit')}
            </p>
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
            {activity.is_archived ? null : (
              <Button size="sm" onClick={onToggle} disabled={busy}>
                {t(toggleKey[activity.status])}
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={onArchive} disabled={busy}>
              {t(activity.is_archived ? 'teacher.activities.restore' : 'teacher.activities.archive')}
            </Button>
            <Button variant="outline" size="sm" onClick={onDelete}>
              {t('teacher.activities.delete')}
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
  const classrooms = useFetch<Classroom[]>('/api/classrooms')
  const [roomId, setRoomId] = useState('')
  const writableRooms = useMemo(() => (classrooms.data ?? []).filter((r) => !r.is_archived), [classrooms.data])
  // 只有確定載入且未封存的教室才可編輯；載入中或失敗時視為不可用
  const writableRoomIds = useMemo(() => new Set(writableRooms.map((r) => r.id)), [writableRooms])
  const selectedRoom = writableRooms.some((r) => r.id === roomId) ? roomId : writableRooms[0]?.id
  const [showArchived, setShowArchived] = useState(false)
  const [deleting, setDeleting] = useState<Activity>()
  const list = usePagedFetch<Activity>(showArchived ? '/api/activities?archived=true' : '/api/activities')
  // /api/topics 列出全部題目；只提供啟用中的題目，與學生端目錄一致
  const available = useFetch<Topic[]>('/api/topics')
  const topics = useMemo(() => (available.data ?? []).filter((tp) => tp.is_active), [available.data])
  const topicById = useMemo(() => new Map(topics.map((tp) => [tp.id, tp])), [topics])
  const [editing, setEditing] = useState<Activity | null>(null)
  const [busyIds, setBusyIds] = useState<ReadonlySet<string>>(new Set())
  const busyRef = useRef(new Set<string>())
  // 以活動 id 記錄列內錯誤，顯示在失敗的那一列
  const [rowError, setRowError] = useState<{ id: string; message: string }>()

  async function create(v: Values) {
    if (!selectedRoom) throw new Error(t('teacher.activities.roomRequired'))
    await api.post<Activity>('/api/activities', {
      title: v.title,
      ...(v.description.trim() ? { description: v.description.trim() } : {}),
      topic_id: v.topicId,
      classroom_id: selectedRoom,
      ...(v.opensAt ? { opens_at: utcTime(v.opensAt) } : {}),
      ...(v.closesAt ? { closes_at: utcTime(v.closesAt) } : {}),
    })
    toast.success(t('teacher.activities.created'))
    setRowError(undefined)
    list.reload()
  }

  async function update(a: Activity, v: Values) {
    // 只在題目有變動時才送 topic_id：已有學生開始的活動不能換題目（409 activity_topic_locked），沒換就不該碰到它
    const topicChanged = v.topicId !== a.topic_id
    await api.patch<Activity>(`/api/activities/${a.id}`, {
      title: v.title,
      description: v.description.trim(),
      ...(topicChanged ? { topic_id: v.topicId } : {}),
      ...(v.opensAt !== localTime(a.opens_at) ? { opens_at: utcTime(v.opensAt) } : {}),
      ...(v.closesAt !== localTime(a.closes_at) ? { closes_at: utcTime(v.closesAt) } : {}),
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

  async function archive(a: Activity) {
    if (busyRef.current.has(a.id)) return
    busyRef.current.add(a.id)
    setBusyIds(new Set(busyRef.current))
    setRowError(undefined)
    try {
      await api.post<Activity>(`/api/activities/${a.id}/${a.is_archived ? 'unarchive' : 'archive'}`)
      toast.success(t(a.is_archived ? 'teacher.activities.restoredToast' : 'teacher.activities.archivedToast'))
      list.reload()
    } catch (err) {
      setRowError({ id: a.id, message: errorText(t, err) })
    } finally {
      busyRef.current.delete(a.id)
      setBusyIds(new Set(busyRef.current))
    }
  }

  async function remove(a: Activity) {
    await api.delete(`/api/activities/${a.id}`, { confirm_title: a.title })
    setDeleting(undefined)
    toast.success(t('teacher.activities.deleted'))
    list.reload()
  }

  return (
    <Page title={t('teacher.activities.title')}>
      <Card>
        <CardContent className="space-y-3">
          <h2 className="text-lg font-medium">{t('teacher.activities.createHeading')}</h2>
          <Label htmlFor="activity-classroom">{t('teacher.activities.classroom')}</Label>
          <select id="activity-classroom" className={selectClass} value={selectedRoom ?? ''} onChange={(e) => setRoomId(e.target.value)}>
            {writableRooms.map((room) => <option key={room.id} value={room.id}>{room.name}</option>)}
          </select>
          {classrooms.error ? <ErrorBlock error={classrooms.error} onRetry={classrooms.reload} /> : null}
          <ActivityForm
            idPrefix="create"
            initial={{ title: '', description: '', topicId: '', opensAt: '', closesAt: '' }}
            topics={topics}
            topicsFailed={!!available.error}
            submitLabel={t('teacher.activities.createButton')}
            resetOnSuccess
            onSubmit={create}
          />
        </CardContent>
      </Card>

      <h2 className="text-lg font-medium">{t('teacher.activities.listHeading')}</h2>
      <ArchiveViewToggle archived={showArchived} onChange={setShowArchived} />
      {list.loading && !list.items ? <LoadingBlock /> : null}
      {list.error ? <ErrorBlock error={list.error} onRetry={list.reload} /> : null}
      {list.items && list.items.length === 0 ? <EmptyBlock>{t(showArchived ? 'teacher.activities.emptyArchived' : 'teacher.activities.empty')}</EmptyBlock> : null}
      {list.items && list.items.length > 0 ? (
        <ul className="space-y-3">
          {list.items.map((a) => (
            <ActivityRow
              key={a.id}
              activity={a}
              topic={topicById.get(a.topic_id)}
              archived={!writableRoomIds.has(a.classroom_id)}
              busy={busyIds.has(a.id) || !writableRoomIds.has(a.classroom_id)}
              error={rowError?.id === a.id ? rowError.message : undefined}
              onEdit={() => setEditing(a)}
              onToggle={() => void toggle(a)}
              onArchive={() => void archive(a)}
              onDelete={() => setDeleting(a)}
            />
          ))}
        </ul>
      ) : null}
      <LoadMore hasMore={list.hasMore} loading={list.loadingMore} error={list.moreError} onLoadMore={list.loadMore} />

      {deleting ? (
        <NameConfirmDialog
          title={t('teacher.activities.deleteTitle')}
          description={t('teacher.activities.deleteBody')}
          name={deleting.title}
          onConfirm={() => remove(deleting)}
          onCancel={() => setDeleting(undefined)}
        />
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
                topicId: editing.topic_id,
                opensAt: localTime(editing.opens_at),
                closesAt: localTime(editing.closes_at),
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
