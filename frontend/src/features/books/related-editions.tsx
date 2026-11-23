import { useState } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock } from '@/components/page'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import { normalizeBookLanguage } from './book-filtering'
import { validBookLanguage, validPublicationYear, safeBookUrl } from './book-validation'
import type { Book, BookCandidate, BookInput, BookWork } from './types'

type RelatedEdition = { work_id: string; work_title: string; book: BookCandidate }
export function RelatedEditions({ book, workTitle, close, changed }: { book: Book; workTitle: string; close: () => void; changed: (work: BookWork) => void }) {
  const t = useT()
  const results = useFetch<RelatedEdition[]>(`/api/books/${encodeURIComponent(book.id)}/editions`)
  const [selected, setSelected] = useState<string[]>([])
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()
  async function save() {
    if (pending || results.loading || results.error) return
    const books: BookInput[] = (results.data ?? []).filter(({ book: candidate }) => selected.includes(candidate.source_id)).map(({ book: candidate }) => {
      const { source, source_id, ...fields } = candidate
      const language = normalizeBookLanguage(fields.language)
      return { ...fields, language: validBookLanguage(language ?? '') ? language : null, publication_year: validPublicationYear(fields.publication_year ?? '') ? fields.publication_year : null, external_sources: [{ source, source_id }], recommendation_note: null }
    })
    if (!books.length) return
    setPending(true); setError(undefined)
    try {
      const work = await api.post<BookWork>(`/api/book-works/${encodeURIComponent(book.work_id)}/editions`, { confirm_title: workTitle, books })
      changed(work); close()
    } catch (err) { setError(err) } finally { setPending(false) }
  }
  return <Dialog open onOpenChange={(open) => !open && !pending && close()}><DialogContent showCloseButton={false} className="max-h-[90vh] overflow-y-auto sm:max-w-2xl"><DialogHeader><DialogTitle>{t('books.relatedTitle', { title: workTitle })}</DialogTitle><DialogDescription>{t('books.relatedHint')}</DialogDescription></DialogHeader>
    {results.loading ? <LoadingBlock /> : results.error ? <ErrorBlock error={results.error} onRetry={() => { setSelected([]); results.reload() }} /> : <>
      {!results.data?.length ? <EmptyBlock>{t('books.relatedEmpty')}</EmptyBlock> : null}
      <ul className="space-y-3">{results.data?.map(({ work_id, work_title, book: candidate }) => <li key={candidate.source_id} className="rounded border p-3 space-y-2"><label className="flex items-start gap-2"><input type="checkbox" disabled={pending} checked={selected.includes(candidate.source_id)} onChange={(event) => setSelected(event.target.checked ? [...selected, candidate.source_id] : selected.filter((id) => id !== candidate.source_id))} /><span>{candidate.title}</span></label>{!validBookLanguage(normalizeBookLanguage(candidate.language) ?? '') || !validPublicationYear(candidate.publication_year ?? '') ? <p>{t('books.fillSkipped')}</p> : null}<p>{candidate.language || t('books.languageMissing')} · {candidate.publication_year || t('books.yearMissing')}</p><p>{work_title} · Open Library {work_id}</p>{candidate.isbn ? <p>ISBN: {candidate.isbn}</p> : null}{safeBookUrl(candidate.source_url) ? <a className="underline" href={safeBookUrl(candidate.source_url)} target="_blank" rel="noreferrer">{t('books.sourceLink')}</a> : null}</li>)}</ul>
    </>}
    {error ? <ErrorBlock error={error} /> : null}
    <Button disabled={pending || results.loading || !!results.error || !selected.length} onClick={() => void save()}>{t(pending ? 'books.saving' : 'books.relatedImport')}</Button><Button variant="outline" disabled={pending} onClick={close}>{t('books.close')}</Button>
  </DialogContent></Dialog>
}
