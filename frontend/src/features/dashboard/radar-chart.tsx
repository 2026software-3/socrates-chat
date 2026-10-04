import { frameworkLabelKey } from '@/features/dashboard/labels'
import { useI18n, useT } from '@/i18n'
import { SCHOOLS, type SchoolScores } from '@/lib/types'

const W = 440
const H = 320
const CX = W / 2
const CY = H / 2
const R = 100

/** 第 i 個軸的角度：從正上方開始順時針。 */
const angle = (i: number) => -Math.PI / 2 + (i * 2 * Math.PI) / SCHOOLS.length
const point = (i: number, r: number) => [CX + r * Math.cos(angle(i)), CY + r * Math.sin(angle(i))] as const
const polygon = (rs: number[]) => rs.map((r, i) => point(i, r).map((n) => n.toFixed(1)).join(',')).join(' ')

/**
 * 6 個學派維度的雷達圖（純 SVG，沒有圖表函式庫）。分數從 0 到 `max`。
 * 圖本身只是輔助：同一份數字也以清單形式提供給螢幕閱讀器與讀不清楚圖的人。
 */
export function RadarChart({ scores, max, label }: { scores: SchoolScores; max: number; label: string }) {
  const t = useT()
  const { lang } = useI18n()
  const num = new Intl.NumberFormat(lang, { maximumFractionDigits: 1 })
  const rings = Array.from({ length: max }, (_, k) => k + 1)
  const values = SCHOOLS.map((k) => Math.max(0, Math.min(max, scores[k] ?? 0)))

  return (
    <figure className="m-0 space-y-2">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={label} className="mx-auto h-auto w-full max-w-md" data-testid="radar-chart">
        {rings.map((ring) => (
          <polygon
            key={ring}
            points={polygon(SCHOOLS.map(() => (R * ring) / max))}
            className="fill-none stroke-border"
            strokeWidth={ring === max ? 1.5 : 1}
          />
        ))}
        {SCHOOLS.map((k, i) => {
          const [x, y] = point(i, R)
          return <line key={k} x1={CX} y1={CY} x2={x} y2={y} className="stroke-border" strokeWidth={1} />
        })}
        <polygon
          points={polygon(values.map((v) => (R * v) / max))}
          className="fill-primary/25 stroke-primary"
          strokeWidth={2}
          strokeLinejoin="round"
        />
        {values.map((v, i) => {
          const [x, y] = point(i, (R * v) / max)
          return <circle key={SCHOOLS[i]} cx={x} cy={y} r={3.5} className="fill-primary" />
        })}
        {SCHOOLS.map((k, i) => {
          const [x, y] = point(i, R + 16)
          const c = Math.cos(angle(i))
          const anchor = Math.abs(c) < 0.2 ? 'middle' : c > 0 ? 'start' : 'end'
          return (
            <text
              key={k}
              x={x}
              y={y}
              textAnchor={anchor}
              dominantBaseline="middle"
              className="fill-foreground"
              fontSize={13}
            >
              {t(frameworkLabelKey(k))}
            </text>
          )
        })}
      </svg>
      <figcaption className="sr-only">
        <ul>
          {SCHOOLS.map((k, i) => (
            <li key={k}>{t('dashboard.radar.value', { name: t(frameworkLabelKey(k)), value: num.format(values[i]), max })}</li>
          ))}
        </ul>
      </figcaption>
    </figure>
  )
}
