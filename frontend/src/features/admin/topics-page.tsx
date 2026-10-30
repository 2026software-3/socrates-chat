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
import { Textarea } from '@/components/ui/textarea'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type TopicInput = { title: string; description: string; category: string }

/** 新增與編輯共用的欄位表單；送出中停用按鈕，錯誤以翻譯後的訊息顯示在表單內。 */
function TopicForm({
  idPrefix,
  initial,
  submitLabel,
  pendingLabel,
  categories,
  onSubmit,
  onCancel,
}: {
  idPrefix: string
  initial: TopicInput
  submitLabel: string
  pendingLabel: string
  categories: string[]
  onSubmit: (v: TopicInput) => Promise<void>
  onCancel?: () => void
}) {
  const t = useT()
  const [v, setV] = useState(initial)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (pending || !v.title.trim()) return
    setPending(true)
    setError(undefined)
    try {
      await onSubmit(v)
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
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-description`}>{t('admin.topics.descriptionLabel')}</Label>
        <Textarea
          id={`${idPrefix}-description`}
          rows={3}
          value={v.description}
          onChange={(e) => setV({ ...v, description: e.target.value })}
        />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-category`}>{t('admin.topics.categoryLabel')}</Label>
        <Input
          id={`${idPrefix}-category`}
          list={`${idPrefix}-categories`}
          value={v.category}
          onChange={(e) => setV({ ...v, category: e.target.value })}
        />
        <datalist id={`${idPrefix}-categories`}>
          {categories.map((c) => (
            <option key={c} value={c} />
          ))}
        </datalist>
      </div>
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
  const [editing, setEditing] = useState<Topic>()
  const [creating, setCreating] = useState(false)
  const [confirming, setConfirming] = useState<{ topic: Topic; next: boolean }>()
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState('')

  const topics = useMemo(() => (data ?? []).map((x) => ({ ...x, ...patched[x.id] })), [data, patched])
  const categories = useMemo(
    () => [...new Set(topics.map((x) => x.category).filter((c): c is string => !!c))].sort((a, b) => a.localeCompare(b)),
    [topics],
  )
  // 選取的分類若已不存在（例如最後一題被改走），視為「全部」
  const activeCategory = categories.includes(category) ? category : ''
  const q = query.trim().toLowerCase()
  const shown = topics.filter(
    (x) =>
      (!activeCategory || x.category === activeCategory) &&
      (!q || [x.title, x.description, x.category ?? ''].some((s) => s.toLowerCase().includes(q))),
  )
  const num = useMemo(() => new Intl.NumberFormat(lang), [lang])
  const filtered = q !== '' || activeCategory !== ''

  function patch(id: string, p: Partial<Topic>) {
    setPatchState((prev) => {
      const map = prev.base === data ? prev.map : {}
      return { base: data, map: { ...map, [id]: { ...map[id], ...p } } }
    })
  }

  async function create(v: TopicInput) {
    const body: { title: string; description?: string; category?: string } = { title: v.title.trim() }
    if (v.description.trim()) body.description = v.description.trim()
    if (v.category.trim()) body.category = v.category.trim()
    setNotice(undefined)
    await api.post('/api/topics', body)
    setCreating(false)
    setNotice(t('admin.topics.created', { title: body.title }))
    reload()
  }

  async function edit(topic: Topic, v: TopicInput) {
    const body: Partial<Pick<Topic, 'title' | 'description' | 'category'>> = {}
    if (v.title.trim() !== topic.title) body.title = v.title.trim()
    if (v.description.trim() !== topic.description.trim()) body.description = v.description.trim()
    // 後端 PATCH：沒帶＝保留、null＝清除分類
    const category = v.category.trim()
    if (category !== (topic.category ?? '')) body.category = category === '' ? null : category
    if (Object.keys(body).length > 0) {
      await api.patch(`/api/topics/${encodeURIComponent(topic.id)}`, body)
      patch(topic.id, body)
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
            <div className="space-y-1.5 md:w-56">
              <Label htmlFor="topic-category-filter">{t('admin.topics.categoryFilter')}</Label>
              <select
                id="topic-category-filter"
                value={activeCategory}
                onChange={(e) => setCategory(e.target.value)}
                className="border-input bg-background h-8 w-full rounded-lg border px-2 text-sm"
              >
                <option value="">{t('admin.topics.allCategories')}</option>
                {categories.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
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
                    <TableHead>{t('admin.topics.col.category')}</TableHead>
                    <TableHead>{t('admin.topics.col.status')}</TableHead>
                    <TableHead>{t('admin.topics.col.actions')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {shown.map((x) => (
                    <TableRow key={x.id} className={x.is_active ? '' : 'text-muted-foreground'}>
                      <TableCell className="max-w-96 font-medium whitespace-normal break-words">{x.title}</TableCell>
                      <TableCell>{x.category ?? '—'}</TableCell>
                      <TableCell>
                        <Badge variant={x.is_active ? 'default' : 'secondary'}>
                          {x.is_active ? t('admin.topics.active') : t('admin.topics.inactive')}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-wrap items-center gap-2">
                          <Button variant="ghost" size="icon" aria-label={t('admin.topics.edit', { title: x.title })} onClick={() => setEditing(x)}>
                            <Pencil aria-hidden />
                          </Button>
                          <Button
                            variant="outline"
                            disabled={toggling.has(x.id)}
                            aria-label={t(x.is_active ? 'admin.topics.deactivateAria' : 'admin.topics.activateAria', { title: x.title })}
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
            initial={{ title: '', description: '', category: '' }}
            submitLabel={t('admin.topics.create')}
            pendingLabel={t('admin.topics.creating')}
            categories={categories}
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
          <ConfirmFact label={t('admin.topics.fact.topic')}>{confirming.topic.title}</ConfirmFact>
          <ConfirmFact label={t('admin.topics.fact.category')}>{confirming.topic.category ?? '—'}</ConfirmFact>
        </ConfirmDialog>
      ) : null}

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
              initial={{ title: editing.title, description: editing.description, category: editing.category ?? '' }}
              submitLabel={t('common.save')}
              pendingLabel={t('admin.topics.saving')}
              categories={categories}
              onSubmit={(v) => edit(editing, v)}
              onCancel={() => setEditing(undefined)}
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </Page>
  )
}
