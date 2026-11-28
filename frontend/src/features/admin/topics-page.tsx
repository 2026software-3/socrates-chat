import { TopicBooksDialog } from '@/features/books/book-list'
import { Pencil } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ConfirmDialog, ConfirmFact } from '@/features/chat/confirm-dialog'
import { errorText, useI18n, useT } from '@/i18n'
import { translateInBrowser } from '@/lib/browser-translator'
import { api } from '@/lib/api'
import type { Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'
import { GuidingQuestionsField } from './guiding-questions-field'
import { TopicDimensionsDialog } from './topic-dimensions'

type TopicInput = { title: string; questions: string[]; translations?: Topic['translations'] }

async function translateTopicFields(language: 'en' | 'es', sources: string[]) {
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([
      translateInBrowser('zh-TW', language, sources),
      new Promise<null>((resolve) => { timer = setTimeout(() => resolve(null), 5000) }),
    ])
  } finally {
    clearTimeout(timer)
  }
}

/** 去頭尾空白並略過空白項目。 */
const cleanQuestions = (qs: string[]) => qs.map((q) => q.trim()).filter(Boolean)

/** 新增與編輯共用的欄位表單；送出中停用按鈕，錯誤以翻譯後的訊息顯示在表單內。 */
function TopicForm({
  idPrefix,
  initial,
  submitLabel,
  pendingLabel,
  onSubmit,
  onCancel,
}: {
  idPrefix: string
  initial: TopicInput
  submitLabel: string
  pendingLabel: string
  onSubmit: (v: TopicInput) => Promise<void>
  onCancel?: () => void
}) {
  const t = useT()
  const [v, setV] = useState(initial)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()

  const fields = [
    { key: 'title', source: v.title.trim(), changed: v.title.trim() !== initial.title },
    ...cleanQuestions(v.questions).map((source) => ({ key: `q:${source}`, source, changed: !initial.questions.includes(source) })),
  ].filter((field) => field.source)

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (pending || !v.title.trim()) return
    setPending(true)
    setError(undefined)
    try {
      const translations: NonNullable<Topic['translations']> = {}
      const changed = fields.filter((field) => field.changed)
      await Promise.all((['en', 'es'] as const).map(async (language) => {
        const values: Record<string, string> = {}
        const generated = changed.length ? await translateTopicFields(language, changed.map((f) => f.source)) : null
        changed.forEach((field, index) => {
          if (generated?.[index]) values[field.key] = generated[index]
        })
        for (const field of fields.filter((f) => !f.changed)) {
          const edited = v.translations?.[language]?.[field.key]?.trim()
          if (edited && edited !== initial.translations?.[language]?.[field.key]) values[field.key] = edited
        }
        if (Object.keys(values).length) translations[language] = values
      }))
      await onSubmit({ ...v, translations: Object.keys(translations).length ? translations : undefined })
      setV(initial)
    } catch (err) {
      setError(err)
    } finally {
      setPending(false)
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3" noValidate>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-title`}>{t('admin.topics.titleLabel')}</Label>
        <Input
          id={`${idPrefix}-title`}
          value={v.title}
          required
          aria-invalid={error ? true : undefined}
          onChange={(e) => setV({ ...v, title: e.target.value })}
        />
      </div>
      <GuidingQuestionsField idPrefix={idPrefix} value={v.questions} onChange={(questions) => setV({ ...v, questions })} />
      {initial.title && fields.some((field) => field.changed) && <p role="status">{t('admin.topics.overwriteTranslations')}</p>}
      {initial.title && (['en', 'es'] as const).map((language) => (
        <fieldset key={language} className="space-y-2">
          <legend>{language === 'en' ? 'English' : 'Español'}</legend>
          {fields.filter((field) => !field.changed).map((field, index) => (
            <label key={index} className="block">
              {field.source}
              <Input aria-label={`${language} — ${field.source}`}
                placeholder={t('admin.topics.pendingTranslation')}
                value={v.translations?.[language]?.[field.key] ?? ''}
                onChange={(e) => setV({ ...v, translations: { ...v.translations,
                  [language]: { ...v.translations?.[language], [field.key]: e.target.value },
                } })} />
            </label>
          ))}
        </fieldset>
      ))}
      <div aria-live="polite">
        {error ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, error)}</AlertTitle>
          </Alert>
        ) : null}
      </div>
      <div className="flex gap-2">
        <Button type="submit" disabled={pending || !v.title.trim()}>
          {pending ? pendingLabel : submitLabel}
        </Button>
        {onCancel ? (
          <Button type="button" variant="outline" disabled={pending} onClick={onCancel}>
            {t('common.cancel')}
          </Button>
        ) : null}
      </div>
    </form>
  )
}

