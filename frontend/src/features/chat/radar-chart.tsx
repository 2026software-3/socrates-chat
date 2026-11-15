import { useI18n, useT } from '@/i18n'
import type { Lang } from '@/i18n/define'
import type { LocalizedText, RadarView } from '@/lib/types'

const W = 440
const H = 320
const CX = W / 2
const CY = H / 2
const R = 100
/** S-04.6：0 到 5 的整數分。 */
const MAX = 5
/** 軸標籤放不下太長的名稱；完整名稱在圖下方的清單。 */
const AXIS_LABEL_CHARS = 14

const SLOT: Record<Lang, keyof LocalizedText> = { 'zh-TW': 'zh_tw', en: 'en', es: 'es' }

type Axis = { id: string; name: string; description: string; score: number }

/**
 * 把後端的結果整理成依題目維度順序的軸。分數缺漏、不是 0 到 5 的整數，或沒有維度時回傳 `null`：
 * 寧可顯示空狀態，也不畫出不完整的圖。
 */
function toAxes(radar: RadarView, lang: Lang): Axis[] | null {
  const { dimensions, scores } = radar
  if (!dimensions?.length || !scores) return null
  const axes: Axis[] = []
  for (const dimension of dimensions) {
    const score = scores[dimension.id]
    if (typeof score !== 'number' || !Number.isInteger(score) || score < 0 || score > MAX) return null
    axes.push({ id: dimension.id, name: dimension.name[SLOT[lang]], description: dimension.description[SLOT[lang]], score })
  }
  return axes
}

const shorten = (text: string) => (text.length > AXIS_LABEL_CHARS ? `${text.slice(0, AXIS_LABEL_CHARS - 1)}…` : text)

/** 單場雷達圖（純 SVG，沒有圖表函式庫，S-11.3）。軸的數量與名稱來自題目的維度。 */
function RadarChart({ axes, label }: { axes: Axis[]; label: string }) {
  /** 第 i 個軸的角度：從正上方開始順時針。 */
  const angle = (i: number) => -Math.PI / 2 + (i * 2 * Math.PI) / axes.length
  const point = (i: number, r: number) => [CX + r * Math.cos(angle(i)), CY + r * Math.sin(angle(i))] as const
  const polygon = (rs: number[]) => rs.map((r, i) => point(i, r).map((n) => n.toFixed(1)).join(',')).join(' ')
  const rings = Array.from({ length: MAX }, (_, k) => k + 1)

  return (
    <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={label} className="mx-auto h-auto w-full max-w-md" data-testid="radar-chart">
      {rings.map((ring) => (
        <polygon
          key={ring}
          points={polygon(axes.map(() => (R * ring) / MAX))}
          className="fill-none stroke-border"
          strokeWidth={ring === MAX ? 1.5 : 1}
        />
      ))}
      {axes.map((axis, i) => {
        const [x, y] = point(i, R)
        return <line key={axis.id} x1={CX} y1={CY} x2={x} y2={y} className="stroke-border" strokeWidth={1} />
      })}
      <polygon
        data-testid="radar-shape"
        points={polygon(axes.map((axis) => (R * axis.score) / MAX))}
        className="fill-primary/25 stroke-primary"
        strokeWidth={2}
        strokeLinejoin="round"
      />
      {axes.map((axis, i) => {
        const [x, y] = point(i, (R * axis.score) / MAX)
        return <circle key={axis.id} cx={x} cy={y} r={3.5} className="fill-primary" />
      })}
      {axes.map((axis, i) => {
        const [x, y] = point(i, R + 16)
        const c = Math.cos(angle(i))
        const anchor = Math.abs(c) < 0.2 ? 'middle' : c > 0 ? 'start' : 'end'
        return (
          <text key={axis.id} x={x} y={y} textAnchor={anchor} dominantBaseline="middle" className="fill-foreground" fontSize={13} aria-hidden="true">
            {shorten(axis.name)}
          </text>
        )
      })}
    </svg>
  )
}

/**
 * 已完成的單場雷達圖（F-24.2）：圖、各維度的分數，以及可展開的白話說明。
 * 圖只是輔助，同一份數字也以清單呈現給螢幕閱讀器與讀不清楚圖的人。
 */
export function RadarResult({ radar }: { radar: RadarView }) {
  const t = useT()
  const { lang } = useI18n()
  const axes = toAxes(radar, lang)

  if (!axes) {
    return (
      <p role="status" className="text-muted-foreground text-sm">
        {t('chat.radar.empty')}
      </p>
    )
  }

  return (
    <div className="space-y-3">
      <p className="text-muted-foreground text-sm">{t('chat.radar.note')}</p>
      <figure className="m-0 space-y-3">
        <RadarChart axes={axes} label={t('chat.radar.chartLabel', { max: MAX })} />
        <figcaption>
          <ul className="space-y-2">
            {axes.map((axis) => (
              <li key={axis.id}>
                <details className="rounded-lg border p-3">
                  <summary className="cursor-pointer text-sm font-medium break-words">
                    <span>{axis.name}</span>
                    <span className="text-muted-foreground ml-2 font-normal">
                      {t('chat.radar.score', { value: axis.score, max: MAX })}
                    </span>
                  </summary>
                  <p className="mt-2 text-sm break-words whitespace-pre-wrap">{axis.description}</p>
                </details>
              </li>
            ))}
          </ul>
        </figcaption>
      </figure>
      {axes.every((axis) => axis.score === 0) ? (
        <p role="status" className="text-muted-foreground text-sm">
          {t('chat.radar.allZero')}
        </p>
      ) : null}
    </div>
  )
}
