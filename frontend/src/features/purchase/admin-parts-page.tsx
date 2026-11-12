import { useState, type FormEvent } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useI18n } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import { partName, type Part } from './part'

function PartForm({ part, reload }: { part: Part; reload: () => void }) {
  const { t, lang } = useI18n()
  const [price, setPrice] = useState(String(part.price))
  const [enabled, setEnabled] = useState(part.enabled)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>()
  const [saved, setSaved] = useState(false)
  async function save(event: FormEvent) {
    event.preventDefault()
    if (busy) return
    setBusy(true); setError(undefined); setSaved(false)
    try { await api.patch(`/api/admin/parts/${encodeURIComponent(part.id)}`, { price: Number(price), enabled }); setSaved(true); reload() }
    catch (err) { setError(err) } finally { setBusy(false) }
  }
  return <form onSubmit={(event) => void save(event)} className="space-y-3 rounded-md border p-4">
    <h2 className="font-medium">{partName(part, lang)}</h2>
    <Label htmlFor={`${part.id}-price`}>{t('parts.priceLabel')}</Label>
    <Input id={`${part.id}-price`} type="number" min="0" step="1" required value={price} onChange={(e) => setPrice(e.target.value)} />
    <label className="flex items-center gap-2"><input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />{t('parts.enabled')}</label>
    <Button disabled={busy}>{t('common.save')}</Button>
    {saved ? <p role="status">{t('parts.saved')}</p> : null}
    {error ? <ErrorBlock error={error} onRetry={() => setError(undefined)} /> : null}
  </form>
}

export function AdminPartsPage() {
  const { t } = useI18n()
  const { data, loading, error, reload } = useFetch<Part[]>('/api/admin/parts')
  return <Page title={t('parts.admin')}>
    {loading && !data ? <LoadingBlock /> : null}
    {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
    {data?.length === 0 ? <EmptyBlock>{t('parts.adminEmpty')}</EmptyBlock> : null}
    {data?.map((part) => <PartForm key={part.id} part={part} reload={reload} />)}
  </Page>
}
