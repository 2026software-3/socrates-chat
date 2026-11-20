import { useState } from 'react'
import { useMe } from '@/auth/auth-context'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { ConfirmDialog, ConfirmFact } from '@/features/chat/confirm-dialog'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { Book, BookWork, WorkImpact } from './types'

type Action = 'merge' | 'move' | 'delete' | 'status'
function WorkActionDialog({ work, book, works, action, close, changed }: { work?: BookWork; book?: Book; works: BookWork[]; action: Action; close: () => void; changed: () => void }) {
  const t = useT()
  const title = book?.title ?? work!.title
  const [target, setTarget] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()
  const impact = useFetch<WorkImpact>(action === 'status' ? null : book ? `/api/books/${encodeURIComponent(book.id)}/work-impact` : `/api/book-works/${encodeURIComponent(work!.id)}/impact`)
  const choosing = action === 'merge' || action === 'move'
  async function confirm() {
    if (pending || (action !== 'status' && !impact.data) || (choosing && !target)) return
    setPending(true); setError(undefined)
    try {
      if (action === 'move') await api.put(`/api/books/${encodeURIComponent(book!.id)}/work`, { work_id: target, confirm_title: title })
      else if (action === 'merge') await api.post(`/api/book-works/${encodeURIComponent(work!.id)}/merge`, { target_work_id: target, confirm_title: title })
      else if (action === 'delete') await api.delete(`/api/book-works/${encodeURIComponent(work!.id)}`, { confirm_title: title })
      else await api.patch(`/api/book-works/${encodeURIComponent(work!.id)}/status`, { is_active: !work!.is_active })
      changed(); close()
    } catch (err) { setError(err) } finally { setPending(false) }
  }
  return <ConfirmDialog open onOpenChange={(open) => !open && close()} title={t(`books.workAction.${action}`)} description={t(`books.workHint.${action}`)} confirmLabel={t('books.confirmWorkAction')} destructive={action === 'delete'} pending={pending} confirmDisabled={impact.loading || (action !== 'status' && !impact.data) || (choosing && !target)} error={error} onConfirm={() => void confirm()}>
    <ConfirmFact label="">{title}</ConfirmFact>
    {action === 'status' ? <p>{t(work!.is_active ? 'books.active' : 'books.inactive')} → {t(work!.is_active ? 'books.deactivate' : 'books.activate')}</p> : null}
    {target ? <p>{t('books.targetWork')}: {works.find((item) => item.id === target)?.title} · {t(works.find((item) => item.id === target)?.is_active ? 'books.active' : 'books.inactive')}</p> : null}
    {impact.loading ? <LoadingBlock /> : null}
    {impact.error ? <ErrorBlock error={impact.error} onRetry={impact.reload} /> : null}
    {impact.data ? <><p>{t('books.editionCount', { count: impact.data.edition_count })}</p><p>{t('books.impact', { topics: impact.data.topic_count, activities: impact.data.activity_topic_count })}</p>{book && impact.data.edition_count === 1 ? <p>{t('books.moveLastEdition')}</p> : null}</> : null}
    {choosing ? <div className="space-y-2"><Label htmlFor="target-work">{t('books.targetWork')}</Label><select id="target-work" className="w-full rounded border p-2" value={target} disabled={pending} onChange={(event) => setTarget(event.target.value)}><option value="">{t('books.chooseWork')}</option>{works.filter((candidate) => candidate.id !== (book?.work_id ?? work!.id)).map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.title} ({candidate.editions.length}) · {t(candidate.is_active ? 'books.active' : 'books.inactive')}</option>)}</select></div> : null}
  </ConfirmDialog>
}
export function WorkActions({ work, works, changed }: { work: BookWork; works: BookWork[]; changed: () => void }) {
  const t = useT(); const me = useMe(); const [action, setAction] = useState<Action>()
  return <div className="flex flex-wrap gap-2"><Button variant="outline" onClick={() => setAction('merge')}>{t('books.workAction.merge')}</Button>{me.roles.admin ? <><Button variant="outline" onClick={() => setAction('status')}>{t('books.workAction.status')}</Button><Button variant="destructive" onClick={() => setAction('delete')}>{t('books.workAction.delete')}</Button></> : null}{action ? <WorkActionDialog work={work} works={works} action={action} close={() => setAction(undefined)} changed={changed} /> : null}</div>
}
export function MoveEditionButton({ book, works, changed }: { book: Book; works: BookWork[]; changed: () => void }) {
  const t = useT(); const [open, setOpen] = useState(false)
  return <><Button variant="outline" onClick={() => setOpen(true)}>{t('books.workAction.move')}</Button>{open ? <WorkActionDialog book={book} works={works} action="move" close={() => setOpen(false)} changed={changed} /> : null}</>
}
