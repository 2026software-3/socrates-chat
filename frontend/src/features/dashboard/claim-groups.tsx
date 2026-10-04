import { useState } from 'react'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { RadarChart } from '@/features/dashboard/radar-chart'
import { errorText, useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { TopicDistribution } from '@/lib/types'

/**
 * 一個題目的「主張分布」：每列是一種主張（AI 把意思相近的歸成一群並命名），
 * 顯示人數與比例；展開可看這群人的核心理由（AI 總結的文字，沒有姓名與原文）與論證傾向。
 * 分組由 AI 完成，可能不夠穩定，所以教師可以按「重新分組」。
 */
export function ClaimGroups({ topic, onChanged, canRegroup }: { topic: TopicDistribution; onChanged: () => void; canRegroup: boolean }) {
  const t = useT()
  const { lang } = useI18n()
  const pct = new Intl.NumberFormat(lang, { style: 'percent', maximumFractionDigits: 0 })
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>()
  const total = topic.claims.reduce((n, g) => n + g.count, 0)

  async function regroup() {
    setPending(true)
    setError(undefined)
    try {
      await api.post('/api/dashboard/class/regroup', { title: topic.title })
      onChanged()
    } catch (e) {
      setError(e)
    } finally {
      setPending(false)
    }
  }

  return (
    <section className="space-y-3">
      <h4 className="text-sm font-medium">{t('dashboard.claims')}</h4>
      {topic.claims.length === 0 ? (
        <p className="text-muted-foreground rounded-md border border-dashed p-4 text-center text-sm">{t('dashboard.claims.empty')}</p>
      ) : (
        <ul aria-label={t('dashboard.claims')} className="space-y-2">
          {topic.claims.map((g) => {
            const share = total > 0 ? g.count / total : 0
            const name = g.other ? t('dashboard.claims.other') : (g.name ?? '')
            return (
              <li key={g.other ? '__other__' : g.name} data-testid="claim-group" className="rounded-md border">
                <details className="group">
                  <summary className="grid cursor-pointer list-none grid-cols-[minmax(5rem,9rem)_1fr_4.75rem] items-center gap-x-3 p-2.5 text-sm">
                    <span className="font-medium break-words">{name}</span>
                    <span className="bg-muted h-3 overflow-hidden rounded-full" aria-hidden>
                      <span className="bg-primary block h-full rounded-full" style={{ width: `${share * 100}%` }} />
                    </span>
                    <span className="text-right tabular-nums">
                      {g.count}
                      <span className="text-muted-foreground ml-1">({pct.format(share)})</span>
                    </span>
                  </summary>
                  <div className="space-y-4 border-t p-3">
                    {g.reasons.length > 0 ? (
                      <div className="space-y-1">
                        <p className="text-muted-foreground text-xs font-medium">{t('dashboard.claims.reasons')}</p>
                        <ul className="list-disc space-y-1 pl-5 text-sm">
                          {g.reasons.map((r) => (
                            <li key={r} className="break-words">
                              {r}
                            </li>
                          ))}
                        </ul>
                      </div>
                    ) : null}
                    {g.radar.scored > 0 ? (
                      <div className="space-y-1">
                        <p className="text-muted-foreground text-xs font-medium">{t('dashboard.claims.radar')}</p>
                        <RadarChart scores={g.radar.average} max={g.radar.max} label={`${name}：${t('dashboard.claims.radar')}`} />
                      </div>
                    ) : null}
                  </div>
                </details>
              </li>
            )
          })}
        </ul>
      )}
      {topic.ungrouped > 0 ? (
        <p role="status" className="text-muted-foreground text-xs">
          {t('dashboard.claims.ungrouped', { count: topic.ungrouped })}
        </p>
      ) : null}
      {canRegroup && (topic.claims.length > 0 || topic.ungrouped > 0) ? (
        <div className="space-y-2">
          <p className="text-muted-foreground text-xs">{t('dashboard.claims.regroupHelp')}</p>
          <Button variant="outline" size="sm" disabled={pending} onClick={() => void regroup()}>
            {pending ? t('dashboard.claims.regrouping') : t('dashboard.claims.regroup')}
          </Button>
          {error ? (
            <Alert variant="destructive" role="alert">
              <AlertDescription>{errorText(t, error)}</AlertDescription>
            </Alert>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}
