import { useState } from 'react'
import { useMe } from '@/auth/auth-context'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import { WorkActions, MoveEditionButton } from './work-actions'
import { BookActionDialog } from './book-actions'
import { safeBookUrl } from './book-validation'
import { BookLanguageFilter } from './book-filters'
import { filterWorks } from './book-filtering'
import { BookForm } from './book-form'
import type { Book, BookInput, BookWork } from './types'

export function BooksPage() {
  const t = useT()
  const me = useMe()
  const books = useFetch<BookWork[]>('/api/book-works')
  const [viewing, setViewing] = useState<Book>()
  const [action, setAction] = useState<{ book: Book; action: 'delete' | 'status' }>()
  const [query, setQuery] = useState('')
  const [language, setLanguage] = useState('all')
  const [status, setStatus] = useState('all')
  const [sort, setSort] = useState('newest')
  const [editing, setEditing] = useState<Book | 'new'>()
  const shown = books.data && filterWorks(books.data.filter((book) => status === 'all' || book.is_active === (status === 'active')), query, language, sort)
  async function save(value: BookInput) {
    if (editing === 'new') await api.post<Book>('/api/books', value)
    else if (editing) await api.put<Book>(`/api/books/${encodeURIComponent(editing.id)}`, value)
    setEditing(undefined)
    books.reload()
  }
  return <Page title={t('books.nav')} description={t('books.intro')} actions={<Button onClick={() => setEditing('new')}>{t('books.new')}</Button>}>
    {books.data ? <div className="flex flex-wrap gap-2">{(['all', 'active', 'inactive'] as const).map((key) => <Button key={key} variant={status === key ? 'secondary' : 'outline'} aria-pressed={status === key} onClick={() => setStatus(key)}>{t(key === 'all' ? 'books.totalCount' : key === 'active' ? 'books.activeCount' : 'books.inactiveCount', { count: books.data!.filter((book) => key === 'all' || book.is_active === (key === 'active')).length })}</Button>)}</div> : null}
    {books.data ? <p>{t('books.libraryEditions', { count: books.data.reduce((sum, work) => sum + work.editions.length, 0) })}</p> : null}
    <div className="grid gap-3 sm:grid-cols-2"><BookLanguageFilter books={books.data?.flatMap((work) => work.editions) ?? []} value={language} change={setLanguage} />
      <div className="space-y-1.5"><Label htmlFor="book-sort">{t('books.sort')}</Label><select id="book-sort" className="w-full rounded border p-2" value={sort} onChange={(event) => setSort(event.target.value)}>{(['newest', 'oldest', 'title'] as const).map((value) => <option key={value} value={value}>{t(value === 'title' ? 'books.byTitle' : `books.${value}`)}</option>)}</select></div>
    </div>
    {shown ? <p role="status">{t('books.resultCount', { count: shown.length })}</p> : null}
    <div className="space-y-1.5"><Label htmlFor="book-query">{t('books.search')}</Label><Input id="book-query" type="search" value={query} onChange={(event) => setQuery(event.target.value)} /></div>
    {books.loading && !books.data ? <LoadingBlock /> : null}
    {books.error ? <ErrorBlock error={books.error} onRetry={books.reload} /> : null}
    {shown?.length === 0 ? <EmptyBlock>{t(books.data?.length ? 'books.noMatch' : 'books.empty')}</EmptyBlock> : null}
    <ul className="space-y-3">{shown?.map((work) => <li key={work.id} className="rounded-lg border p-4 space-y-2">
      <div className="flex flex-wrap items-center gap-3"><h2 className="font-semibold">{work.title}</h2><Badge>{t(work.is_active ? 'books.active' : 'books.inactive')}</Badge><span>{t('books.editionCount', { count: work.editions.length })}</span></div>
      {work.editions.length === 0 ? <p>{t('books.noEditions')}</p> : null}
      <WorkActions work={work} works={books.data ?? []} changed={books.reload} />
      <ul className="space-y-3">{work.editions.map((book) => <li key={book.id} className="rounded border p-3 space-y-2">
      <div className="flex flex-wrap items-center gap-3"><h3 className="font-semibold break-words">{book.title}</h3><Badge variant={book.is_active ? 'secondary' : 'outline'}>{t(book.is_active ? 'books.active' : 'books.inactive')}</Badge></div>
      <p className="text-muted-foreground text-sm">{book.authors || t('books.authorMissing')} · {book.language || t('books.languageMissing')} · {book.publication_year || t('books.yearMissing')}</p>
      <Button variant="outline" aria-label={t('books.view', { title: book.title })} onClick={() => setViewing(book)}>{t('books.view', { title: book.title })}</Button>
      <MoveEditionButton book={book} works={books.data ?? []} changed={books.reload} />
      {(me.roles.admin || me.id === book.created_by) ? <Button variant="outline" aria-label={t('books.edit', { title: book.title })} onClick={() => setEditing(book)}>{t('books.editTitle')}</Button> : null}
      {me.roles.admin ? <div className="flex flex-wrap gap-2"><Button variant="outline" onClick={() => setAction({ book, action: 'status' })}>{t(book.is_active ? 'books.deactivate' : 'books.activate')}</Button><Button variant="destructive" aria-label={t('books.delete', { title: book.title })} onClick={() => setAction({ book, action: 'delete' })}>{t('books.deleteTitle')}</Button></div> : null}
    </li>)}</ul></li>)}</ul>
    {action ? <BookActionDialog key={`${action.action}-${action.book.id}`} {...action} close={() => setAction(undefined)} changed={books.reload} /> : null}
    {viewing ? <Dialog open onOpenChange={(open) => !open && setViewing(undefined)}><DialogContent showCloseButton={false} className="max-h-[90vh] overflow-y-auto sm:max-w-2xl"><DialogHeader><DialogTitle>{viewing.title}</DialogTitle><DialogDescription>{viewing.authors || t('books.authorMissing')}</DialogDescription></DialogHeader><p>{viewing.language || t('books.languageMissing')} · {viewing.publication_year || t('books.yearMissing')}</p>{viewing.external_sources.map((source) => <p key={`${source.source}-${source.source_id}`}>{t('books.source', { source: source.source })}: {source.source_id}</p>)}{viewing.isbn ? <p>ISBN: {viewing.isbn}</p> : null}{viewing.description ? <p className="whitespace-pre-wrap break-words">{viewing.description}</p> : null}{viewing.recommendation_note ? <p className="whitespace-pre-wrap break-words">{viewing.recommendation_note}</p> : null}{safeBookUrl(viewing.source_url) ? <a href={safeBookUrl(viewing.source_url)} target="_blank" rel="noreferrer" className="underline">{t('books.sourceLink')}</a> : null}<Button variant="outline" onClick={() => setViewing(undefined)}>{t('books.close')}</Button></DialogContent></Dialog> : null}
    <Dialog open={editing !== undefined} onOpenChange={(open) => !open && setEditing(undefined)}><DialogContent showCloseButton={false} className="max-h-[90vh] overflow-y-auto sm:max-w-2xl"><DialogHeader><DialogTitle>{t(editing === 'new' ? 'books.new' : 'books.editTitle')}</DialogTitle><DialogDescription>{t('books.formHint')}</DialogDescription></DialogHeader>
      {editing ? <BookForm key={editing === 'new' ? 'new' : editing.id} initial={editing === 'new' ? undefined : editing} save={save} cancel={() => setEditing(undefined)} /> : null}
    </DialogContent></Dialog>
  </Page>
}
