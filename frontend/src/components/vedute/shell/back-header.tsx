import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { ArrowLeft } from 'lucide-react'
import { cn } from 'cn'
import { CarvedInscription } from './carved-inscription'

export type BackHeaderProps = {
  backTo: string
  backLabel: string
  title: ReactNode
  numeral: string
  latin: string
  detail?: string
  trailing?: ReactNode
  /** Renders title as `<h1>` when the page itself does not render an `<h1>`. */
  asHeading?: boolean
  className?: string
}

/**
 * Focus-shell sticky header (Figma Page/Back Header 2269:1853):
 * used for single-document reading/writing pages (conversation, transcript, summary).
 */
export function BackHeader({
  backTo,
  backLabel,
  title,
  numeral,
  latin,
  detail,
  trailing,
  asHeading = false,
  className,
}: BackHeaderProps) {
  const TitleTag = asHeading ? 'h1' : 'p'

  return (
    <header
      className={cn(
        'vedute-double-rule-b paper-texture sticky top-0 z-20 flex min-h-14 shrink-0 items-center gap-2 px-2 py-2 md:gap-4 md:px-7 md:py-3 lg:px-10',
        className,
      )}
    >
      <Link
        to={backTo}
        className="inline-flex min-h-11 min-w-11 shrink-0 items-center justify-center gap-2 px-2 text-ink no-underline hover:text-chalk focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
      >
        <ArrowLeft aria-hidden="true" className="size-5 shrink-0" />
        <span
          className="type-body sr-only underline decoration-chalk underline-offset-4 md:not-sr-only"
          style={{ textDecorationThickness: 'var(--stroke-emphasis)' }}
        >
          {backLabel}
        </span>
      </Link>

      <span aria-hidden="true" className="h-6 w-px shrink-0 bg-ink-faint" />

      <TitleTag className="type-h3 md:type-h1 min-w-0 flex-1 truncate text-ink">{title}</TitleTag>

      {trailing && <div className="flex shrink-0 items-center gap-2">{trailing}</div>}

      <div className="inline-flex shrink-0 items-center gap-2 whitespace-nowrap">
        <span
          aria-hidden="true"
          className="type-latin-small inline-grid size-6.5 shrink-0 place-items-center rounded-full border border-chalk bg-paper text-chalk"
        >
          {numeral}
        </span>
        <CarvedInscription
          text={latin}
          animateKey={`focus-${numeral}-${latin}`}
          variant="back"
          className="hidden md:inline"
        />
        {detail && <span className="type-note text-ink-soft">{detail}</span>}
      </div>
    </header>
  )
}
