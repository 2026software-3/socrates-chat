import { Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/features/chat/confirm-dialog'
import { useDateFormat } from '@/features/chat/format'
import { useI18n, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { Conversation } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 我的對話列表（不含訊息）；可刪除對話。 */
export function ConversationListPage() {
  const t = useT()
  const { data, error, loading, reload } = useFetch<Conversation[]>('/api/conversations')
  const [removed, setRemoved] = useState<ReadonlySet<string>>(new Set())
  const [target, setTarget] = useState<Conversation>()
  const [pending, setPending] = useState(false)
  const [deleteError, setDeleteError] = useState<unknown>()

  const items = data?.filter((c) => !removed.has(c.id))

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
      {loading && !data ? <LoadingBlock /> : null}
      {error && !data ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {items && items.length === 0 ? (
        <EmptyBlock>
          <span className="block">{t('chat.list.empty')}</span>
          <Button asChild variant="link">
            <Link to="/available">{t('chat.list.browse')}</Link>
          </Button>
        </EmptyBlock>
      ) : null}
      {items && items.length > 0 ? (
        <ul className="space-y-3">
          {items.map((c) => (
            <ConversationRow
              key={c.id}
              conversation={c}
              onDelete={() => {
                setDeleteError(undefined)
                setTarget(c)
              }}
            />
          ))}
        </ul>
      ) : null}
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

function ConversationRow({ conversation: c, onDelete }: { conversation: Conversation; onDelete: () => void }) {
  const t = useT()
  const { lang } = useI18n()
  const formatDate = useDateFormat(lang)
  const created = formatDate(c.created_at)
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
      <Button
        variant="outline"
        size="sm"
        className="self-start sm:self-auto"
        aria-label={t('chat.list.delete', { title: c.title })}
        onClick={onDelete}
      >
        <Trash2 aria-hidden />
        {t('common.delete')}
      </Button>
    </li>
  )
}
