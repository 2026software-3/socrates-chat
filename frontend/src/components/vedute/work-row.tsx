import { Children, useId, useState, type ReactNode } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { cn } from 'cn'
import { BookCover, BookCoverSkeleton } from './book-cover'
import { EditionRowSkeleton } from './edition-row'
import { Skeleton } from './skeleton'

export type WorkRowProps = {
  coverSrc?: string | null
  coverAlt?: string
  cover?: ReactNode
  title: ReactNode
  status?: ReactNode
  meta?: ReactNode
  actions?: ReactNode
  inactive?: boolean
  expanded?: boolean
  defaultExpanded?: boolean
  onExpandedChange?: (expanded: boolean) => void
  expandLabel?: string
  collapseLabel?: string
  children?: ReactNode
  className?: string
}

export function WorkRow({
  coverSrc,
  coverAlt = '',
  cover,
  title,
  status,
  meta,
  actions,
  inactive = false,
  expanded,
  defaultExpanded = false,
  onExpandedChange,
  expandLabel,
  collapseLabel,
  children,
  className,
}: WorkRowProps) {
  const [internalExpanded, setInternalExpanded] = useState(defaultExpanded)
  const panelId = useId()
  const isControlled = expanded !== undefined
  const isExpanded = isControlled ? expanded : internalExpanded
  const hasEditions = Children.count(children) > 0 && Boolean(expandLabel && collapseLabel)

  function toggle() {
    const next = !isExpanded
    if (!isControlled) setInternalExpanded(next)
    onExpandedChange?.(next)
  }

  return (
    <article
      data-vedute-work-row=""
      data-inactive={inactive ? 'true' : undefined}
      className={cn(
        'rounded-sm border border-ink-faint border-l-3 bg-surface py-3.5 pl-4 pr-2 md:pl-5 md:pr-3',
        inactive ? 'border-l-ink-faint' : 'border-l-ochra-graphic',
        className,
      )}
    >
      <div className="flex items-center gap-3 md:gap-4">
        {cover ?? <BookCover size="md" src={coverSrc} alt={coverAlt} />}
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <div className="type-h3 min-w-0 text-ink">{title}</div>
            {status && <div className="shrink-0">{status}</div>}
          </div>
          {meta && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1">{meta}</div>
          )}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-1">{actions}</div>}
      </div>

      {hasEditions && (
        <div className="pt-0.5 md:pl-18">
          <button
            type="button"
            aria-expanded={isExpanded}
            aria-controls={panelId}
            onClick={toggle}
            className="vedute-work-toggle type-note -ml-1 inline-flex min-h-11 cursor-pointer items-center gap-1.5 rounded-sm bg-transparent px-1 text-ink-soft hover:text-chalk-deep active:text-chalk-deep"
          >
            {isExpanded ? (
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <ChevronRight size={16} strokeWidth={1.5} aria-hidden="true" />
            )}
            <span>{isExpanded ? collapseLabel : expandLabel}</span>
          </button>
          {isExpanded && (
            <div id={panelId} className="flex flex-col border-t border-ink-faint">
              {children}
            </div>
          )}
        </div>
      )}
    </article>
  )
}

export function WorkRowSkeleton({
  editionsCount = 0,
  className,
}: {
  editionsCount?: number
  className?: string
}) {
  return (
    <div
      className={cn(
        'rounded-sm border border-ink-faint border-l-3 border-l-ink-faint bg-surface py-3.5 pl-4 pr-2 md:pl-5 md:pr-3',
        className,
      )}
    >
      <div className="flex items-center gap-3 md:gap-4">
        <BookCoverSkeleton size="md" />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-center gap-2.5">
            <Skeleton className="h-6 w-2/5 min-w-28" />
            <Skeleton className="h-5 w-12" />
          </div>
          <div className="flex flex-wrap gap-4">
            <Skeleton className="h-4 w-16" />
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-4 w-16" />
          </div>
        </div>
        <Skeleton className="size-11 shrink-0" />
      </div>
      <div className="pt-1 md:pl-18">
        <Skeleton className="my-2.5 h-4 w-28" />
        {editionsCount > 0 && (
          <div className="flex flex-col border-t border-ink-faint">
            {Array.from({ length: editionsCount }, (_, i) => (
              <EditionRowSkeleton key={i} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
