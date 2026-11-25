import type { ReactNode } from 'react'
import { cn } from 'cn'
import { stackBarProportions } from './scales'
import { Skeleton } from './skeleton'

export type StackBarTone = 'caeruleum' | 'ochra' | 'aerugo' | 'neutral' | 'track'

export type StackBarSegment = {
  label: string
  value: number
  valueLabel?: ReactNode
  tone?: StackBarTone
}

/** DESIGN.md The No Right-or-Wrong Rule：龐貝藍 → 赭金 → 銅綠 → 淡墨未表態 */
const DEFAULT_TONES: readonly StackBarTone[] = ['caeruleum', 'ochra', 'aerugo', 'neutral']

function toneClass(tone: StackBarTone, onPaper: boolean): string {
  switch (tone) {
    case 'caeruleum':
      return 'bg-caeruleum'
    case 'ochra':
      return onPaper ? 'bg-ochra-graphic-deep' : 'bg-ochra-graphic'
    case 'aerugo':
      return 'bg-aerugo'
    case 'neutral':
      return 'bg-ink-faint'
    case 'track':
      return 'bg-caeruleum-wash'
  }
}

export type StackBarProps = {
  segments: readonly StackBarSegment[]
  size?: 'md' | 'sm'
  showLegend?: boolean
  onPaper?: boolean
  emptyLabel?: string
  ariaLabel?: string
  className?: string
}

export function StackBar({
  segments,
  size = 'md',
  showLegend = true,
  onPaper = false,
  emptyLabel,
  ariaLabel,
  className,
}: StackBarProps) {
  const proportions = stackBarProportions(segments.map((s) => s.value))
  const hasData = proportions.some((p) => p > 0)
  const barHeight = size === 'sm' ? 'h-2' : 'h-2.5'
  const emptyTrack = size === 'sm' ? 'bg-caeruleum-wash' : 'bg-paper-deep'

  const resolved = segments.map((segment, index) => ({
    ...segment,
    tone: segment.tone ?? DEFAULT_TONES[index] ?? 'neutral',
    percent: proportions[index] ?? 0,
  }))

  return (
    <div
      data-vedute-stack-bar=""
      data-size={size}
      role={showLegend ? 'group' : 'img'}
      aria-label={ariaLabel}
      className={cn('flex w-full flex-col gap-2.5', className)}
    >
      <div
        aria-hidden="true"
        className={cn('flex w-full overflow-hidden', barHeight, !hasData && emptyTrack)}
      >
        {hasData &&
          resolved.map((segment, index) =>
            segment.percent > 0 ? (
              <div
                key={`${segment.label}-${index}`}
                data-vedute-segment=""
                data-tone={segment.tone}
                style={{ width: `${segment.percent}%` }}
                className={cn('h-full shrink-0', toneClass(segment.tone, onPaper))}
              />
            ) : null,
          )}
      </div>

      {!hasData && emptyLabel && (
        <p className="type-note text-ink-soft">{emptyLabel}</p>
      )}

      {showLegend && resolved.length > 0 && (
        <ul className="m-0 flex list-none flex-wrap items-center gap-x-3.5 gap-y-1.5 p-0">
          {resolved.map((segment, index) => (
            <li
              key={`${segment.label}-${index}`}
              className="type-note inline-flex items-center gap-1.5 text-ink-soft"
            >
              <span
                aria-hidden="true"
                className={cn('size-2.5 shrink-0', toneClass(segment.tone, onPaper))}
              />
              <span>{segment.label}</span>
              <span>{segment.valueLabel ?? segment.value}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

export function StackBarSkeleton({
  size = 'md',
  showLegend = true,
  legendCount = 3,
  className,
}: {
  size?: 'md' | 'sm'
  showLegend?: boolean
  legendCount?: number
  className?: string
}) {
  return (
    <div className={cn('flex w-full flex-col gap-2.5', className)}>
      <Skeleton className={cn('w-full rounded-none', size === 'sm' ? 'h-2' : 'h-2.5')} />
      {showLegend && (
        <div className="flex flex-wrap gap-x-3.5 gap-y-1.5">
          {Array.from({ length: legendCount }, (_, i) => (
            <Skeleton key={i} className="h-4 w-20" />
          ))}
        </div>
      )}
    </div>
  )
}
