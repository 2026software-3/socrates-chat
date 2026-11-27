import type { ReactNode } from 'react'
import { cn } from 'cn'
import { DEFAULT_CARD_PLATE } from './shell/plates'
import { toRoman } from './scales'
import { Skeleton } from './skeleton'

export type PlateCardProps = {
  prefix?: string
  numeral: string | number
  title: ReactNode
  headerAction?: ReactNode
  meta?: ReactNode
  action?: ReactNode
  archived?: boolean
  plateSrc?: string
  className?: string
}

export function PlateCard({
  prefix = 'CLASSIS',
  numeral,
  title,
  headerAction,
  meta,
  action,
  archived = false,
  plateSrc,
  className,
}: PlateCardProps) {
  const formatted = typeof numeral === 'number' ? toRoman(numeral) : numeral
  const imgClassName = cn(
    'vedute-plate-image pointer-events-none size-full object-cover',
    archived && 'grayscale',
  )

  return (
    <article
      data-vedute-plate-card=""
      data-archived={archived ? 'true' : undefined}
      className={cn(
        'flex h-full flex-col overflow-hidden rounded-none border border-ink bg-surface',
        className,
      )}
    >
      <div className="relative h-24 w-full shrink-0 overflow-hidden bg-paper">
        {plateSrc ? (
          <img src={plateSrc} alt="" aria-hidden="true" className={imgClassName} />
        ) : (
          <picture className="block size-full">
            <source type="image/avif" srcSet={DEFAULT_CARD_PLATE.avifSrcSet} sizes="(max-width: 767px) 100vw, 400px" />
            <source type="image/webp" srcSet={DEFAULT_CARD_PLATE.webpSrcSet} sizes="(max-width: 767px) 100vw, 400px" />
            <img src={DEFAULT_CARD_PLATE.smWebp} alt="" aria-hidden="true" className={imgClassName} />
          </picture>
        )}
        {!archived && (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 bg-aerugo opacity-35 mix-blend-multiply"
          />
        )}
        <div
          aria-hidden="true"
          className={cn(
            'pointer-events-none absolute inset-0',
            archived ? 'vedute-plate-fade-archived' : 'vedute-plate-fade',
          )}
        />
        <span
          aria-hidden="true"
          className="type-plate-tag absolute top-2.5 left-2.5 border border-ink bg-paper px-1.5 py-px text-ink select-none"
        >
          {prefix} {formatted}
        </span>
      </div>

      <div className="flex flex-1 flex-col gap-2 px-4 pt-3.5 pb-4">
        <div className="flex min-h-7 items-start justify-between gap-2">
          <div className="type-h3 min-w-0 flex-1 line-clamp-2 text-ink">{title}</div>
          {headerAction && <div className="-my-2 -mr-2 shrink-0">{headerAction}</div>}
        </div>
        {meta && (
          <div className="flex flex-wrap items-center gap-x-3.5 gap-y-1">{meta}</div>
        )}
        {action && <div className="mt-auto pt-1 [&>.vedute-button]:w-full">{action}</div>}
      </div>
    </article>
  )
}

const META_SKELETON_WIDTHS = ['w-18', 'w-28', 'w-25'] as const

export function PlateCardSkeleton({
  metaCount = 3,
  className,
}: {
  metaCount?: number
  className?: string
}) {
  return (
    <div
      data-vedute-plate-card-skeleton=""
      className={cn(
        'flex h-full flex-col overflow-hidden rounded-none border border-ink bg-surface',
        className,
      )}
    >
      <Skeleton className="h-24 w-full shrink-0 rounded-none" />
      <div className="flex flex-1 flex-col gap-2 px-4 pt-3.5 pb-4">
        <Skeleton className="h-7 w-3/5" />
        {metaCount > 0 && (
          <div className="flex flex-wrap items-center gap-x-3.5 gap-y-1">
            {Array.from({ length: metaCount }, (_, i) => (
              <Skeleton
                key={i}
                className={cn('h-5.25', META_SKELETON_WIDTHS[i % META_SKELETON_WIDTHS.length])}
              />
            ))}
          </div>
        )}
        <div className="mt-auto pt-1">
          <Skeleton className="h-11 w-full" />
        </div>
      </div>
    </div>
  )
}
