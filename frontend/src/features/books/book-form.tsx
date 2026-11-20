import { useId, useRef, useState, type FormEvent } from 'react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { errorText, useT } from '@/i18n'
import { bookLanguage, normalizeBookLanguage } from './book-filtering'
import { BookSearch } from './book-search'
import { textLimits, safeBookUrl, validIsbn } from './book-validation'
import type { BookCandidate, BookInput } from './types'

const emptyBook: BookInput = { external_sources: [], title: '', authors: null, language: null, publication_year: null, isbn: null, source_url: null, description: null, recommendation_note: null }
const bookFields = ['title', 'authors', 'language', 'publication_year', 'isbn', 'source_url', 'description', 'recommendation_note'] as const

export function BookForm({ initial = emptyBook, save, cancel }: { initial?: BookInput; save: (value: BookInput) => Promise<void>; cancel: () => void }) {
  const t = useT()
  const id = useId()
  const imported = useRef<Partial<BookInput>>({})
  const [value, setValue] = useState<BookInput>(() => ({ ...Object.fromEntries(bookFields.map((field) => [field, field === 'language' ? normalizeBookLanguage(initial[field]) : initial[field]])), external_sources: initial.external_sources ?? [] }) as BookInput)
  const [pending, setPending] = useState(false)
  const [validation, setValidation] = useState<Partial<Record<(typeof bookFields)[number], string>>>({})
  const [notice, setNotice] = useState<'skipped' | 'conflict' | false>(false)
  const [error, setError] = useState<unknown>()
  function choose(candidate: BookCandidate, fillOnly: boolean) {
    candidate = { ...candidate, language: normalizeBookLanguage(candidate.language) }
    const normalized = (text: string | null | undefined) => (text ?? '').trim().toLowerCase().replace(/\s+/g, ' ')
    const authors = (text: string | null) => normalized(text).split(';').map((part) => part.trim()).sort().join(';')
    const isbn = (text: string | null) => normalized(text).replace(/[\s-]/g, '')
    if (fillOnly && (
      (value.isbn?.trim() && isbn(value.isbn) !== isbn(candidate.isbn)) ||
      (value.authors?.trim() && candidate.authors?.trim() && authors(value.authors) !== authors(candidate.authors)) ||
      (['language', 'publication_year'] as const).some((field) => value[field]?.trim() && candidate[field]?.trim() && normalized(value[field]) !== normalized(candidate[field]))
    )) { setNotice('conflict'); return }

    let skipped = false
    let applied = false
    const next = { ...value }
    const nextImported: Partial<BookInput> = fillOnly ? { ...imported.current } : {}
    for (const field of bookFields) {
      if (field === 'recommendation_note' || (fillOnly && value[field]?.trim())) continue
      const proposed = candidate[field]?.trim()
      if (!proposed) {
        if (!fillOnly) {
          if (field === 'title') next.title = ''
          else next[field] = null
        }
        continue
      }
      const limit = textLimits[field]
      if ((limit && [...proposed].length > limit) || (field === 'source_url' && !safeBookUrl(proposed)) || (field === 'isbn' && !validIsbn(proposed))) {
        skipped = true
        if (!fillOnly && value[field] === imported.current[field]) {
          if (field === 'title') next.title = ''
          else next[field] = null
        }
        continue
      }
      applied = true
      next[field] = proposed
      nextImported[field] = proposed
    }
    const source = { source: candidate.source, source_id: candidate.source_id }
    if (!fillOnly) next.external_sources = [source]
    else if (applied && !value.external_sources.some((ref) => ref.source === source.source && ref.source_id === source.source_id)) next.external_sources = [...value.external_sources, source]
    imported.current = nextImported
    setValue(next)
    setValidation({})
    setNotice(skipped ? 'skipped' : false)
  }
  async function submit(event: FormEvent) {
    event.preventDefault()
    if (pending || !value.title.trim()) return
    const issues: Partial<Record<(typeof bookFields)[number], string>> = {}
    for (const field of bookFields) {
      const limit = textLimits[field]
      if (limit && [...(value[field]?.trim() ?? '')].length > limit) issues[field] = t('books.fieldTooLong', { field: t(`books.${field}`), max: limit })
    }
    if (value.source_url?.trim() && !safeBookUrl(value.source_url.trim())) issues.source_url = t('books.invalidUrl')
    if (value.isbn?.trim() && !validIsbn(value.isbn)) issues.isbn = t('books.invalidIsbn')
    setValidation(issues)
    if (Object.keys(issues).length) return
    setPending(true)
    setError(undefined)
    try {
      const body = { ...Object.fromEntries(bookFields.map((field) => [field, value[field]?.trim() || null])), external_sources: value.external_sources } as BookInput
      await save(body)
    } catch (err) { setError(err) } finally { setPending(false) }
  }
  return <form onSubmit={submit} className="space-y-4" noValidate>
    <BookSearch title={value.title} disabled={pending} choose={choose} />
    {notice ? <p role="status">{t(notice === 'conflict' ? 'books.fillConflict' : 'books.fillSkipped')}</p> : null}
    {bookFields.map((field) => <div key={field} className="space-y-1.5">
      <Label htmlFor={`${id}-${field}`}>{t(`books.${field}`)}</Label>
      {field === 'language'
        ? <select id={`${id}-${field}`} className="w-full rounded border p-2" disabled={pending} value={!value.language ? '' : ['zh', 'en', 'es'].includes(bookLanguage(value.language)) ? bookLanguage(value.language) : 'other'} onChange={(event) => setValue({ ...value, language: event.target.value || null })}>
          <option value="">{t('books.languageMissing')}</option>
          {(['zh', 'en', 'es', 'other'] as const).map((code) => <option key={code} value={code}>{t(`books.language.${code}`)}{value.language && code === 'other' && !['zh', 'en', 'es', 'other'].includes(bookLanguage(value.language)) ? ` (${value.language})` : ''}</option>)}
        </select>
        : field === 'description' || field === 'recommendation_note'
        ? <Textarea id={`${id}-${field}`} value={value[field] ?? ''} disabled={pending} aria-invalid={!!validation[field]} aria-describedby={validation[field] ? `${id}-${field}-error` : undefined} onChange={(event) => setValue({ ...value, [field]: event.target.value })} />
        : <Input id={`${id}-${field}`} value={value[field] ?? ''} required={field === 'title'} disabled={pending} aria-invalid={!!validation[field]} aria-describedby={validation[field] ? `${id}-${field}-error` : undefined} onChange={(event) => setValue({ ...value, [field]: event.target.value })} />}
      {validation[field] ? <p id={`${id}-${field}-error`} className="text-sm text-destructive">{validation[field]}</p> : null}
    </div>)}
    {Object.keys(validation).length ? <Alert role="alert" variant="destructive"><AlertTitle>{Object.values(validation).join(" ")}</AlertTitle></Alert> : null}
    {error ? <Alert variant="destructive" role="alert"><AlertTitle>{errorText(t, error)}</AlertTitle></Alert> : null}
    <div className="flex gap-2"><Button type="submit" disabled={pending || !value.title.trim()}>{t(pending ? 'books.saving' : 'books.save')}</Button><Button type="button" variant="outline" disabled={pending} onClick={cancel}>{t('common.cancel')}</Button></div>
  </form>
}
