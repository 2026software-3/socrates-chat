import { useMemo, useState } from 'react'
import { nativeSelectClass } from '@/components/native-select-class'
import { ErrorBlock, LoadingBlock } from '@/components/page'
import { Label } from '@/components/ui/label'
import { useI18n, type MessageKey } from '@/i18n'
import type { AiProvider, UsageNumbers, UsageRange, UsageReport } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

const RANGES: UsageRange[] = ['today', '7d', '30d', 'all']

/**
 * 教師 key 或教室的 AI 用量（F-40.8，S-03.1）：依（供應商、模型）細分的請求次數與 token。
 * 不顯示金額；供應商沒回報 token 的請求只標示「供應商未回報」，不顯示估算值。
 */
export function UsagePanel({ path, providers, idPrefix }: { path: string; providers?: AiProvider[]; idPrefix: string }) {
  const { t, lang } = useI18n()
  const [range, setRange] = useState<UsageRange>('7d')
  const { data, error, loading, reload } = useFetch<UsageReport>(`${path}?range=${range}`)
  const num = useMemo(() => new Intl.NumberFormat(lang), [lang])
  const names = useMemo(() => new Map((providers ?? []).map((p) => [p.id, p.name])), [providers])
  const selectId = `${idPrefix}-range`

  return (
    <div className="space-y-3 rounded-md border p-3">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <h3 className="text-sm font-medium">{t('usage.title')}</h3>
        <div className="w-44 space-y-1">
          <Label htmlFor={selectId} className="text-xs">
            {t('usage.range')}
          </Label>
          <select id={selectId} className={nativeSelectClass} value={range} onChange={(e) => setRange(e.target.value as UsageRange)}>
            {RANGES.map((r) => (
              <option key={r} value={r}>
                {t(`usage.range.${r}` as MessageKey)}
              </option>
            ))}
          </select>
        </div>
      </div>
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {!data && loading ? <LoadingBlock /> : null}
      {data ? (
        data.totals.requests === 0 ? (
          <p className="text-muted-foreground text-sm">{t('usage.empty')}</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-muted-foreground border-b text-left">
                  <th scope="col" className="py-1 pr-3 font-medium">
                    {t('usage.col.model')}
                  </th>
                  <th scope="col" className="px-3 py-1 text-right font-medium">
                    {t('usage.col.requests')}
                  </th>
                  <th scope="col" className="px-3 py-1 text-right font-medium">
                    {t('usage.col.input')}
                  </th>
                  <th scope="col" className="py-1 pl-3 text-right font-medium">
                    {t('usage.col.output')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {data.items.map((item) => (
                  <UsageRow
                    key={`${item.provider}/${item.model}`}
                    label={`${names.get(item.provider) ?? item.provider} / ${item.model}`}
                    numbers={item}
                    num={num}
                  />
                ))}
                <UsageRow label={t('usage.total')} numbers={data.totals} num={num} strong />
              </tbody>
            </table>
          </div>
        )
      ) : null}
      <p className="text-muted-foreground text-xs">{t('usage.note')}</p>
    </div>
  )
}

function UsageRow({ label, numbers, num, strong }: { label: string; numbers: UsageNumbers; num: Intl.NumberFormat; strong?: boolean }) {
  const { t } = useI18n()
  // 全部請求都沒回報 token：不顯示 0，標示供應商未回報；部分回報時數字是已回報部分的加總
  const unreportedAll = numbers.requests > 0 && numbers.unreported_requests >= numbers.requests
  const tokenCell = (value: number) => (unreportedAll ? <span className="text-muted-foreground">{t('usage.unreported')}</span> : num.format(value))
  return (
    <tr className={strong ? 'border-t font-medium' : 'border-b last:border-b-0'}>
      <th scope="row" className="py-1 pr-3 text-left font-normal">
        {label}
        {!unreportedAll && numbers.unreported_requests > 0 ? (
          <span className="text-muted-foreground block text-xs">{t('usage.unreportedCount', { count: num.format(numbers.unreported_requests) })}</span>
        ) : null}
      </th>
      <td className="px-3 py-1 text-right tabular-nums">{num.format(numbers.requests)}</td>
      <td className="px-3 py-1 text-right tabular-nums">{tokenCell(numbers.input_tokens)}</td>
      <td className="py-1 pl-3 text-right tabular-nums">{tokenCell(numbers.output_tokens)}</td>
    </tr>
  )
}
