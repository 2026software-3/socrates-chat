import { useState } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Button } from '@/components/ui/button'
import { useI18n } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'

import { partName, type PartView } from './part'

export function PartsPage() {
  const { t, lang } = useI18n()
  const { data, loading, error, reload } = useFetch<PartView[]>('/api/parts')
  const balance = useFetch<{ balance: number }>('/api/points')
  const [busy, setBusy] = useState<string>()
  const [purchaseError, setPurchaseError] = useState<unknown>()
  async function buy(part: PartView) {
    if (busy) return
    setBusy(part.id); setPurchaseError(undefined)
    try {
      await api.post(`/api/parts/${encodeURIComponent(part.id)}/purchase`)
      reload(); balance.reload()
    } catch (err) {
      setPurchaseError(err); reload(); balance.reload()
    } finally { setBusy(undefined) }
  }
  return <Page title={t('parts.title')}>
    {balance.data ? <p>{t('parts.balance', { points: balance.data.balance })}</p> : null}
    {balance.error ? <ErrorBlock error={balance.error} onRetry={balance.reload} /> : null}
    {loading && !data ? <LoadingBlock /> : null}
    {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
    {purchaseError ? <ErrorBlock error={purchaseError} onRetry={() => setPurchaseError(undefined)} /> : null}
    {data?.length === 0 ? <EmptyBlock>{t('parts.empty')}</EmptyBlock> : null}
    {data?.map((part) => <section key={part.id} className="space-y-2 rounded-md border p-4">
      <h2 className="font-medium">{partName(part, lang)}</h2>
      <p>{t('parts.price', { points: part.price })}</p>
      {part.owned ? <span>{t('parts.owned')}</span> : <Button disabled={Boolean(busy) || !part.enabled} onClick={() => void buy(part)}>{busy === part.id ? t('parts.buying') : t('parts.buy')}</Button>}
      {!part.enabled ? <p>{t('parts.disabled')}</p> : null}
    </section>)}
  </Page>
}
