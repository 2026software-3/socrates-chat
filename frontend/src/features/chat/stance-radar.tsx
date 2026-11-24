import { useI18n, useT } from '@/i18n'
import type { RadarView } from '@/lib/types'

const labels = { 'zh-TW': 'zh_tw', en: 'en', es: 'es' } as const
const levelKeys = ['chat.stance.negative2', 'chat.stance.negative1', 'chat.stance.zero', 'chat.stance.positive1', 'chat.stance.positive2'] as const

/** Signed positions use an offset radius: -2 at the centre, 0 halfway, +2 outside.
 * Unknown vertices have no point or connecting edge, never a fabricated zero. */
export function StanceRadar({ radar }: { radar: RadarView }) {
  const t = useT()
  const { lang } = useI18n()
  const dimensions = radar.dimensions
  const scores = radar.scores
  if (!dimensions?.length || !scores || Object.entries(scores).some(([id, score]) =>
    !dimensions.some((d) => d.id === id) || !Number.isInteger(score) || score < -2 || score > 2)) {
    return <p role="status">{t('chat.radar.empty')}</p>
  }
  const point = (i: number, radius: number) => {
    const angle = -Math.PI / 2 + (i * 2 * Math.PI) / dimensions.length
    return [220 + radius * Math.cos(angle), 160 + radius * Math.sin(angle)] as const
  }
  const polygon = (radii: number[]) => radii.map((radius, i) => point(i, radius).map((v) => v.toFixed(1)).join(',')).join(' ')
  const radius = (score: number) => (100 * (score + 2)) / 4
  const complete = dimensions.every((d) => scores[d.id] !== undefined)
  return (
    <div className="space-y-3">
      <p className="text-muted-foreground text-sm">{t('chat.stance.note')}</p>
      <figure className="m-0 space-y-3">
        <svg viewBox="0 0 440 320" role="img" aria-label={t('chat.stance.chartLabel')} className="mx-auto h-auto w-full max-w-md" data-testid="radar-chart">
          {[-1, 0, 1, 2].map((level) => <polygon key={level} points={polygon(dimensions.map(() => radius(level)))} className="fill-none stroke-border" strokeWidth={level === 0 ? 2 : 1} />)}
          {dimensions.map((d, i) => {
            const [x, y] = point(i, 100)
            return <line key={d.id} x1={220} y1={160} x2={x} y2={y} className="stroke-border" />
          })}
          {complete ? <polygon data-testid="radar-shape" points={polygon(dimensions.map((d) => radius(scores[d.id])))} className="fill-primary/25 stroke-primary" strokeWidth={2} /> : dimensions.map((d, i) => {
            const next = (i + 1) % dimensions.length
            const a = scores[d.id]
            const b = scores[dimensions[next].id]
            if (a === undefined || b === undefined) return null
            const [x1, y1] = point(i, radius(a))
            const [x2, y2] = point(next, radius(b))
            return <line key={d.id} x1={x1} y1={y1} x2={x2} y2={y2} className="stroke-primary" strokeWidth={2} />
          })}
          {dimensions.map((d, i) => {
            const score = scores[d.id]
            if (score === undefined) return null
            const [x, y] = point(i, radius(score))
            return <circle key={d.id} data-testid="stance-point" cx={x} cy={y} r={3.5} className="fill-primary" />
          })}
          {[-2, -1, 0, 1, 2].map((level) => <text key={level} x={224} y={160 - radius(level)} className="fill-foreground" fontSize={10} aria-hidden="true">{level}</text>)}
          {dimensions.map((d, i) => {
            const [x, y] = point(i, 116)
            const name = d.name[labels[lang]]
            const direction = Math.cos(-Math.PI / 2 + (i * 2 * Math.PI) / dimensions.length)
            const anchor = Math.abs(direction) < 0.2 ? 'middle' : direction > 0 ? 'start' : 'end'
            const glyphs = Array.from(name)
            const label = glyphs.length > 8 ? `${glyphs.slice(0, 7).join('')}…` : name
            return <text key={d.id} x={x} y={y} textAnchor={anchor} dominantBaseline="middle" className="fill-foreground" fontSize={13} aria-hidden="true">{label}</text>
          })}
        </svg>
        <figcaption>
          <ul className="space-y-2">
            {dimensions.map((d) => {
              const score = scores[d.id]
              return (
                <li key={d.id}>
                  <details className="rounded-lg border p-3">
                    <summary className="cursor-pointer text-sm font-medium break-words">
                      {d.name[labels[lang]]}{' '}
                      <span className="text-muted-foreground font-normal">{score === undefined ? t('chat.stance.unknown') : t('chat.stance.value', { value: score, label: t(levelKeys[score + 2]) })}</span>
                    </summary>
                    <p className="mt-2 text-sm break-words whitespace-pre-wrap">{d.description[labels[lang]]}</p>
                  </details>
                </li>
              )
            })}
          </ul>
        </figcaption>
      </figure>
    </div>
  )
}