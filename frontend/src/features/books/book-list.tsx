import { useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { EmptyBlock, ErrorBlock, LoadingBlock } from '@/components/page'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { Activity, Topic } from '@/lib/types'
import { BookLanguageFilter } from './book-filters'
import { filterWorks } from './book-filtering'
import { RelatedEditions } from './related-editions'
import { BookForm } from './book-form'
import type { Book, BookInput, BookWork } from './types'

export function BookListEditor({ path, readOnly = false }: { path: string; readOnly?: boolean }) {
  const t = useT()
  const list = useFetch<BookWork[]>(path)
  const library = useFetch<BookWork[]>(readOnly ? null : '/api/book-works')
  const [language, setLanguage] = useState('all')
  const [savedList, setSavedList] = useState<BookWork[]>()
  const [selected, setSelected] = useState<BookWork[]>()
  const [query, setQuery] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()
  const [notice, setNotice] = useState(false)
  const [related, setRelated] = useState<Book>()
  const [creating, setCreating] = useState(false)
  const chosen = selected ?? list.data ?? []
  const baseline = savedList ?? list.data ?? []
  const added = chosen.filter((book) => !baseline.some((old) => old.id === book.id)).length
  const removed = baseline.filter((book) => !chosen.some((next) => next.id === book.id)).length
  const available = library.data && filterWorks(library.data.filter((book) => book.is_active && book.editions.some((edition) => edition.is_active) && !chosen.some((selected) => selected.id === book.id)), query, language)
  async function save() {
    if (readOnly || pending || !list.data) return
    setPending(true); setError(undefined); setNotice(false)
    try { const saved = await api.put<BookWork[]>(path, { work_ids: chosen.map((b) => b.id) }); setSelected(saved); setSavedList(saved); setNotice(true) }
    catch (err) { setError(err) } finally { setPending(false) }
  }
  async function create(value: BookInput) {
    const created = await api.post<Book>('/api/books', value)
    const work: BookWork = { id: created.work_id, title: created.title, is_active: true, created_at: created.created_at, updated_at: created.updated_at, editions: [created] }
    setRelated(created)
    setSelected([...chosen, work]); setNotice(false); setCreating(false); library.reload()
  }
  if (list.loading && !list.data) return <LoadingBlock />
  if (!list.data) return list.error ? <ErrorBlock error={list.error} onRetry={list.reload} /> : null
  return <div className="space-y-4">
    {related ? <RelatedEditions book={related} workTitle={related.title} close={() => setRelated(undefined)} changed={(work) => { setSelected(chosen.map((item) => item.id === work.id ? work : item)); library.reload() }} /> : null}
    {readOnly ? <p role="status">{t('books.readonly')}</p> : <p className="text-sm text-muted-foreground">{t('books.listChanges')}</p>}
    <h3 className="font-medium">{t('books.selectedCount', { count: chosen.length })}</h3>
    {!readOnly ? <p role="status">{t('books.pendingChanges', { added, removed })}</p> : null}
    {chosen.length === 0 ? <EmptyBlock>{t('books.listEmpty')}</EmptyBlock> : <ul className="space-y-2">{chosen.map((book) => <li key={book.id} className="flex flex-wrap items-center justify-between gap-2 rounded border p-3"><div><p className="font-medium break-words">{book.title}</p><p className="text-sm text-muted-foreground">{t('books.editionCount', { count: book.editions.length })}</p>{book.editions.length === 0 ? <p>{t('books.noEditions')}</p> : !book.editions.some((edition) => edition.is_active) ? <p>{t('books.noActiveEditions')}</p> : null}{!book.is_active ? <Badge variant="outline">{t('books.inactive')}</Badge> : null}</div>{!readOnly ? <Button variant="outline" disabled={pending} aria-label={t('books.remove', { title: book.title })} onClick={() => { setSelected(chosen.filter((b) => b.id !== book.id)); setNotice(false) }}>{t('books.remove', { title: book.title })}</Button> : null}</li>)}</ul>}
    {!readOnly ? <>
      <p className="text-sm text-muted-foreground">{t('books.removalHint')}</p>
      <section className="space-y-3 border-t pt-3"><h3 className="font-medium">{t('books.pick')}</h3>
        <Label htmlFor="book-list-filter">{t('books.search')}</Label><Input id="book-list-filter" type="search" value={query} onChange={(event) => setQuery(event.target.value)} />
        <BookLanguageFilter books={library.data?.flatMap((work) => work.editions) ?? []} value={language} change={setLanguage} />
        {available ? <p>{t('books.resultCount', { count: available.length })}</p> : null}
        {library.loading && !library.data ? <LoadingBlock /> : null}
        {library.error ? <ErrorBlock error={library.error} onRetry={library.reload} /> : null}
        {available?.length === 0 ? <EmptyBlock>{t('books.noMatch')}</EmptyBlock> : null}
        <ul className="max-h-60 space-y-2 overflow-y-auto">{available?.map((book) => <li key={book.id} className="flex flex-wrap items-center justify-between gap-2 rounded border p-3"><div><p className="break-words">{book.title}</p><p className="text-sm text-muted-foreground">{t('books.editionCount', { count: book.editions.length })}</p></div><Button variant="outline" disabled={pending} aria-label={t('books.add', { title: book.title })} onClick={() => { setSelected([...chosen, book]); setNotice(false) }}>{t('books.add', { title: book.title })}</Button></li>)}</ul>
        <Button variant="outline" disabled={pending} onClick={() => setCreating(true)}>{t('books.new')}</Button>
        {creating ? <section className="rounded border p-3"><h3>{t('books.new')}</h3><BookForm save={create} cancel={() => setCreating(false)} /></section> : null}
      </section>
      {error ? <Alert role="alert" variant="destructive"><AlertTitle>{errorText(t, error)}</AlertTitle></Alert> : null}
      {notice ? <p role="status">{t('books.saved')}</p> : null}
      <Button disabled={pending || creating} onClick={() => void save()}>{t(pending ? 'books.saving' : 'books.listSave')}</Button>
    </> : null}
  </div>
}

export function TopicBooksDialog({ topic, close }: { topic: Topic; close: () => void }) {
  const t = useT()
  return <Dialog open onOpenChange={(open) => !open && close()}><DialogContent showCloseButton={false} className="max-h-[90vh] overflow-y-auto sm:max-w-2xl"><DialogHeader><DialogTitle>{t('books.listFor', { title: topic.title })}</DialogTitle><DialogDescription>{t('books.topicHint')}</DialogDescription></DialogHeader><BookListEditor key={topic.id} path={`/api/topics/${encodeURIComponent(topic.id)}/books`} /><Button variant="outline" onClick={close}>{t('books.close')}</Button></DialogContent></Dialog>
}

export function ActivityBooksButton({ activity, classroomArchived }: { activity: Activity; classroomArchived: boolean }) {
  const t = useT()
  const [open, setOpen] = useState(false)
  return <><Button variant="outline" onClick={() => setOpen(true)}>{t('books.activityList')}</Button>{open ? <ActivityBooksDialog activity={activity} readOnly={activity.is_archived || classroomArchived} close={() => setOpen(false)} /> : null}</>
}

function ActivityBooksDialog({ activity, readOnly, close }: { activity: Activity; readOnly: boolean; close: () => void }) {
  const t = useT()
  const topics = useFetch<Topic[]>('/api/topics')
  const [selectedTopic, setSelectedTopic] = useState(activity.topic_ids[0] ?? '')
  const current = activity.topic_ids.includes(selectedTopic) ? selectedTopic : activity.topic_ids[0]
  return <Dialog open onOpenChange={(open) => !open && close()}><DialogContent showCloseButton={false} className="max-h-[90vh] overflow-y-auto sm:max-w-2xl"><DialogHeader><DialogTitle>{t('books.listFor', { title: activity.title })}</DialogTitle><DialogDescription>{t('books.activityHint')}</DialogDescription></DialogHeader>
    {topics.loading && !topics.data ? <LoadingBlock /> : null}{topics.error ? <ErrorBlock error={topics.error} onRetry={topics.reload} /> : null}
    {topics.data ? <><Label htmlFor="activity-book-topic">{t('books.activityList')}</Label><select id="activity-book-topic" className="w-full rounded border p-2" value={current} onChange={(event) => setSelectedTopic(event.target.value)}>{activity.topic_ids.map((id) => <option key={id} value={id}>{topics.data?.find((topic) => topic.id === id)?.title ?? id}</option>)}</select>{current ? <BookListEditor key={current} path={`/api/activities/${encodeURIComponent(activity.id)}/topics/${encodeURIComponent(current)}/books`} readOnly={readOnly} /> : null}</> : null}
    <Button variant="outline" onClick={close}>{t('books.close')}</Button>
  </DialogContent></Dialog>
}
