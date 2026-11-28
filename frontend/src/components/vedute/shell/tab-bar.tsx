import { useRef } from 'react'
import { Link } from 'react-router'
import { cn } from 'cn'
import { useT } from '@/i18n'
import { FREQUENT_AREA_IDS, type ShellArea, type ShellRole } from './areas'

export type TabBarProps = {
  role: ShellRole
  areas: ShellArea[]
  currentArea?: ShellArea
  sheetOpen: boolean
  onOpenSheet: (trigger: HTMLElement | null) => void
}

function PlazaMiniIcon() {
  return (
    <svg viewBox="0 0 30 21" aria-hidden="true" focusable="false" className="block h-[21px] w-[30px]">
      <rect x="0.5" y="0.5" width="29" height="20" fill="none" stroke="currentColor" />
      <rect x="3" y="3" width="4" height="12" fill="currentColor" />
      <rect x="11" y="13" width="13" height="5" fill="currentColor" />
      <rect x="13" y="2" width="4" height="5" fill="currentColor" />
      <circle cx="25" cy="9" r="2.2" fill="currentColor" />
      <path d="M5 10 Q15 12 29 9" fill="none" stroke="currentColor" strokeWidth="0.8" strokeDasharray="1.6 1.2" />
    </svg>
  )
}

/**
 * Mobile (<768) bottom TabBar (Figma Nav/Tab Bar 2884:2655):
 * 60px interactive row + 3px double ink top rule + env(safe-area-inset-bottom) paper fill.
 * Nothing tappable sits inside the home-indicator safe area.
 */
export function TabBar({ role, areas, currentArea, sheetOpen, onOpenSheet }: TabBarProps) {
  const t = useT()
  const plazaBtnRef = useRef<HTMLButtonElement>(null)
  const frequentIds = FREQUENT_AREA_IDS[role]
  const frequentAreas = areas.filter((a) => frequentIds.includes(a.id))
  const isElsewhere = currentArea !== undefined && !frequentIds.includes(currentArea.id)

  const plazaLabel =
    isElsewhere && currentArea
      ? `${currentArea.numeral}・${t(currentArea.shortLabelKey ?? currentArea.labelKey)}`
      : t('shell.plan.plaza')

  return (
    <nav
      aria-label={t('shell.nav.tabBar')}
      className="vedute-double-rule-t vedute-tabbar fixed inset-x-0 bottom-0 z-25 grid grid-cols-4 bg-paper md:hidden"
    >
      {frequentAreas.map((area) => {
        const isCurrent = currentArea?.id === area.id
        return (
          <Link
            key={area.id}
            to={area.to}
            aria-current={isCurrent ? 'page' : undefined}
            className={cn(
              'relative flex min-h-[60px] flex-col items-center justify-center gap-px px-0.5 pt-2 pb-1.5 no-underline focus-visible:outline-2 focus-visible:-outline-offset-4 focus-visible:outline-chalk',
              isCurrent ? 'text-chalk-deep' : 'text-ink-soft',
            )}
          >
            {isCurrent && (
              <span aria-hidden="true" className="absolute top-0 left-1/2 h-0.5 w-11.5 -translate-x-1/2 bg-chalk" />
            )}
            <span aria-hidden="true" className="type-latin-label">
              {area.numeral}
            </span>
            <span className={cn('type-note max-w-full truncate', isCurrent && 'font-bold')}>
              {t(area.labelKey)}
            </span>
          </Link>
        )
      })}

      <button
        ref={plazaBtnRef}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={sheetOpen}
        aria-controls="vedute-plan-sheet"
        aria-current={isElsewhere ? 'page' : undefined}
        onClick={() => onOpenSheet(plazaBtnRef.current)}
        className={cn(
          'relative flex min-h-[60px] cursor-pointer flex-col items-center justify-center gap-px border-0 bg-transparent px-0.5 pt-2 pb-1.5 focus-visible:outline-2 focus-visible:-outline-offset-4 focus-visible:outline-chalk',
          isElsewhere ? 'text-chalk-deep' : 'text-ink-soft',
        )}
      >
        {isElsewhere && (
          <span aria-hidden="true" className="absolute top-0 left-1/2 h-0.5 w-11.5 -translate-x-1/2 bg-chalk" />
        )}
        <PlazaMiniIcon />
        <span className={cn('type-note max-w-full truncate', isElsewhere && 'font-bold')}>{plazaLabel}</span>
      </button>
    </nav>
  )
}
