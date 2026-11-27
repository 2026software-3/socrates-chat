import type { ReactNode } from 'react'
import { cn } from 'cn'
import { Skeleton } from './skeleton'
import { StageScaleSkeleton } from './stage-scale'
import { StampSkeleton } from './stamp'

export type DirectoryRowVariant = 'status' | 'topic'
export type DirectoryRowTone = 'active' | 'draft' | 'ended' | 'topic' | 'inactive'

const statusToneClasses: Record<DirectoryRowTone, string> = {
  active: 'border-l-aerugo bg-aerugo-wash/45',
  draft: 'border-l-ochra-graphic bg-surface',
  ended: 'border-l-ink-soft bg-surface',
  topic: 'border-l-caeruleum bg-surface',
  inactive: 'border-l-ink-soft bg-surface',
}

const topicToneClasses: Record<DirectoryRowTone, string> = {
  topic: 'border-l-caeruleum',
  active: 'border-l-aerugo',
  draft: 'border-l-ochra-graphic',
  ended: 'border-l-ink-faint',
  inactive: 'border-l-ink-faint',
}

function rowFrameClass(variant: DirectoryRowVariant, tone: DirectoryRowTone) {
  if (variant === 'topic') {
    return cn(
      'rounded-sm border border-ink-faint border-l-3 bg-surface px-4 py-3.5 md:px-5 md:py-4',
      topicToneClasses[tone],
    )
  }
  return cn('border-l-3 py-3.5 pl-4 pr-3 md:px-4', statusToneClasses[tone])
}

export type DirectoryRowProps = {
  variant?: DirectoryRowVariant
  tone?: DirectoryRowTone
  stamp?: ReactNode
  title: ReactNode
  status?: ReactNode
  meta?: ReactNode
  trailing?: ReactNode
  actions?: ReactNode
  className?: string
}

export function DirectoryRow({
  variant = 'status',
  tone,
  stamp,
  title,
  status,
  meta,
  trailing,
  actions,
  className,
}: DirectoryRowProps) {
  const resolvedTone = tone ?? (variant === 'topic' ? 'topic' : 'active')

  return (
    <div
      data-vedute-directory-row=""
      data-variant={variant}
      data-tone={resolvedTone}
      className={cn(
        'flex items-start gap-3 md:items-center md:gap-4',
        rowFrameClass(variant, resolvedTone),
        className,
      )}
    >
      {stamp && <div className="shrink-0">{stamp}</div>}
      <div className="flex min-w-0 flex-1 flex-col gap-3 md:flex-row md:items-center md:gap-4">
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <div className="type-h3 min-w-0 line-clamp-2 text-ink">{title}</div>
            {status && <div className="shrink-0">{status}</div>}
          </div>
          {meta && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
              {meta}
            </div>
          )}
        </div>
        {(trailing || actions) && (
          <div className="flex flex-wrap items-center justify-between gap-3 md:shrink-0 md:flex-nowrap md:justify-end md:gap-4">
            {trailing && <div className="shrink-0">{trailing}</div>}
            {actions && (
              <div className={cn('flex shrink-0 items-center gap-1', !trailing && 'ml-auto')}>
                {actions}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

export function DirectoryRowSkeleton({
  variant = 'status',
  showStage = false,
  className,
}: {
  variant?: DirectoryRowVariant
  showStage?: boolean
  className?: string
}) {
  const tone: DirectoryRowTone = variant === 'topic' ? 'inactive' : 'ended'

  return (
    <div
      className={cn(
        'flex items-start gap-3 md:items-center md:gap-4',
        rowFrameClass(variant, tone),
        className,
      )}
    >
      <StampSkeleton />
      <div className="flex min-w-0 flex-1 flex-col gap-3 md:flex-row md:items-center md:gap-4">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-center gap-2.5">
            <Skeleton className="h-6 w-2/5 min-w-28" />
            {variant === 'status' && <Skeleton className="h-5 w-14" />}
          </div>
          <div className="flex flex-wrap gap-4">
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-4 w-20" />
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 md:shrink-0 md:flex-nowrap md:justify-end md:gap-4">
          {showStage && <StageScaleSkeleton />}
          <Skeleton className={cn('h-11 w-28', !showStage && 'ml-auto')} />
        </div>
      </div>
    </div>
  )
}
