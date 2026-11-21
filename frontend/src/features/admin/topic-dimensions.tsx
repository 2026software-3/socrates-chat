import { useState, type FormEvent } from 'react'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { errorText, useI18n, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

const languages = ['zh-TW', 'en', 'es'] as const
type Language = (typeof languages)[number]
type Localized = { zh_tw: string; en: string; es: string }
type Dimension = { id: string; name: Localized; description: Localized; rubric: string[] }
type Dimensions = {
  dimensions: Dimension[] | null
  locked: boolean
  prompt_version: string | null
  rubric_language: Language | null
  pending_languages: Language[]
}
const keys = { 'zh-TW': 'zh_tw', en: 'en', es: 'es' } as const
const labels = { 'zh-TW': '繁體中文', en: 'English', es: 'Español' }

export function TopicDimensionsDialog({ topic, close }: { topic: Topic; close: () => void }) {
  const t = useT()
  const { lang } = useI18n()
  const path = `/api/topics/${encodeURIComponent(topic.id)}/dimensions`
  const { data, loading, error, reload } = useFetch<Dimensions>(path)
  const [language, setLanguage] = useState<Language>(lang)
  const [latest, setLatest] = useState<Dimensions>()
  const [pending, setPending] = useState(false)
  const [actionError, setActionError] = useState<unknown>()
  const view = latest ?? data

  async function retry() {
    setPending(true)
    setActionError(undefined)
    try {
      setLatest(await api.post<Dimensions>(`${path}/retry`))
    } catch (err) {
      setActionError(err)
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && !pending && close()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl" showCloseButton={!pending}>
        <DialogHeader>
          <DialogTitle>{t('admin.dimensions.title', { title: topic.title })}</DialogTitle>
          <DialogDescription>{t('admin.dimensions.intro')}</DialogDescription>
        </DialogHeader>
        {loading && !view ? <LoadingBlock /> : null}
        {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
        {view && !view.dimensions ? (
          <div className="space-y-3">
            <p role="status">{t('admin.dimensions.pending')}</p>
            <Button disabled={pending} onClick={() => void retry()}>
              {t(pending ? 'admin.dimensions.generating' : 'admin.dimensions.retry')}
            </Button>
          </div>
        ) : null}
        {view?.dimensions ? (
          <>
            <div className="flex items-center gap-2">
              <Label htmlFor="dimension-language">{t('admin.dimensions.language')}</Label>
              <select
                id="dimension-language"
                className="border-input bg-background rounded-lg border px-2 py-1"
                value={language}
                disabled={pending}
                onChange={(event) => setLanguage(event.target.value as Language)}
              >
                {languages.map((choice) => <option key={choice} value={choice}>{labels[choice]}</option>)}
              </select>
            </div>
            {view.locked ? <p className="text-sm">{t('admin.dimensions.locked')}</p> : null}
            {view.pending_languages.includes(language) ? (
              <p className="text-sm">{t('admin.dimensions.translationPending')}</p>
            ) : null}
            <DimensionsEditor
              key={`${language}:${view.locked}`}
              language={language}
              dimensions={view.dimensions}
              rubricEditable={!view.locked && view.rubric_language === language}
              pending={pending}
              onSave={async (dimensions) => {
                setPending(true)
                setActionError(undefined)
                try {
                  setLatest(await api.patch<Dimensions>(path, { language, dimensions }))
                  return true
                } catch (err) {
                  setActionError(err)
                  if (isApiError(err) && err.code === 'dimensions_locked') {
                    setLatest(undefined)
                    reload()
                  }
                  return false
                } finally {
                  setPending(false)
                }
              }}
            />
          </>
        ) : null}
        {actionError ? (
          <Alert role="alert" variant="destructive"><AlertTitle>{errorText(t, actionError)}</AlertTitle></Alert>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

type Edit = { id: string; name: string; description: string; rubric?: string[] }

function DimensionsEditor({ language, dimensions, rubricEditable, pending, onSave }: {
  language: Language
  dimensions: Dimension[]
  rubricEditable: boolean
  pending: boolean
  onSave: (dimensions: Edit[]) => Promise<boolean>
}) {
  const t = useT()
  const [drafts, setDrafts] = useState(() => dimensions.map((dimension) => ({
    id: dimension.id,
    name: dimension.name[keys[language]],
    description: dimension.description[keys[language]],
    rubric: dimension.rubric,
  })))
  const [saved, setSaved] = useState(false)
  const valid = drafts.every((draft) => draft.name.trim() && draft.description.trim() &&
    (!rubricEditable || draft.rubric.every((value) => value.trim())))

  function change(id: string, patch: Partial<(typeof drafts)[number]>) {
    setSaved(false)
    setDrafts((current) => current.map((draft) => draft.id === id ? { ...draft, ...patch } : draft))
  }

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (pending || !valid) return
    const body = drafts.map(({ id, name, description, rubric }) => ({
      id, name, description, ...(rubricEditable ? { rubric } : {}),
    }))
    setSaved(await onSave(body))
  }

  return (
    <form className="space-y-4" onSubmit={(event) => void submit(event)}>
      {drafts.map((draft) => (
        <fieldset key={draft.id} className="space-y-2 rounded-md border p-3" disabled={pending}>
          <legend className="px-1 font-medium">{draft.id}</legend>
          <Label htmlFor={`dimension-name-${draft.id}`}>{t('admin.dimensions.name')}</Label>
          <Input id={`dimension-name-${draft.id}`} required maxLength={100} value={draft.name}
            onChange={(event) => change(draft.id, { name: event.target.value })} />
          <Label htmlFor={`dimension-description-${draft.id}`}>{t('admin.dimensions.description')}</Label>
          <Textarea id={`dimension-description-${draft.id}`} required maxLength={500} value={draft.description}
            onChange={(event) => change(draft.id, { description: event.target.value })} />
          <details className="space-y-2">
            <summary className="cursor-pointer text-sm">{t('admin.dimensions.rubric')}</summary>
            {rubricEditable ? null : <p className="text-sm">{t('admin.dimensions.rubricSourceOnly')}</p>}
            {draft.rubric.map((text, index) => {
              const id = `dimension-rubric-${draft.id}-${index}`
              return (
                <div key={id} className="space-y-1">
                  <Label htmlFor={id}>{t('admin.dimensions.score', { score: index + 1 })}</Label>
                  <Textarea id={id} readOnly={!rubricEditable} maxLength={500} rows={2} value={text}
                    onChange={(event) => change(draft.id, { rubric: draft.rubric.map((value, i) =>
                      i === index ? event.target.value : value) })} />
                </div>
              )
            })}
          </details>
        </fieldset>
      ))}
      <Button type="submit" disabled={!valid || pending}>{t(pending ? 'admin.topics.saving' : 'common.save')}</Button>
      {saved ? <p role="status">{t('admin.dimensions.saved')}</p> : null}
    </form>
  )
}
