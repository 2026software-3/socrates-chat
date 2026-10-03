import { Pencil } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
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

/** 題目庫：新增、編輯、啟用／停用題目（只有管理者進得來）。 */
export function TopicsPage() {
  const t = useT()
  const { lang } = useI18n()
  const { data, error, loading, reload } = useFetch<Topic[]>('/api/admin/topics')

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
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState('')
  const [includeInactive, setIncludeInactive] = useState(true)

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
      (includeInactive || x.is_active) &&
      (!activeCategory || x.category === activeCategory) &&
      (!q || [x.title, x.description, x.category ?? ''].some((s) => s.toLowerCase().includes(q))),
  )
  const num = new Intl.NumberFormat(lang)
  const filtered = q !== '' || activeCategory !== '' || !includeInactive

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
    await api.post('/api/admin/topics', body)
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
      await api.patch(`/api/admin/topics/${encodeURIComponent(topic.id)}`, body)
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
      await api.patch(`/api/admin/topics/${encodeURIComponent(topic.id)}`, { is_active: next })
    } catch (err) {
      patch(topic.id, { is_active: !next }) // 失敗就還原
      setActionError(err)
    } finally {
      setToggling((s) => {
        const copy = new Set(s)
        copy.delete(topic.id)
        return copy
      })
    }
  }

  return (
    <Page title={t('admin.topics.title')}>
      <p className="text-muted-foreground text-sm">{t('admin.topics.intro')}</p>

      <section aria-labelledby="topic-create-heading" className="space-y-3 rounded-md border p-4">
        <h2 id="topic-create-heading" className="font-medium">
          {t('admin.topics.create')}
        </h2>
        <TopicForm
          idPrefix="create"
          initial={{ title: '', description: '', category: '' }}
          submitLabel={t('admin.topics.create')}
          pendingLabel={t('admin.topics.creating')}
          categories={categories}
          onSubmit={create}
        />
        <div aria-live="polite">
          {notice ? <p className="text-sm text-green-700 dark:text-green-400">{notice}</p> : null}
        </div>
      </section>

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
            <div className="flex items-center gap-2 pb-1.5">
              <Switch id="topic-include-inactive" checked={includeInactive} onCheckedChange={setIncludeInactive} />
              <Label htmlFor="topic-include-inactive">{t('admin.topics.includeInactive')}</Label>
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
            <ul aria-label={t('admin.topics.list')} className="divide-y rounded-md border">
              {shown.map((x) => (
                <li key={x.id} className={`flex items-start gap-3 px-3 py-3 ${x.is_active ? '' : 'bg-muted/50 text-muted-foreground'}`}>
                  <div className="min-w-0 flex-1 space-y-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium break-words">{x.title}</span>
                      {x.category ? <Badge variant="outline">{x.category}</Badge> : null}
                      {x.is_active ? null : <Badge variant="secondary">{t('admin.topics.inactive')}</Badge>}
                    </div>
                    <p className="text-sm break-words">{x.description || t('admin.topics.noDescription')}</p>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Switch
                      checked={x.is_active}
                      disabled={toggling.has(x.id)}
                      aria-label={t('admin.topics.activeToggle', { title: x.title })}
                      onCheckedChange={(next) => void toggle(x, next)}
                    />
                    <Button variant="ghost" size="icon" aria-label={t('admin.topics.edit', { title: x.title })} onClick={() => setEditing(x)}>
                      <Pencil aria-hidden />
                    </Button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
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
