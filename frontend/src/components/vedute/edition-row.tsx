import type { ReactNode } from 'react'
import { cn } from 'cn'
import { BookCover, BookCoverSkeleton } from './book-cover'
import { Skeleton } from './skeleton'

export type EditionRowTone = 'default' | 'pending'

export type EditionRowProps = {
  coverSrc?: string | null
  coverAlt?: string
  cover?: ReactNode
  language?: string
  title: ReactNode
  meta?: ReactNode
  actions?: ReactNode
  tone?: EditionRowTone
  className?: string
}

export function EditionRow({
  coverSrc,
  coverAlt = '',
  cover,
  language,
  title,
  meta,
  actions,
  tone = 'default',
  className,
}: EditionRowProps) {
  return (
    <div
      data-vedute-edition-row=""
      data-tone={tone}
      className={cn(
        'flex items-center gap-3 py-3',
        tone === 'pending'
          ? 'rounded-sm border border-ochra-graphic bg-ochra-wash px-3'
          : 'border-b border-ink-faint last:border-b-0',
        className,
      )}
    >
      {cover ?? <BookCover size="sm" src={coverSrc} alt={coverAlt} />}
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
          {language && (
            <span className="type-note inline-flex shrink-0 items-center rounded-sm border border-ink-faint bg-paper px-2 py-0 text-ink">
              {language}
            </span>
          )}
          <span
            className={cn(
              'type-body-strong min-w-0 flex-1 wrap-break-word',
              tone === 'pending' ? 'text-ochra' : 'text-ink',
            )}
          >
            {title}
          </span>
        </div>
        {meta && (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5">{meta}</div>
        )}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-1">{actions}</div>}
    </div>
  )
}

export function EditionRowSkeleton({
  actionCount = 2,
  className,
}: {
  actionCount?: number
  className?: string
}) {
  return (
    <div
      className={cn(
        'flex items-center gap-3 border-b border-ink-faint py-3 last:border-b-0',
        className,
      )}
    >
      <BookCoverSkeleton size="sm" />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <Skeleton className="h-5 w-10" />
          <Skeleton className="h-5 w-2/5 min-w-24" />
        </div>
        <div className="flex flex-wrap gap-3">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-4 w-12" />
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {Array.from({ length: actionCount }, (_, i) => (
          <Skeleton key={i} className="size-11" />
        ))}
      </div>
    </div>
  )
}
