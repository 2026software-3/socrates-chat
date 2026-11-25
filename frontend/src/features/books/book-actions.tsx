import { useState } from 'react'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { ConfirmDialog, ConfirmFact } from '@/features/chat/confirm-dialog'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { Book, DeletionImpact } from './types'

export function BookActionDialog({ book, action, close, changed }: { book: Book; action: 'delete' | 'status'; close: () => void; changed: () => void }) {
  const t = useT()
  const impact = useFetch<DeletionImpact>(action === 'delete' ? `/api/books/${encodeURIComponent(book.id)}/deletion-impact` : null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()
  async function confirm() {
    if (pending || (action === 'delete' && !impact.data)) return
    setPending(true)
    setError(undefined)
    try {
      if (action === 'delete') await api.delete(`/api/books/${encodeURIComponent(book.id)}`, { confirm_title: book.title })
      else await api.patch(`/api/books/${encodeURIComponent(book.id)}/status`, { is_active: !book.is_active })
      changed()
      close()
    } catch (err) { setError(err) } finally { setPending(false) }
  }
  return <ConfirmDialog open onOpenChange={(open) => !open && close()} title={t(action === 'delete' ? 'books.deleteTitle' : 'books.statusTitle')} description={t(action === 'delete' ? 'books.deleteBody' : 'books.statusHint')} confirmLabel={t(action === 'delete' ? 'books.deleteConfirm' : book.is_active ? 'books.deactivate' : 'books.activate')} destructive={action === 'delete' || book.is_active} pending={pending || impact.loading} error={error} onConfirm={() => void confirm()}>
    <ConfirmFact label="">{book.title}</ConfirmFact>
    {impact.loading ? <LoadingBlock /> : null}
    {impact.error ? <ErrorBlock error={impact.error} onRetry={impact.reload} /> : null}
    {impact.data?.deletes_work ? <p>{t('books.deleteLastEdition')}</p> : null}
    {impact.data ? <p>{t('books.impact', { topics: impact.data.topic_count, activities: impact.data.activity_topic_count })}</p> : null}
  </ConfirmDialog>
}
