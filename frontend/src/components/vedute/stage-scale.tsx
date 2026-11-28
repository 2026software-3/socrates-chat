import { cn } from 'cn'
import { stageForRound, type StageNumber } from './scales'
import { Skeleton } from './skeleton'

export type StageScaleVariant = 'full' | 'compact'

export type StageScaleProps = {
  stage?: StageNumber
  round?: number
  roundLabel?: string
  labels: readonly [string, string, string]
  ariaLabel?: string
  variant?: StageScaleVariant
  className?: string
}

const STAGE_INDICES = [0, 1, 2] as const

export function StageScale({
  stage,
  round,
  roundLabel,
  labels,
  ariaLabel,
  variant = 'full',
  className,
}: StageScaleProps) {
  const currentStage = stage ?? stageForRound(round ?? 1)
  const currentIndex = currentStage - 1

  if (variant === 'compact') {
    const currentLabel = labels[currentIndex]
    const summary = ariaLabel ? `${ariaLabel}：${currentLabel}` : currentLabel

    return (
      <div
        data-vedute-stage-scale=""
        data-variant="compact"
        data-stage={currentStage}
        aria-label={summary}
        className={cn('inline-flex items-center gap-2', className)}
      >
        {roundLabel && (
          <span className="type-note whitespace-nowrap text-ink-soft">{roundLabel}</span>
        )}
        <span aria-hidden="true" className="inline-flex items-center gap-0.75">
          {STAGE_INDICES.map((idx) => (
            <span
              key={idx}
              className={cn(
                'h-1 w-4 shrink-0',
                idx <= currentIndex ? 'bg-caeruleum' : 'bg-caeruleum-wash',
              )}
            />
          ))}
        </span>
        <span className="type-note whitespace-nowrap font-bold text-caeruleum">
          {currentLabel}
        </span>
      </div>
    )
  }

  return (
    <div
      data-vedute-stage-scale=""
      data-variant="full"
      data-stage={currentStage}
      className={cn('inline-flex items-start gap-2.5', className)}
    >
      {roundLabel && (
        <span className="type-note whitespace-nowrap text-ink-soft">{roundLabel}</span>
      )}
      <ol aria-label={ariaLabel} className="m-0 flex list-none items-start gap-1.5 p-0">
        {STAGE_INDICES.map((idx) => {
          const state = idx < currentIndex ? 'done' : idx === currentIndex ? 'current' : 'upcoming'
          const isCurrent = state === 'current'
          const filled = idx <= currentIndex

          return (
            <li
              key={idx}
              data-state={state}
              aria-current={isCurrent ? 'step' : undefined}
              className="flex flex-col items-start gap-1"
            >
              <span
                aria-hidden="true"
                className={cn(
                  'block h-1 w-9 md:w-12',
                  filled ? 'bg-caeruleum' : 'bg-caeruleum-wash',
                )}
              />
              <span
                className={cn(
                  'type-note whitespace-nowrap',
                  isCurrent ? 'font-bold text-caeruleum' : 'text-ink-soft',
                )}
              >
                {labels[idx]}
              </span>
            </li>
          )
        })}
      </ol>
    </div>
  )
}

export function StageScaleSkeleton({
  variant = 'full',
  showRound = false,
  className,
}: {
  variant?: StageScaleVariant
  showRound?: boolean
  className?: string
}) {
  if (variant === 'compact') {
    return (
      <div className={cn('inline-flex items-center gap-2', className)}>
        {showRound && <Skeleton className="h-4 w-10" />}
        <div className="inline-flex items-center gap-0.75">
          <Skeleton className="h-1 w-4 rounded-none" />
          <Skeleton className="h-1 w-4 rounded-none" />
          <Skeleton className="h-1 w-4 rounded-none" />
        </div>
        <Skeleton className="h-4 w-8" />
      </div>
    )
  }

  return (
    <div className={cn('inline-flex items-start gap-2.5', className)}>
      {showRound && <Skeleton className="h-5 w-11" />}
      <div className="flex items-start gap-1.5">
        {STAGE_INDICES.map((idx) => (
          <div key={idx} className="flex flex-col gap-1">
            <Skeleton className="h-1 w-9 rounded-none md:w-12" />
            <Skeleton className="h-4 w-7" />
          </div>
        ))}
      </div>
    </div>
  )
}
