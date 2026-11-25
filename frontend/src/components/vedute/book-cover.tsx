import { useState } from 'react'
import { BookOpen } from 'lucide-react'
import { cn } from 'cn'
import { Skeleton } from './skeleton'

export type BookCoverSize = 'sm' | 'md' | 'lg'

const sizeMap: Record<BookCoverSize, { box: string; spine: string; icon: number }> = {
  sm: { box: 'h-11 w-8', spine: 'left-0.75', icon: 14 },
  md: { box: 'h-19.5 w-14', spine: 'left-1.5', icon: 20 },
  lg: { box: 'h-39 w-28', spine: 'left-3', icon: 32 },
}

export type BookCoverProps = {
  size?: BookCoverSize
  src?: string | null
  alt?: string
  placeholderLabel?: string
  className?: string
}

export function BookCover({
  size = 'sm',
  src,
  alt = '',
  placeholderLabel,
  className,
}: BookCoverProps) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null)
  const showImage = Boolean(src) && failedSrc !== src
  const dims = sizeMap[size]

  if (showImage && src) {
    return (
      <div
        data-vedute-book-cover=""
        data-size={size}
        className={cn(
          'relative shrink-0 overflow-hidden rounded-sm border border-ink-faint bg-paper-deep',
          dims.box,
          className,
        )}
      >
        <img
          src={src}
          alt={alt}
          onError={() => setFailedSrc(src)}
          className="size-full object-cover"
        />
      </div>
    )
  }

  return (
    <div
      data-vedute-book-cover=""
      data-size={size}
      data-placeholder="true"
      aria-hidden={placeholderLabel ? undefined : true}
      className={cn(
        'relative flex shrink-0 flex-col items-center justify-center gap-1 overflow-hidden rounded-sm border border-ink-faint bg-paper-deep text-ink-soft',
        dims.box,
        className,
      )}
    >
      <span
        aria-hidden="true"
        className={cn('pointer-events-none absolute inset-y-0 w-px bg-ink-faint', dims.spine)}
      />
      <BookOpen size={dims.icon} strokeWidth={1.5} absoluteStrokeWidth aria-hidden="true" />
      {size === 'lg' && placeholderLabel && (
        <span className="type-note whitespace-nowrap text-ink-soft">{placeholderLabel}</span>
      )}
    </div>
  )
}

export function BookCoverSkeleton({
  size = 'sm',
  className,
}: {
  size?: BookCoverSize
  className?: string
}) {
  return <Skeleton className={cn('shrink-0 rounded-sm', sizeMap[size].box, className)} />
}
