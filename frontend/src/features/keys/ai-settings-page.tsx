import { ArrowDown, ArrowUp, Plus, X } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { Link, useParams } from 'react-router'
import { nativeSelectClass } from '@/components/native-select-class'
import { ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { UsagePanel } from '@/features/keys/usage-panel'
import { errorText, useT } from '@/i18n'
import { api, isApiError } from '@/lib/api'
import type { AiProvider, Classroom, ClassroomAiSettings, ModelList, ModelRef, TeacherKey } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

const MAX_FALLBACKS = 3
const EMPTY: ModelRef = { provider: '', model: '' }

/** 教室的 AI 模型與備援設定（F-40.4，S-03.1）：只有教室建立者（教師）進得來。 */
export function ClassroomAiSettingsPage() {
  const t = useT()
  const { classroomId = '' } = useParams()
  const id = encodeURIComponent(classroomId)
  const classrooms = useFetch<Classroom[]>('/api/classrooms')
  const settings = useFetch<ClassroomAiSettings>(`/api/classrooms/${id}/ai-settings`)
  const providers = useFetch<AiProvider[]>('/api/teacher/ai-providers')
  const keys = useFetch<TeacherKey[]>('/api/teacher/keys')
  const classroom = classrooms.data?.find((c) => c.id === classroomId)
  const back = `/teacher/classrooms/${id}`
  const title = t('aiSettings.title')

  const failed = settings.error ?? providers.error ?? keys.error ?? classrooms.error
  const [settingsData, providersData, keysData] = [settings.data, providers.data, keys.data]
  const ready = settingsData && providersData && keysData && classrooms.data
  return (
    <Page title={title} description={classroom?.name} crumbs={[{ label: t('aiSettings.back'), to: back }, { label: title }]}>
      {failed ? (
        <ErrorBlock
          error={failed}
          onRetry={() => {
            settings.reload()
            providers.reload()
            keys.reload()
            classrooms.reload()
          }}
        />
      ) : !ready ? (
        <LoadingBlock />
      ) : (
        <>
          <p className="text-muted-foreground text-sm">{t('aiSettings.intro')}</p>
          <SettingsForm
            classroomId={classroomId}
            settings={settingsData}
            providers={providersData}
            keys={keysData}
            archived={classroom?.is_archived ?? false}
            onSaved={settings.reload}
          />
          <section className="space-y-2" aria-label={t('aiSettings.usageTitle')}>
            <h2 className="text-base font-medium">{t('aiSettings.usageTitle')}</h2>
            <UsagePanel idPrefix="classroom" path={`/api/classrooms/${id}/ai-usage`} providers={providersData} />
          </section>
        </>
      )}
    </Page>
  )
}

function whereLabel(t: ReturnType<typeof useT>, target: string | undefined): string | undefined {
  if (target === 'primary') return t('aiSettings.primary')
  const m = /^fallback_(\d)$/.exec(target ?? '')
  return m ? t('aiSettings.fallbackN', { n: m[1] }) : undefined
}

function SettingsForm({
  classroomId,
  settings,
  providers,
  keys,
  archived,
  onSaved,
}: {
  classroomId: string
  settings: ClassroomAiSettings
  providers: AiProvider[]
  keys: TeacherKey[]
  archived: boolean
  onSaved: () => void
}) {
  const t = useT()
  const [primary, setPrimary] = useState<ModelRef>(settings.primary ?? EMPTY)
  const [fallbacks, setFallbacks] = useState<ModelRef[]>(settings.fallbacks)
  const [confirmed, setConfirmed] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<unknown>()
  const [saved, setSaved] = useState(false)

  const names = useMemo(() => new Map(providers.map((p) => [p.id, p.name])), [providers])
  const nameOf = (id: string) => names.get(id) ?? id
  const haveKey = useMemo(() => new Set(keys.map((k) => k.provider)), [keys])

  const chosen = [primary, ...fallbacks]
  const filled = chosen.every((m) => m.provider && m.model.trim())
  const missingKey = [...new Set(chosen.map((m) => m.provider).filter((p) => p && !haveKey.has(p)))]
  // 第一次為教室選定某家供應商（含備援）時要確認學生的對話內容會送往它
  const needConfirm = [...new Set(chosen.map((m) => m.provider).filter((p) => p && !settings.confirmed_providers.includes(p)))]
  const canSave = !archived && !saving && filled && missingKey.length === 0 && (needConfirm.length === 0 || confirmed)

  const update = (i: number, patch: Partial<ModelRef>) => setFallbacks((f) => f.map((m, j) => (j === i ? { ...m, ...patch } : m)))
  const move = (i: number, d: -1 | 1) =>
    setFallbacks((f) => {
      const next = [...f]
      ;[next[i], next[i + d]] = [next[i + d], next[i]]
      return next
    })

  async function onSubmit(e: FormEvent) {
    e.preventDefault()
    if (!canSave) return
    setSaving(true)
    setError(undefined)
    setSaved(false)
    try {
      await api.put(`/api/classrooms/${encodeURIComponent(classroomId)}/ai-settings`, {
        provider: primary.provider,
        model: primary.model.trim(),
        fallbacks: fallbacks.map((m) => ({ provider: m.provider, model: m.model.trim() })),
        data_flow_confirmed: needConfirm.length === 0 || confirmed,
      })
      setSaved(true)
      setConfirmed(false)
      onSaved()
    } catch (err) {
      setError(err)
    } finally {
      setSaving(false)
    }
  }

  const errorWhere = isApiError(error) ? whereLabel(t, error.target) : undefined

  return (
    <form onSubmit={onSubmit} className="space-y-5" noValidate>
      {settings.transitional ? (
        <Alert role="status">
          <AlertDescription className="text-foreground">{t('aiSettings.transitional')}</AlertDescription>
        </Alert>
      ) : null}
      {archived ? (
        <Alert role="status">
          <AlertDescription className="text-foreground">{t('aiSettings.archived')}</AlertDescription>
        </Alert>
      ) : null}

      <ModelFields
        legend={t('aiSettings.primary')}
        idPrefix="primary"
        value={primary}
        providers={providers}
        disabled={archived}
        onChange={(patch) => setPrimary((p) => ({ ...p, ...patch }))}
      />

      <section className="space-y-3" aria-label={t('aiSettings.fallbacks')}>
        <div>
          <h2 className="text-base font-medium">{t('aiSettings.fallbacks')}</h2>
          <p className="text-muted-foreground text-xs">{t('aiSettings.fallbackHelp')}</p>
        </div>
        {fallbacks.map((m, i) => (
          <div key={i} className="space-y-2 rounded-md border p-3">
            <div className="flex items-center justify-between gap-2">
              <h3 className="text-sm font-medium">{t('aiSettings.fallbackN', { n: i + 1 })}</h3>
              <div className="flex gap-1">
                <Button type="button" variant="ghost" size="icon-sm" aria-label={t('aiSettings.moveUp', { n: i + 1 })} disabled={archived || i === 0} onClick={() => move(i, -1)}>
                  <ArrowUp aria-hidden />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('aiSettings.moveDown', { n: i + 1 })}
                  disabled={archived || i === fallbacks.length - 1}
                  onClick={() => move(i, 1)}
                >
                  <ArrowDown aria-hidden />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('aiSettings.removeFallback', { n: i + 1 })}
                  disabled={archived}
                  onClick={() => setFallbacks((f) => f.filter((_, j) => j !== i))}
                >
                  <X aria-hidden />
                </Button>
              </div>
            </div>
            <ModelFields
              idPrefix={`fallback-${i + 1}`}
              value={m}
              providers={providers}
              disabled={archived}
              onChange={(patch) => update(i, patch)}
            />
          </div>
        ))}
        {fallbacks.length < MAX_FALLBACKS ? (
          <Button type="button" variant="outline" size="sm" disabled={archived} onClick={() => setFallbacks((f) => [...f, EMPTY])}>
            <Plus aria-hidden />
            {t('aiSettings.addFallback')}
          </Button>
        ) : null}
      </section>

      <p className="text-muted-foreground text-xs">{t('aiSettings.disclaimer')}</p>

      {missingKey.map((p) => (
        <Alert key={p} role="alert" variant="destructive">
          <AlertTitle>{t('aiSettings.noKey', { provider: nameOf(p) })}</AlertTitle>
          <AlertDescription>
            <Link to="/teacher/keys" className="underline">
              {t('aiSettings.goToKeys')}
            </Link>
          </AlertDescription>
        </Alert>
      ))}

      {needConfirm.length > 0 ? (
        <label className="flex items-start gap-2 text-sm">
          <input type="checkbox" className="mt-1" checked={confirmed} disabled={archived} onChange={(e) => setConfirmed(e.target.checked)} />
          <span>{t('aiSettings.confirm', { providers: needConfirm.map(nameOf).join('、') })}</span>
        </label>
      ) : settings.confirmed_providers.length > 0 ? (
        <p className="text-muted-foreground text-xs">{t('aiSettings.confirmed', { providers: settings.confirmed_providers.map(nameOf).join('、') })}</p>
      ) : null}

      <p className="text-muted-foreground text-xs">{t('aiSettings.verifyNotice')}</p>
      <div aria-live="polite" className="space-y-2">
        {error ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>
              {errorWhere ? t('aiSettings.errorAt', { where: errorWhere, message: errorText(t, error) }) : errorText(t, error)}
            </AlertTitle>
          </Alert>
        ) : null}
        {saved ? (
          <p role="status" className="text-sm text-green-700 dark:text-green-400">
            {t('aiSettings.saved')}
          </p>
        ) : null}
      </div>
      <Button type="submit" disabled={!canSave}>
        {saving ? t('aiSettings.saving') : t('aiSettings.save')}
      </Button>
    </form>
  )
}

