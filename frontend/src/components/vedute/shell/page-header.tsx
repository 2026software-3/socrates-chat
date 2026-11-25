import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { cn } from 'cn'
import { useT } from '@/i18n'
import { BUILDINGS, type AreaSubTab, type HeaderWash, type ShellArea } from './areas'
import { CarvedInscription } from './carved-inscription'
import { PLATES } from './plates'

export type HeaderCrumb = { label: string; to?: string }

const WASH_CLASS: Record<Exclude<HeaderWash, 'none'>, string> = {
  aerugo: 'bg-aerugo',
  ochra: 'bg-ochra-graphic',
  caeruleum: 'bg-caeruleum',
}

export type PageHeaderProps = {
  title: ReactNode
  subtitle?: ReactNode
  crumbs?: HeaderCrumb[]
  actions?: ReactNode
  area?: ShellArea
  /** Renders heading as `<h1>` by default; pass `'p'` only if the page body renders its own `<h1>`. */
  asHeading?: boolean
  className?: string
}

/**
 * Work-shell full-bleed Veduta header (Figma Page/Header 2271:2199):
 * responsive AVIF/WebP plate + area pigment wash (MULTIPLY 35%) + white fade gradient +
 * title, carved Latin inscription (380ms delayed), subtitle, and top-right numeral pin.
 */
export function PageHeader({
  title,
  subtitle,
  crumbs,
  actions,
  area,
  asHeading = true,
  className,
}: PageHeaderProps) {
  const building = area ? BUILDINGS[area.building] : BUILDINGS.basilica
  const plate = PLATES[building.plate]
  const wash = building.wash
  const TitleTag = asHeading ? 'h1' : 'p'

  return (
    <div
      data-vedute-page-header=""
      className={cn(
        'relative min-h-[150px] w-full overflow-hidden border-b border-ink bg-paper md:min-h-[176px]',
        className,
      )}
    >
      <picture className="pointer-events-none absolute inset-0 block size-full">
        <source type="image/avif" srcSet={plate.avifSrcSet} sizes="(max-width: 767px) 100vw, 1224px" />
        <source type="image/webp" srcSet={plate.webpSrcSet} sizes="(max-width: 767px) 100vw, 1224px" />
        <img
          src={plate.fallback}
          alt=""
          aria-hidden="true"
          style={{ objectPosition: building.pos }}
          className="size-full object-cover"
        />
      </picture>

      {wash !== 'none' && (
        <div
          aria-hidden="true"
          data-vedute-wash={wash}
          className={cn('pointer-events-none absolute inset-0 opacity-35 mix-blend-multiply', WASH_CLASS[wash])}
        />
      )}

      <div aria-hidden="true" className="vedute-page-header-fade pointer-events-none absolute inset-0" />

      {area && (
        <span
          aria-hidden="true"
          data-vedute-header-pin=""
          className="type-latin-small absolute top-3 right-4 z-10 inline-grid size-7 place-items-center rounded-full border border-chalk bg-paper text-chalk select-none"
        >
          {area.numeral}
        </span>
      )}

      <div className="relative z-10 flex min-h-[150px] flex-col justify-end gap-1 px-4 pt-7 pb-5 md:min-h-[176px] md:px-7 lg:px-10">
        {crumbs && crumbs.length > 0 && (
          <nav aria-label="breadcrumb" className="type-note flex flex-wrap items-center gap-x-1.5 text-ink-soft">
            {crumbs.map((crumb, idx) => (
              <span key={`${idx}-${crumb.label}`} className="inline-flex items-center gap-x-1.5">
                {idx > 0 && <span aria-hidden="true">/</span>}
                {crumb.to ? (
                  <Link
                    to={crumb.to}
                    className="text-ink-soft no-underline hover:text-chalk hover:underline hover:underline-offset-4 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
                  >
                    {crumb.label}
                  </Link>
                ) : (
                  <span aria-current="page" className="text-ink">
                    {crumb.label}
                  </span>
                )}
              </span>
            ))}
          </nav>
        )}

        <div className="flex flex-wrap items-end justify-between gap-4">
          <div className="flex min-w-0 flex-col gap-1">
            <TitleTag className="type-h1 text-ink">{title}</TitleTag>
            <CarvedInscription
              text={building.latin}
              delayMs={380}
              animateKey={area ? `${area.id}-${area.building}` : building.id}
              variant="header"
            />
            {subtitle && <div className="type-body hidden text-ink-soft md:block">{subtitle}</div>}
          </div>
          {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
        </div>
      </div>
    </div>
  )
}

export type PageTitleProps = {
  title: ReactNode
  eyebrow?: ReactNode
  latin?: string
  note?: ReactNode
  actions?: ReactNode
  className?: string
}

/** Sub-page inline title header (Figma Page/Title 2092:6788). */
export function PageTitle({ title, eyebrow, latin, note, actions, className }: PageTitleProps) {
  return (
    <div className={cn('flex flex-wrap items-center justify-between gap-4', className)}>
      <div className="flex min-w-0 flex-col gap-1">
        {eyebrow && <p className="type-note tracking-[0.12em] text-ink-soft">{eyebrow}</p>}
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <h1 className="type-h1 text-ink">{title}</h1>
          {latin && (
            <span aria-hidden="true" className="vedute-inscription vedute-inscription-header">
              {latin}
            </span>
          )}
          {note && <p className="type-note text-chalk-deep">{note}</p>}
        </div>
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  )
}

export type AreaSubTabsProps = {
  tabs: AreaSubTab[]
  pathname: string
  className?: string
}

/** Nav/Tab Item horizontal bar under PageHeader for grouped sub-pages (DESIGN.md Navigation). */
export function AreaSubTabs({ tabs, pathname, className }: AreaSubTabsProps) {
  const t = useT()
  return (
    <nav
      aria-label={t('shell.nav.subPages')}
      className={cn(
        'flex gap-7 overflow-x-auto border-b border-ink-faint px-4 md:px-7 lg:px-10',
        className,
      )}
    >
      {tabs.map((tab) => {
        const isCurrent = pathname === tab.to
        return (
          <Link
            key={tab.to}
            to={tab.to}
            aria-current={isCurrent ? 'page' : undefined}
            className={cn(
              'vedute-area-tab inline-flex min-h-11 shrink-0 items-center whitespace-nowrap no-underline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-chalk',
              isCurrent ? 'type-body-strong text-ink' : 'type-body text-ink-soft hover:text-chalk',
            )}
          >
            {t(tab.labelKey)}
          </Link>
        )
      })}
    </nav>
  )
}
