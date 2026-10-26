import { Archive, ArchiveRestore, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { ArchiveViewToggle } from '@/components/archive-view-toggle'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/features/chat/confirm-dialog'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { Conversation, Page as PageResult } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 我的對話列表（不含訊息）；可刪除對話。 */
export function ConversationListPage() {
  const [archived, setArchived] = useState(false)
  const [cursors, setCursors] = useState<(string | null)[]>([null])
  const cursor = cursors[cursors.length - 1]
  return <ConversationList key={`${archived}:${cursor ?? ''}`} archived={archived} onViewChange={(next) => { setArchived(next); setCursors([null]) }} cursor={cursor} onNext={(next) => setCursors((previous) => [...previous, next])} onPrevious={cursors.length > 1 ? () => setCursors((previous) => previous.slice(0, -1)) : undefined} />
}

function ConversationList({ archived, onViewChange, cursor, onNext, onPrevious }: { archived: boolean; onViewChange: (archived: boolean) => void; cursor: string | null; onNext: (cursor: string) => void; onPrevious?: () => void }) {
  const t = useT()
  const { data, error, loading, reload } = useFetch<PageResult<Conversation>>(`/api/conversations?${new URLSearchParams({ ...(archived ? { archived: 'true' } : {}), ...(cursor ? { cursor } : {}) }).toString()}`)
  const [removed, setRemoved] = useState<ReadonlySet<string>>(new Set())
  const [target, setTarget] = useState<Conversation>()
  const [pending, setPending] = useState(false)
  const [deleteError, setDeleteError] = useState<unknown>()

  const items = data?.items.filter((c) => !removed.has(c.id))

  async function toggleArchive(c: Conversation) {
    try {
      await api.post(`/api/conversations/${c.id}/${c.is_archived ? 'unarchive' : 'archive'}`)
      setRemoved((s) => new Set(s).add(c.id))
      toast.success(t(c.is_archived ? 'chat.list.restoredToast' : 'chat.list.archivedToast'))
    } catch (e) {
      toast.error(errorText(t, e))
    }
  }

  async function confirmDelete() {
    if (!target || pending) return
    setPending(true)
    setDeleteError(undefined)
    try {
      await api.delete(`/api/conversations/${target.id}`)
      setRemoved((s) => new Set(s).add(target.id))
      setTarget(undefined)
      toast.success(t('chat.list.deleted'))
    } catch (e) {
      if (isApiError(e) && e.code === 'not_found') {
        // 已在別處刪除：視為成功
        setRemoved((s) => new Set(s).add(target.id))
        setTarget(undefined)
      } else {
        setDeleteError(e)
      }
    } finally {
      setPending(false)
    }
  }

  return (
    <Page title={t('chat.list.title')}>
      <ArchiveViewToggle archived={archived} onChange={onViewChange} />
      {loading && !data ? <LoadingBlock /> : null}
      {error && !data ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {items && items.length === 0 ? (
        <EmptyBlock>
          <span className="block">{t(archived ? 'chat.list.emptyArchived' : 'chat.list.empty')}</span>
          <Button asChild variant="link">
            <Link to="/">{t('chat.list.browse')}</Link>
          </Button>
        </EmptyBlock>
      ) : null}
      {items && items.length > 0 ? (
        <ul className="space-y-3">
          {items.map((c) => (
            <ConversationRow
              key={c.id}
              conversation={c}
              onArchive={() => void toggleArchive(c)}
              onDelete={() => {
                setDeleteError(undefined)
                setTarget(c)
              }}
            />
          ))}
        </ul>
      ) : null}
      {data || onPrevious ? <div className="flex gap-2">
        {onPrevious ? <Button variant="outline" onClick={onPrevious}>{t('chat.list.previous')}</Button> : null}
        {data?.next_cursor ? <Button variant="outline" disabled={loading} onClick={() => onNext(data.next_cursor!)}>{t('chat.list.next')}</Button> : null}
      </div> : null}
      <ConfirmDialog
        open={target !== undefined}
        onOpenChange={(o) => {
          if (!o) setTarget(undefined)
        }}
        title={t('chat.list.deleteTitle')}
        description={t('chat.list.deleteBody')}
        confirmLabel={t('chat.list.deleteConfirm')}
        pending={pending}
        error={deleteError}
        onConfirm={confirmDelete}
        destructive
      />
    </Page>
  )
}

function ConversationRow({ conversation: c, onArchive, onDelete }: { conversation: Conversation; onArchive: () => void; onDelete: () => void }) {
  const t = useT()
  const { lang } = useI18n()
  const created = formatDate(c.created_at, lang)
  return (
    <li className="flex flex-col gap-3 rounded-lg border p-4 sm:flex-row sm:items-center sm:justify-between">
      <div className="min-w-0 space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          <Link
            to={`/conversations/${c.id}`}
            aria-label={t('chat.list.open', { title: c.title })}
            className="font-medium break-words underline-offset-4 hover:underline"
          >
            {c.title}
          </Link>
          <Badge variant={c.status === 'active' ? 'default' : 'secondary'}>{t(`chat.status.${c.status}`)}</Badge>
        </div>
        <p className="text-muted-foreground flex flex-wrap gap-x-3 text-sm">
          <span>{t('chat.list.turns', { count: c.turn_count })}</span>
          {created ? <span>{t('chat.list.created', { date: created })}</span> : null}
        </p>
      </div>
      <div className="flex gap-2 self-start sm:self-auto">
        <Button
          variant="outline"
          size="sm"
          aria-label={t(c.is_archived ? 'chat.list.restoreAria' : 'chat.list.archiveAria', { title: c.title })}
          onClick={onArchive}
        >
          {c.is_archived ? <ArchiveRestore aria-hidden /> : <Archive aria-hidden />}
          {t(c.is_archived ? 'chat.list.restore' : 'chat.list.archive')}
        </Button>
        <Button
          variant="outline"
          size="sm"
          aria-label={t('chat.list.delete', { title: c.title })}
          onClick={onDelete}
        >
          <Trash2 aria-hidden />
          {t('common.delete')}
        </Button>
      </div>
    </li>
  )
}
