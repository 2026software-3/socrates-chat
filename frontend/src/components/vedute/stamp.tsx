import { cn } from 'cn'
import { toRoman } from './scales'
import { Skeleton } from './skeleton'

export type StampTone = 'paper' | 'aerugo' | 'ochra' | 'caeruleum'
export type StampSize = 'responsive' | 'md' | 'sm'

const toneStyles: Record<StampTone, { box: string; prefix: string; numeral: string }> = {
  paper: {
    box: 'border-ink bg-paper',
    prefix: 'text-ink-soft',
    numeral: 'text-ink',
  },
  aerugo: {
    box: 'border-aerugo bg-aerugo-wash',
    prefix: 'text-aerugo',
    numeral: 'text-aerugo',
  },
  ochra: {
    box: 'border-ochra bg-ochra-wash',
    prefix: 'text-ochra',
    numeral: 'text-ochra',
  },
  caeruleum: {
    box: 'border-caeruleum bg-caeruleum-wash',
    prefix: 'text-caeruleum',
    numeral: 'text-caeruleum',
  },
}

const sizeStyles: Record<
  StampSize,
  { box: string; prefix: string; prefixCompact: string; numeral: string }
> = {
  responsive: {
    box: 'size-12 md:size-16',
    prefix: 'type-stamp-label-sm md:type-stamp-label-md',
    prefixCompact: 'type-stamp-label-compact-sm md:type-stamp-label-compact-md',
    numeral: 'type-stamp-numeral-sm md:type-stamp-numeral-md',
  },
  md: {
    box: 'size-16',
    prefix: 'type-stamp-label-md',
    prefixCompact: 'type-stamp-label-compact-md',
    numeral: 'type-stamp-numeral-md',
  },
  sm: {
    box: 'size-12',
    prefix: 'type-stamp-label-sm',
    prefixCompact: 'type-stamp-label-compact-sm',
    numeral: 'type-stamp-numeral-sm',
  },
}

export type StampProps = {
  prefix: string
  numeral: string | number
  tone?: StampTone
  size?: StampSize
  ariaLabel?: string
  className?: string
}

export function Stamp({
  prefix,
  numeral,
  tone = 'paper',
  size = 'responsive',
  ariaLabel,
  className,
}: StampProps) {
  const compactPrefix = prefix.length >= 6
  const formatted = typeof numeral === 'number' ? toRoman(numeral) : numeral
  const palette = toneStyles[tone]
  const sizing = sizeStyles[size]

  return (
    <div
      data-vedute-stamp=""
      data-tone={tone}
      data-size={size}
      data-compact-prefix={compactPrefix ? 'true' : undefined}
      role={ariaLabel ? 'img' : undefined}
      aria-label={ariaLabel}
      aria-hidden={ariaLabel ? undefined : true}
      className={cn(
        'vedute-stamp relative flex shrink-0 flex-col items-center justify-center overflow-hidden rounded-none border select-none',
        palette.box,
        sizing.box,
        className,
      )}
    >
      <span
        className={cn(
          'whitespace-nowrap',
          compactPrefix ? sizing.prefixCompact : sizing.prefix,
          palette.prefix,
        )}
      >
        {prefix}
      </span>
      <span className={cn('whitespace-nowrap', sizing.numeral, palette.numeral)}>
        {formatted}
      </span>
    </div>
  )
}

export function StampSkeleton({
  size = 'responsive',
  className,
}: {
  size?: StampSize
  className?: string
}) {
  return (
    <Skeleton
      className={cn('shrink-0 rounded-none', sizeStyles[size].box, className)}
    />
  )
}
