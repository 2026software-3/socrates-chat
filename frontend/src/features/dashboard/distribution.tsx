import { RadarChart } from '@/features/dashboard/radar-chart'
import { useT } from '@/i18n'
import type { Radar } from '@/lib/types'

/** 論證傾向：6 個學派的平均分數雷達圖；沒有評分資料時說明原因，不畫空圖。 */
export function RadarPanel({ radar }: { radar: Radar }) {
  const t = useT()
  return (
    <section aria-labelledby="dist-frameworks" className="space-y-3">
      <h3 id="dist-frameworks" className="font-medium">
        {t('dashboard.frameworks')}
      </h3>
      <p className="text-muted-foreground text-xs">{t('dashboard.frameworksHelp')}</p>
      {radar.scored > 0 ? (
        <>
          <RadarChart scores={radar.average} max={radar.max} label={t('dashboard.frameworks')} />
          <p className="text-muted-foreground text-center text-xs">{t('dashboard.radar.basedOn', { count: radar.scored })}</p>
        </>
      ) : (
        <p className="text-muted-foreground rounded-md border border-dashed p-4 text-center text-sm">{t('dashboard.radar.empty')}</p>
      )}
    </section>
  )
}

/** 數字摘要磚。 */
export function Stat({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="rounded-lg border p-2.5 sm:p-4">
      <p className="text-muted-foreground text-xs sm:text-sm">{label}</p>
      <p className="text-2xl font-semibold tabular-nums sm:text-3xl">{value}</p>
    </div>
  )
}