/**
 * 題目庫：新增、編輯、啟用／停用題目。GET、POST、PATCH 統一走 `/api/topics`
 * （教師也能放入題目，S-01.3），畫面相同。
 */
export function TopicsPage() {
  const t = useT()
  const { lang } = useI18n()
  const { data, error, loading, reload } = useFetch<Topic[]>('/api/topics')

  // 成功的修改先套在本機，不必等重新載入；資料重新載入（data 換了）就作廢，以伺服器為準
  const [patchState, setPatchState] = useState<{ base: Topic[] | undefined; map: Record<string, Partial<Topic>> }>({
    base: undefined,
    map: {},
  })
  const patched = useMemo(() => (patchState.base === data ? patchState.map : {}), [patchState, data])
  const [toggling, setToggling] = useState<ReadonlySet<string>>(new Set())
  const [actionError, setActionError] = useState<unknown>()
  const [notice, setNotice] = useState<string>()
  const [bookTopic, setBookTopic] = useState<Topic>()
  const [editing, setEditing] = useState<Topic>()
  const [reviewing, setReviewing] = useState<Topic>()
  const [creating, setCreating] = useState(false)
  const [confirming, setConfirming] = useState<{ topic: Topic; next: boolean }>()
  const [query, setQuery] = useState('')

  const topics = useMemo(() => (data ?? []).map((x) => ({ ...x, ...patched[x.id] })), [data, patched])
  const displayTitle = (topic: Topic) => topic.translations?.[lang]?.title ?? topic.title
  const q = query.trim().toLowerCase()
  const shown = topics.filter(
    (x) => !q || [displayTitle(x), x.title, ...(x.guiding_questions ?? [])].some((s) => s.toLowerCase().includes(q)),
  )
  const num = useMemo(() => new Intl.NumberFormat(lang), [lang])
  const filtered = q !== ''

  function patch(id: string, p: Partial<Topic>) {
    setPatchState((prev) => {
      const map = prev.base === data ? prev.map : {}
      return { base: data, map: { ...map, [id]: { ...map[id], ...p } } }
    })
  }

  async function create(v: TopicInput) {
    const body: Pick<Topic, 'title' | 'guiding_questions' | 'translations'> = { title: v.title.trim(), translations: v.translations }
    const questions = cleanQuestions(v.questions)
    if (questions.length > 0) body.guiding_questions = questions
    setNotice(undefined)
    await api.post('/api/topics', body)
    setCreating(false)
    setNotice(t('admin.topics.created', { title: body.title }))
    reload()
  }

  async function edit(topic: Topic, v: TopicInput) {
    const body: Partial<Pick<Topic, 'title' | 'guiding_questions' | 'translations'>> = {}
    if (v.title.trim() !== topic.title) body.title = v.title.trim()
    const questions = cleanQuestions(v.questions)
    const before = topic.guiding_questions ?? []
    if (questions.length !== before.length || questions.some((q, i) => q !== before[i])) body.guiding_questions = questions
    if (v.translations) body.translations = v.translations
    if (Object.keys(body).length > 0) {
      const updated = await api.patch<Topic>(`/api/topics/${encodeURIComponent(topic.id)}`, body)
      patch(topic.id, updated ?? body)
    }
    setEditing(undefined)
  }

  async function toggle(topic: Topic, next: boolean) {
    if (toggling.has(topic.id)) return
    setActionError(undefined)
    setToggling((s) => new Set(s).add(topic.id))
    patch(topic.id, { is_active: next }) // 樂觀更新
    try {
      await api.patch(`/api/topics/${encodeURIComponent(topic.id)}`, { is_active: next })
    } catch (err) {
      patch(topic.id, { is_active: !next }) // 失敗就還原
      setActionError(err)
    } finally {
      setToggling((s) => {
        const copy = new Set(s)
        copy.delete(topic.id)
        return copy
      })
      setConfirming(undefined)
    }
  }

  return (
    <Page
      title={t('admin.topics.title')}
      actions={
        <Button type="button" onClick={() => { setNotice(undefined); setCreating(true) }}>
          {t('admin.topics.new')}
        </Button>
      }
    >
      <p className="text-muted-foreground text-sm">{t('admin.topics.intro')}</p>

      <div aria-live="polite">
        {notice ? <p className="text-sm text-green-700 dark:text-green-400">{notice}</p> : null}
      </div>

      {actionError ? (
        <Alert variant="destructive" role="alert">
          <AlertTitle>{errorText(t, actionError)}</AlertTitle>
        </Alert>
      ) : null}

      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? (
        <div className="space-y-3">
          <div className="flex flex-col gap-2 md:flex-row md:items-end">
            <div className="flex-1 space-y-1.5">
              <Label htmlFor="topic-search">{t('admin.topics.searchLabel')}</Label>
              <Input id="topic-search" type="search" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
          </div>

          <p className="text-sm font-medium" aria-live="polite">
            {filtered
              ? t('admin.topics.countFiltered', { shown: num.format(shown.length), count: num.format(topics.length) })
              : t('admin.topics.count', { count: num.format(topics.length) })}
          </p>

          {topics.length === 0 ? (
            <EmptyBlock>{t('admin.topics.empty')}</EmptyBlock>
          ) : shown.length === 0 ? (
            <EmptyBlock>{t('admin.topics.noMatch')}</EmptyBlock>
          ) : (
            <div className="rounded-md border">
              <Table aria-label={t('admin.topics.list')}>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('admin.topics.col.title')}</TableHead>
                    <TableHead>{t('admin.topics.col.status')}</TableHead>
                    <TableHead>{t('admin.topics.col.actions')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {shown.map((x) => (
                    <TableRow key={x.id} className={x.is_active ? '' : 'text-muted-foreground'}>
                      <TableCell className="max-w-96 font-medium whitespace-normal break-words">{displayTitle(x)}</TableCell>
                      <TableCell>
                        <Badge variant={x.is_active ? 'default' : 'secondary'}>
                          {x.is_active ? t('admin.topics.active') : t('admin.topics.inactive')}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-wrap items-center gap-2">
                          <Button variant="ghost" size="icon" aria-label={t('admin.topics.edit', { title: displayTitle(x) })} onClick={() => setEditing(x)}>
                            <Pencil aria-hidden />
                          </Button>
                          <Button variant="outline" aria-label={t('books.openList', { title: displayTitle(x) })} onClick={() => setBookTopic(x)}>{t('books.topicList')}</Button>
                          <Button variant="outline" onClick={() => setReviewing({ ...x, title: displayTitle(x) })}>
                            {t('admin.dimensions.open')}
                          </Button>
                          <Button
                            variant="outline"
                            disabled={toggling.has(x.id)}
                            aria-label={t(x.is_active ? 'admin.topics.deactivateAria' : 'admin.topics.activateAria', { title: displayTitle(x) })}
                            onClick={() => setConfirming({ topic: x, next: !x.is_active })}
                          >
                            {x.is_active ? t('admin.topics.deactivate') : t('admin.topics.activate')}
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </div>
      ) : null}

      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent showCloseButton={false}>
          <DialogHeader>
            <DialogTitle>{t('admin.topics.create')}</DialogTitle>
            <DialogDescription>{t('admin.topics.createDesc')}</DialogDescription>
          </DialogHeader>
          <TopicForm
            idPrefix="create"
            initial={{ title: '', questions: [] }}
            submitLabel={t('admin.topics.create')}
            pendingLabel={t('admin.topics.creating')}
            onSubmit={create}
            onCancel={() => setCreating(false)}
          />
        </DialogContent>
      </Dialog>

      {confirming ? (
        <ConfirmDialog
          open
          onOpenChange={(open) => !open && setConfirming(undefined)}
          title={t(confirming.next ? 'admin.topics.activateTitle' : 'admin.topics.deactivateTitle')}
          description={t(confirming.next ? 'admin.topics.activateBody' : 'admin.topics.deactivateBody')}
          confirmLabel={t(confirming.next ? 'admin.topics.activateConfirm' : 'admin.topics.deactivateConfirm')}
          destructive={!confirming.next}
          pending={toggling.has(confirming.topic.id)}
          onConfirm={() => void toggle(confirming.topic, confirming.next)}
        >
          <ConfirmFact label={t('admin.topics.fact.topic')}>{displayTitle(confirming.topic)}</ConfirmFact>
        </ConfirmDialog>
      ) : null}

      {bookTopic ? <TopicBooksDialog key={bookTopic.id} topic={bookTopic} close={() => setBookTopic(undefined)} /> : null}

      {reviewing ? <TopicDimensionsDialog key={reviewing.id} topic={reviewing} close={() => setReviewing(undefined)} /> : null}

      <Dialog open={editing !== undefined} onOpenChange={(o) => !o && setEditing(undefined)}>
        <DialogContent showCloseButton={false}>
          <DialogHeader>
            <DialogTitle>{t('admin.topics.editTitle')}</DialogTitle>
            <DialogDescription>{t('admin.topics.editDesc')}</DialogDescription>
          </DialogHeader>
          {editing ? (
            <TopicForm
              key={editing.id}
              idPrefix="edit"
              initial={{ title: editing.title, questions: editing.guiding_questions ?? [], translations: editing.translations }}
              submitLabel={t('common.save')}
              pendingLabel={t('admin.topics.saving')}
              onSubmit={(v) => edit(editing, v)}
              onCancel={() => setEditing(undefined)}
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </Page>
  )
}
