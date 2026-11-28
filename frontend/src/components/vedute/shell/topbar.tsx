import { useRef, useState, type RefObject } from 'react'
import { Link } from 'react-router'
import { ChevronDown, Menu } from 'lucide-react'
import { Button } from '@/components/vedute/button'
import { useT } from '@/i18n'
import type { Lang } from '@/i18n/define'
import type { Me } from '@/lib/types'
import { AccountMenu } from './account-menu'
import { BUILDINGS, ROLE_HOME, type ShellArea, type ShellRole } from './areas'
import { CarvedInscription } from './carved-inscription'

export type TopbarProps = {
  me: Me
  role: ShellRole
  area?: ShellArea
  /** Student points balance; omitted or null hides the points indicator (never falls back to 0 on error). */
  points?: number | null
  drawerOpen: boolean
  sheetOpen: boolean
  onOpenDrawer: () => void
  onOpenSheet: (trigger: HTMLElement | null) => void
  menuButtonRef?: RefObject<HTMLButtonElement | null>
  onSelectLanguage?: (lang: Lang) => void
  languageDisabled?: boolean
  onLogout?: () => void
}

/**
 * Work-shell Topbar (Figma Page/Topbar 2269:1558):
 * Left: ☰ (768–1023 only) + brand (≥768) + Latin subtitle (≥1024).
 * Center: numeral pin (≥768) + carved Latin inscription + ⌄ (<768 to open PlanSheet) + area label (≥1024).
 * Right: student points (≥768) + avatar account menu.
 */
export function Topbar({
  me,
  role,
  area,
  points,
  drawerOpen,
  sheetOpen,
  onOpenDrawer,
  onOpenSheet,
  menuButtonRef,
  onSelectLanguage,
  languageDisabled,
  onLogout,
}: TopbarProps) {
  const t = useT()
  const [accountOpen, setAccountOpen] = useState(false)
  const accountBtnRef = useRef<HTMLButtonElement>(null)
  const locationBtnRef = useRef<HTMLButtonElement>(null)

  const name = me.display_name ?? me.email ?? ''
  const initial = [...name][0]?.toUpperCase() ?? '?'
  const building = area ? BUILDINGS[area.building] : undefined
  const areaLabel = area ? t(area.labelKey) : ''

  return (
    <header className="sticky top-0 z-20 flex h-13 shrink-0 items-center justify-between gap-3 border-b border-ink bg-paper px-4 md:px-6">
      {/* Left: hamburger (tablet only) + brand (≥768) + Latin subtitle (≥1024) */}
      <div className="flex min-w-0 shrink-0 items-center gap-2.5">
        <Button
          ref={menuButtonRef}
          variant="icon"
          aria-label={t('shell.nav.open')}
          aria-expanded={drawerOpen}
          aria-controls="vedute-sidebar-drawer"
          onClick={onOpenDrawer}
          className="hidden md:inline-flex lg:hidden"
        >
          <Menu aria-hidden="true" />
        </Button>

        <Link
          to={ROLE_HOME[role]}
          className="hidden items-baseline gap-2.5 text-ink no-underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk md:inline-flex"
        >
          <span className="type-h3 whitespace-nowrap">{t('shell.brand')}</span>
          <span aria-hidden="true" className="vedute-brand-latin hidden text-ink-soft lg:inline">
            Schola Socratica
          </span>
        </Link>
      </div>

      {/* Center: carved location (interactive button on phones <768 to open PlanSheet) */}
      {area && building && (
        <div
          aria-live="polite"
          className="pointer-events-none absolute top-1/2 left-1/2 flex max-w-[calc(100%-8rem)] -translate-x-1/2 -translate-y-1/2 items-center justify-center md:max-w-[46%]"
        >
          <button
            ref={locationBtnRef}
            type="button"
            aria-haspopup="dialog"
            aria-expanded={sheetOpen}
            aria-controls="vedute-plan-sheet"
            onClick={() => onOpenSheet(locationBtnRef.current)}
            className="pointer-events-auto inline-flex min-h-11 cursor-pointer items-center gap-1 bg-transparent p-0 text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk md:hidden"
          >
            <span className="sr-only">{t('shell.plan.openLocation', { label: areaLabel })}</span>
            <CarvedInscription text={building.latin} animateKey={`${role}-${area.building}`} variant="topbar" />
            <ChevronDown aria-hidden="true" className="size-4 shrink-0 text-chalk-deep" />
          </button>

          <div className="hidden items-center gap-3 md:flex">
            <span aria-hidden="true" className="vedute-pin inline-grid shrink-0 place-items-center">
              {area.numeral}
            </span>
            <CarvedInscription text={building.latin} animateKey={`${role}-${area.building}`} variant="topbar" />
            <span className="vedute-topbar-area-label type-note hidden whitespace-nowrap text-ink-soft lg:inline">{areaLabel}</span>
          </div>
        </div>
      )}

      {/* Right: student points (≥768) + avatar account menu */}
      <div className="relative flex shrink-0 items-center gap-4">
        {role === 'student' && points !== undefined && points !== null && (
          <div className="hidden items-baseline gap-1.5 whitespace-nowrap md:inline-flex">
            <span className="type-note text-ink-soft">{t('shell.points.label')}</span>
            <span className="type-body-strong text-ochra tabular-nums">{points}</span>
          </div>
        )}

        <button
          ref={accountBtnRef}
          type="button"
          aria-haspopup="true"
          aria-expanded={accountOpen}
          aria-controls="vedute-account-menu"
          onClick={() => setAccountOpen((v) => !v)}
          className="inline-flex min-h-11 min-w-11 cursor-pointer items-center justify-center gap-2 border-0 bg-transparent px-1 text-ink hover:text-chalk focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
        >
          <span
            aria-hidden="true"
            className="type-note inline-grid size-7 shrink-0 place-items-center rounded-full border border-ink-faint bg-paper-deep text-ink"
          >
            {initial}
          </span>
          <span className="type-body sr-only max-w-32 truncate md:not-sr-only">{name}</span>
          <ChevronDown aria-hidden="true" className="size-4 shrink-0 text-ink-soft" />
        </button>

        <AccountMenu
          open={accountOpen}
          me={me}
          currentRole={role}
          onClose={() => setAccountOpen(false)}
          onSelectLanguage={onSelectLanguage}
          languageDisabled={languageDisabled}
          onLogout={onLogout}
          returnFocusTo={accountBtnRef}
        />
      </div>
    </header>
  )
}
