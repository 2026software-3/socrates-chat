import { Pencil, Trash2 } from 'lucide-react'
import { useState, type FormEvent } from 'react'
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
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { errorText, useI18n, useT, type MessageKey } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'

type Announcement = {
  id: string
  kind: 'general' | 'downtime'
  text_zh_tw: string
  text_en: string | null
  text_es: string | null
  pending_languages: string[]
  starts_at: string
  ends_at: string | null
  enabled: boolean
}

const MAX_TEXT = 1000

/** 按鈕名稱裡帶的公告內容：太長的截短，讀螢幕時才不會整段念出來。 */
const shorten = (text: string) => (text.length > 30 ? `${text.slice(0, 30)}…` : text)

function statusOf(a: Announcement, now: number): 'active' | 'scheduled' | 'ended' | 'disabled' {
  if (!a.enabled) return 'disabled'
  if (Date.parse(a.starts_at) > now) return 'scheduled'
  if (a.ends_at && Date.parse(a.ends_at) <= now) return 'ended'
  return 'active'
}

/** 維護模式開關：開啟前先確認（會讓所有一般使用者立刻被擋下），關閉直接生效。 */
function MaintenanceSection() {
  const t = useT()
  const { data, error, loading, reload } = useFetch<{ enabled: boolean }>('/api/admin/maintenance')
  const [confirming, setConfirming] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<unknown>()

  async function set(enabled: boolean) {
    setSaving(true)
    setSaveError(undefined)
    try {
      await api.put('/api/admin/maintenance', { enabled })
      setConfirming(false)
      reload()
    } catch (err) {
      setSaveError(err)
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="space-y-3 rounded-md border p-4" aria-labelledby="maintenance-heading">
      <h2 id="maintenance-heading" className="text-lg font-medium">
        {t('admin.maintenance.title')}
      </h2>
      <p className="text-muted-foreground text-sm">{t('admin.maintenance.desc')}</p>
      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? (
        <div className="flex items-center gap-3">
          <Switch
            id="maintenance-switch"
            aria-label={t('admin.maintenance.switch')}
            checked={data.enabled}
            disabled={saving}
            onCheckedChange={(on) => {
              setSaveError(undefined)
              if (on) setConfirming(true)
              else void set(false)
            }}
          />
          <span className="text-sm font-medium">{data.enabled ? t('admin.maintenance.on') : t('admin.maintenance.off')}</span>
        </div>
      ) : null}
      {saveError && !confirming ? (
        <Alert variant="destructive" role="alert">
          <AlertTitle>{errorText(t, saveError)}</AlertTitle>
        </Alert>
      ) : null}

      <AlertDialog open={confirming} onOpenChange={(o) => !o && !saving && setConfirming(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('admin.maintenance.confirmTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('admin.maintenance.confirmDesc')}</AlertDialogDescription>
          </AlertDialogHeader>
          {saveError && confirming ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, saveError)}</AlertTitle>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={saving}>{t('common.cancel')}</AlertDialogCancel>
            <Button variant="destructive" disabled={saving} onClick={() => void set(true)}>
              {t('admin.maintenance.confirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  )
}

/** 公告與維護（S-15）：只有管理者進得來。 */
export function AdminAnnouncementsPage() {
  const t = useT()
  const { lang } = useI18n()
  const { data, error, loading, reload } = useFetch<Announcement[]>('/api/admin/announcements')

  const [text, setText] = useState('')
  const [kind, setKind] = useState<Announcement['kind']>('general')
  const [startsAt, setStartsAt] = useState('')
  const [endsAt, setEndsAt] = useState('')
  const [publishing, setPublishing] = useState(false)
  const [actionError, setActionError] = useState<unknown>()

  const [editing, setEditing] = useState<{ id: string; text: string }>()
  const [saving, setSaving] = useState(false)
  const [toDelete, setToDelete] = useState<Announcement>()
  const [deleting, setDeleting] = useState(false)

  const when = (iso: string) => new Date(iso).toLocaleString(lang, { dateStyle: 'medium', timeStyle: 'short' })

  async function publish(e: FormEvent) {
    e.preventDefault()
    if (publishing || !text.trim()) return
    setPublishing(true)
    setActionError(undefined)
    try {
      await api.post('/api/admin/announcements', {
        kind,
        text_zh_tw: text.trim(),
        ...(startsAt ? { starts_at: new Date(startsAt).toISOString() } : {}),
        ...(endsAt ? { ends_at: new Date(endsAt).toISOString() } : {}),
      })
      setText('')
      setStartsAt('')
      setEndsAt('')
      reload()
    } catch (err) {
      setActionError(err)
    } finally {
      setPublishing(false)
    }
  }

  async function patch(id: string, body: object) {
    setActionError(undefined)
    try {
      await api.patch(`/api/admin/announcements/${id}`, body)
      reload()
      return true
    } catch (err) {
      setActionError(err)
      return false
    }
  }

  async function saveEdit() {
    if (!editing || saving || !editing.text.trim()) return
    setSaving(true)
    if (await patch(editing.id, { text_zh_tw: editing.text.trim() })) setEditing(undefined)
    setSaving(false)
  }

  async function confirmDelete() {
    if (!toDelete || deleting) return
    setDeleting(true)
    setActionError(undefined)
    try {
      await api.delete(`/api/admin/announcements/${toDelete.id}`)
      setToDelete(undefined)
      reload()
    } catch (err) {
      setActionError(err)
      setToDelete(undefined)
    } finally {
      setDeleting(false)
    }
  }

  const now = Date.now()

  return (
    <Page title={t('admin.announcements.title')}>
      <MaintenanceSection />

      <section className="space-y-3" aria-labelledby="announcement-new-heading">
        <h2 id="announcement-new-heading" className="text-lg font-medium">
          {t('admin.announcements.sectionNew')}
        </h2>
        <p className="text-muted-foreground text-sm">{t('admin.announcements.intro')}</p>
        <form onSubmit={(e) => void publish(e)} className="space-y-3" noValidate>
          <div className="space-y-1.5">
            <Label htmlFor="announcement-text">{t('admin.announcements.text')}</Label>
            <Textarea id="announcement-text" rows={3} maxLength={MAX_TEXT} value={text} onChange={(e) => setText(e.target.value)} />
          </div>
          <div className="grid gap-3 sm:grid-cols-3">
            <div className="space-y-1.5">
              <Label htmlFor="announcement-kind">{t('admin.announcements.kind')}</Label>
              <select
                id="announcement-kind"
                value={kind}
                onChange={(e) => setKind(e.target.value as Announcement['kind'])}
                className="border-input bg-background h-8 w-full rounded-lg border px-2.5 text-sm"
              >
                <option value="general">{t('admin.announcements.kind.general')}</option>
                <option value="downtime">{t('admin.announcements.kind.downtime')}</option>
              </select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="announcement-starts">{t('admin.announcements.startsAt')}</Label>
              <Input id="announcement-starts" type="datetime-local" value={startsAt} onChange={(e) => setStartsAt(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="announcement-ends">{t('admin.announcements.endsAt')}</Label>
              <Input id="announcement-ends" type="datetime-local" value={endsAt} onChange={(e) => setEndsAt(e.target.value)} />
            </div>
          </div>
          <p className="text-muted-foreground text-xs">{t('admin.announcements.periodHint')}</p>
          <Button type="submit" disabled={publishing || !text.trim()}>
            {publishing ? t('admin.announcements.publishing') : t('admin.announcements.publish')}
          </Button>
        </form>
      </section>

      {actionError ? (
        <Alert variant="destructive" role="alert">
          <AlertTitle>{errorText(t, actionError)}</AlertTitle>
        </Alert>
      ) : null}

      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? (
        data.length === 0 ? (
          <EmptyBlock>{t('admin.announcements.empty')}</EmptyBlock>
        ) : (
          <ul aria-label={t('admin.announcements.list')} className="divide-y rounded-md border">
            {data.map((a) => {
              const status = statusOf(a, now)
              const label = shorten(a.text_zh_tw)
              const pending = a.pending_languages.map((l) => t(`admin.announcements.lang.${l}` as MessageKey)).join('、')
              return (
                <li key={a.id} className="space-y-2 px-3 py-3">
                  <div className="flex flex-wrap items-start justify-between gap-2">
                    <div className="min-w-0 flex-1 space-y-1">
                      {editing?.id === a.id ? (
                        <div className="space-y-2">
                          <Textarea
                            aria-label={t('admin.announcements.editText')}
                            rows={3}
                            maxLength={MAX_TEXT}
                            value={editing.text}
                            onChange={(e) => setEditing({ id: a.id, text: e.target.value })}
                          />
                          <p className="text-muted-foreground text-xs">{t('admin.announcements.editNote')}</p>
                          <div className="flex gap-2">
                            <Button size="sm" disabled={saving || !editing.text.trim()} onClick={() => void saveEdit()}>
                              {saving ? t('admin.announcements.saving') : t('admin.announcements.save')}
                            </Button>
                            <Button size="sm" variant="outline" disabled={saving} onClick={() => setEditing(undefined)}>
                              {t('common.cancel')}
                            </Button>
                          </div>
                        </div>
                      ) : (
                        <p className="text-sm whitespace-pre-line break-words">{a.text_zh_tw}</p>
                      )}
                      <p className="text-muted-foreground text-xs">
                        {t(`admin.announcements.kind.${a.kind}` as MessageKey)}
                        {' · '}
                        {t(`admin.announcements.status.${status}` as MessageKey)}
                        {' · '}
                        {a.ends_at
                          ? t('admin.announcements.periodTo', { from: when(a.starts_at), to: when(a.ends_at) })
                          : t('admin.announcements.period', { from: when(a.starts_at) })}
                      </p>
                      {a.pending_languages.length > 0 ? (
                        <p className="text-xs text-amber-700 dark:text-amber-400">{t('admin.announcements.pending', { languages: pending })}</p>
                      ) : null}
                    </div>
                    <div className="flex shrink-0 items-center gap-1">
                      <Button
                        variant="outline"
                        size="sm"
                        aria-label={t(a.enabled ? 'admin.announcements.disable' : 'admin.announcements.enable', { text: label })}
                        onClick={() => void patch(a.id, { enabled: !a.enabled })}
                      >
                        {a.enabled ? t('admin.announcements.disableAction') : t('admin.announcements.enableAction')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t('admin.announcements.edit', { text: label })}
                        onClick={() => setEditing({ id: a.id, text: a.text_zh_tw })}
                      >
                        <Pencil aria-hidden />
                      </Button>
                      <Button variant="ghost" size="icon" aria-label={t('admin.announcements.delete', { text: label })} onClick={() => setToDelete(a)}>
                        <Trash2 aria-hidden />
                      </Button>
                    </div>
                  </div>
                </li>
              )
            })}
          </ul>
        )
      ) : null}

      <AlertDialog open={toDelete !== undefined} onOpenChange={(o) => !o && !deleting && setToDelete(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('admin.announcements.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('admin.announcements.deleteDesc', { text: shorten(toDelete?.text_zh_tw ?? '') })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>{t('common.cancel')}</AlertDialogCancel>
            <Button variant="destructive" disabled={deleting} onClick={() => void confirmDelete()}>
              {deleting ? t('admin.announcements.deleting') : t('admin.announcements.deleteConfirm')}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  )
}