function ModelFields({
  legend,
  idPrefix,
  value,
  providers,
  disabled,
  onChange,
}: {
  legend?: string
  idPrefix: string
  value: ModelRef
  providers: AiProvider[]
  disabled: boolean
  onChange: (patch: Partial<ModelRef>) => void
}) {
  const t = useT()
  // 模型清單只是輸入建議：取不到（沒有 key、供應商不提供、逾時）就是純文字輸入，儲存時仍由後端驗證
  const list = useFetch<ModelList>(
    value.provider ? `/api/teacher/keys/models?provider=${encodeURIComponent(value.provider)}` : null,
  )
  const suggestions = value.provider && list.data?.provider === value.provider && list.data.status === 'ok' ? list.data.models : []
  const listUnavailable = Boolean(value.provider) && list.data?.provider === value.provider && list.data.status === 'unavailable'
  const listId = `${idPrefix}-model-list`
  const noteId = `${idPrefix}-model-note`
  return (
    <fieldset className="space-y-3" disabled={disabled}>
      {legend ? <legend className="text-base font-medium">{legend}</legend> : null}
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor={`${idPrefix}-provider`}>{t('aiSettings.provider')}</Label>
          <select id={`${idPrefix}-provider`} className={nativeSelectClass} value={value.provider} onChange={(e) => onChange({ provider: e.target.value })}>
            <option value="">{t('aiSettings.providerPlaceholder')}</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`${idPrefix}-model`}>{t('aiSettings.model')}</Label>
          <Input
            id={`${idPrefix}-model`}
            autoComplete="off"
            spellCheck={false}
            maxLength={200}
            value={value.model}
            placeholder={t('aiSettings.modelPlaceholder')}
            list={suggestions.length > 0 ? listId : undefined}
            aria-describedby={suggestions.length > 0 || listUnavailable ? noteId : undefined}
            onChange={(e) => onChange({ model: e.target.value })}
          />
          {suggestions.length > 0 ? (
            <datalist id={listId}>
              {suggestions.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          ) : null}
          {suggestions.length > 0 ? (
            <p id={noteId} className="text-muted-foreground text-xs">
              {t('aiSettings.modelListHint')}
            </p>
          ) : listUnavailable ? (
            <p id={noteId} className="text-muted-foreground text-xs">
              {t('aiSettings.modelListUnavailable')}
            </p>
          ) : null}
        </div>
      </div>
    </fieldset>
  )
}

