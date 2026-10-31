import { Archive, ArchiveRestore, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { ArchiveViewToggle } from '@/components/archive-view-toggle'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ConfirmDialog } from '@/features/chat/confirm-dialog'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import { api, isApiError, isInvalidCursor } from '@/lib/api'
import type { Conversation, Page as PageResult } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 我的對話列表（不含訊息）；可刪除對話。 */
export function ConversationListPage() {
  const [params, setParams] = useSearchParams()
  const archived = params.get('archived') === 'true'
  const [cursors, setCursors] = useState<(string | null)[]>([null])
  const cursor = cursors[cursors.length - 1]
  return <ConversationList key={`${archived}:${cursor ?? ''}`} archived={archived} onViewChange={(next) => { setParams(next ? { archived: 'true' } : {}); setCursors([null]) }} cursor={cursor} onNext={(next) => setCursors((previous) => [...previous, next])} onPrevious={cursors.length > 1 ? () => setCursors((previous) => previous.slice(0, -1)) : undefined} onRestart={() => setCursors([null])} />
}

function ConversationList({ archived, onViewChange, cursor, onNext, onPrevious, onRestart }: { archived: boolean; onViewChange: (archived: boolean) => void; cursor: string | null; onNext: (cursor: string) => void; onPrevious?: () => void; onRestart: () => void }) {
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
    <Page title={t('chat.list.title')} description={t('chat.list.subtitle')}>
      <ArchiveViewToggle archived={archived} onChange={onViewChange} />
      {loading && !data ? <LoadingBlock /> : null}
      {error && !data ? <ErrorBlock error={error} onRetry={isInvalidCursor(error) ? onRestart : reload} /> : null}
      {items && items.length === 0 ? (
        <EmptyBlock>
          <span className="block">{t(archived ? 'chat.list.emptyArchived' : 'chat.list.empty')}</span>
          <Button asChild variant="link">
            <Link to="/">{t('chat.list.browse')}</Link>
          </Button>
        </EmptyBlock>
      ) : null}
      {items && items.length > 0 ? (
        <div className="rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('chat.list.col.title')}</TableHead>
                <TableHead>{t('chat.list.col.source')}</TableHead>
                <TableHead>{t('chat.list.col.turns')}</TableHead>
                <TableHead>{t('chat.list.col.created')}</TableHead>
                <TableHead>{t('chat.list.col.status')}</TableHead>
                <TableHead>
                  <span className="sr-only">{t('chat.list.col.actions')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
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
            </TableBody>
          </Table>
        </div>
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
  const ended = c.status === 'ended'
  return (
    <TableRow>
      <TableCell className="max-w-72 font-medium break-words whitespace-normal">{c.title}</TableCell>
      <TableCell>{t(c.activity_id ? 'chat.list.source.activity' : 'chat.list.source.personal')}</TableCell>
      <TableCell className="tabular-nums">{c.turn_count}</TableCell>
      <TableCell>{formatDate(c.created_at, lang)}</TableCell>
      <TableCell>{t(`chat.status.${c.status}`)}</TableCell>
      <TableCell>
        <div className="flex flex-wrap items-center justify-end gap-1">
          <Button asChild variant={ended ? 'outline' : 'default'}>
            <Link to={`/conversations/${c.id}`} aria-label={t('chat.list.open', { title: c.title })}>
              {t(ended ? 'chat.list.viewSummary' : 'chat.list.openAction')}
            </Link>
          </Button>
          <Button variant="ghost" size="icon" aria-label={t(c.is_archived ? 'chat.list.restoreAria' : 'chat.list.archiveAria', { title: c.title })} title={t(c.is_archived ? 'chat.list.restore' : 'chat.list.archive')} onClick={onArchive}>
            {c.is_archived ? <ArchiveRestore aria-hidden /> : <Archive aria-hidden />}
          </Button>
          <Button variant="ghost" size="icon" aria-label={t('chat.list.delete', { title: c.title })} title={t('common.delete')} onClick={onDelete}>
            <Trash2 aria-hidden />
          </Button>
        </div>
      </TableCell>
    </TableRow>
  )
}
