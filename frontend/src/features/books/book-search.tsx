import { useEffect, useId, useRef, useState } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useT } from '@/i18n'
import { api, isAbortError } from '@/lib/api'
import { safeBookUrl } from './book-validation'
import type { BookCandidate } from './types'

/** Search is read-only. The containing form alone owns persistence. */
export function BookSearch({ title, disabled, choose }: { title: string; disabled: boolean; choose: (book: BookCandidate, fillOnly: boolean) => void }) {
  const t = useT()
  const id = useId()
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<BookCandidate[]>()
  const [failed, setFailed] = useState(false)
  const [pending, setPending] = useState(false)
  const [fillOnly, setFillOnly] = useState(false)
  const latest = useRef({ title, query, choose, disabled })
  useEffect(() => { latest.current = { title, query, choose, disabled } }, [title, query, choose, disabled])
  const [searched, setSearched] = useState('')
  const controller = useRef<AbortController | null>(null)
  useEffect(() => () => controller.current?.abort(), [])
  async function search(fill: boolean) {
    const text = (fill ? title : query).trim()
    if (!text || pending || disabled) return
    controller.current?.abort()
    const ctrl = new AbortController()
    controller.current = ctrl
    setPending(true); setFailed(false); setResults(undefined); setFillOnly(fill); setSearched(text)
    try {
      const found = await api.get<BookCandidate[]>(`/api/books/search?q=${encodeURIComponent(text)}`, ctrl.signal)
      if (ctrl.signal.aborted || latest.current.disabled || (fill ? latest.current.title : latest.current.query).trim() !== text) return
      if (fill && found.length === 1) latest.current.choose(found[0], true)
      else setResults(found)
    }
    catch (err) { if (!isAbortError(err)) setFailed(true) }
    finally { if (!ctrl.signal.aborted) setPending(false) }
  }
  const visible = searched === (fillOnly ? title : query).trim() ? results : undefined
  return <section className="space-y-3 rounded-lg border p-3">
    <Label htmlFor={id}>{t('books.externalQuery')}</Label><Input id={id} value={query} disabled={disabled || pending} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void search(false) } }} />
    <div className="flex flex-wrap gap-2"><Button type="button" variant="outline" disabled={disabled || pending || !query.trim()} onClick={() => void search(false)}>{t(pending ? 'books.searching' : 'books.externalSearch')}</Button><Button type="button" variant="outline" disabled={disabled || pending || !title.trim()} onClick={() => void search(true)}>{t('books.supplement')}</Button></div>
    {pending ? <p role="status">{t('books.searching')}</p> : null}
    {failed ? <Alert role="alert"><AlertTitle>{t('books.externalFailure')}</AlertTitle></Alert> : null}
    {visible?.length === 0 ? <p role="status">{t('books.externalEmpty')}</p> : null}
    {visible?.length ? <ul className="max-h-64 space-y-2 overflow-y-auto">{visible.map((book) => <li key={`${book.source}-${book.source_id}`} className="space-y-1 rounded border p-3">
      <p className="font-medium break-words">{book.title}</p><p className="text-sm text-muted-foreground">{book.authors || t('books.authorMissing')} · {book.language || t('books.languageMissing')} · {book.publication_year || t('books.yearMissing')}</p>
      {book.isbn ? <p className="text-sm">ISBN: {book.isbn}</p> : null}
      <p className="text-xs">{t('books.source', { source: book.source === 'google_books' || book.source === 'google' ? 'Google Books' : book.source === 'open_library' || book.source === 'openlibrary' ? 'Open Library' : book.source })}</p>
      <div className="flex gap-3 items-center"><Button type="button" variant="outline" disabled={disabled} onClick={() => { choose(book, fillOnly); setResults(undefined) }}>{t('books.select')}</Button>{safeBookUrl(book.source_url) ? <a className="underline" href={safeBookUrl(book.source_url)} target="_blank" rel="noreferrer">{t('books.sourceLink')}</a> : null}</div>
    </li>)}</ul> : null}
  </section>
}
