import { useRef, useState, type ReactNode, type RefObject } from 'react'
import { Link, useNavigate } from 'react-router'
import { X } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/vedute/button'
import { useT } from '@/i18n'
import { BUILDINGS, ROLE_FOOTNOTE_KEY, type BuildingId, type ShellArea, type ShellRole } from './areas'
import { useFocusTrap } from './focus-trap'
import { ForumPlan } from './forum-plan'
import { useAnimatedPresence } from './presence'

export type LegendListProps = {
  areas: ShellArea[]
  currentAreaId?: string
  hoveredBuilding?: BuildingId | null
  onHoverBuilding?: (building: BuildingId | null) => void
  onNavigate?: () => void
  columns?: 1 | 2
  navLabel?: string
  className?: string
}

export function LegendList({
  areas,
  currentAreaId,
  hoveredBuilding,
  onHoverBuilding,
  onNavigate,
  columns = 1,
  navLabel,
  className,
}: LegendListProps) {
  const t = useT()

  const list = (
    <ol
      className={cn(
        'm-0 list-none border-t border-ink p-0',
        columns === 2 ? 'grid grid-cols-2 gap-x-3' : 'flex flex-col',
        className,
      )}
      style={{ borderTopWidth: 'var(--stroke-emphasis)' }}
    >
      {areas.map((area) => {
        const isCurrent = area.id === currentAreaId
        const isHovered = hoveredBuilding === area.building
        const latin = BUILDINGS[area.building].latin
        return (
          <li key={area.id}>
            <Link
              to={area.to}
              aria-current={isCurrent ? 'page' : undefined}
              onClick={onNavigate}
              onPointerEnter={onHoverBuilding ? () => onHoverBuilding(area.building) : undefined}
              onPointerLeave={onHoverBuilding ? () => onHoverBuilding(null) : undefined}
              className={cn(
                'relative grid min-h-12 grid-cols-[30px_minmax(0,1fr)] items-center gap-x-2 border-b border-ink-faint py-1.5 pr-2 pl-1.5 no-underline outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-chalk',
                isCurrent && 'bg-surface',
                columns === 2 && 'min-h-13',
              )}
            >
              {isCurrent && (
                <span aria-hidden="true" className="absolute top-2 bottom-3 left-0 w-0.5 bg-chalk" />
              )}
              <span
                aria-hidden="true"
                className={cn(
                  'type-latin-label text-center',
                  isCurrent ? 'text-chalk' : 'text-ink-soft',
                )}
              >
                {area.numeral}
              </span>
              <span className="flex min-w-0 flex-col">
                <span
                  className={cn(
                    'truncate',
                    isCurrent ? 'type-body-strong text-chalk' : 'type-body text-ink',
                    !isCurrent && isHovered && 'text-chalk',
                  )}
                >
                  {t(area.labelKey)}
                </span>
                <span aria-hidden="true" className="type-latin-small truncate text-ink-soft">
                  {latin}
                </span>
              </span>
            </Link>
          </li>
        )
      })}
    </ol>
  )

  if (!navLabel) return list
  return <nav aria-label={navLabel}>{list}</nav>
}

export type SidebarProps = {
  role: ShellRole
  areas: ShellArea[]
  currentArea?: ShellArea
  onNavigate?: () => void
  footerExtra?: ReactNode
  className?: string
}

/**
 * Work-shell sidebar panel (Figma Nav/Sidebar 2268:1842):
 * Nolli forum plan on top, numbered legend navigation below, role footnote at bottom.
 */
export function Sidebar({ role, areas, currentArea, onNavigate, footerExtra, className }: SidebarProps) {
  const t = useT()
  const navigate = useNavigate()
  const [hovered, setHovered] = useState<BuildingId | null>(null)

  function handleSelectArea(area: ShellArea) {
    navigate(area.to)
    onNavigate?.()
  }

  return (
    <div
      data-vedute-sidebar=""
      className={cn(
        'vedute-sidebar-fixed flex h-full flex-col gap-4 border-r border-ink-faint bg-paper px-4 pt-[26px] pb-6',
        className,
      )}
    >
      <div className="flex flex-col gap-1.5">
        <ForumPlan
          areas={areas}
          currentAreaId={currentArea?.id}
          hoveredBuilding={hovered}
          onHoverBuilding={setHovered}
          onSelectArea={handleSelectArea}
        />
        <div className="flex items-baseline justify-between gap-2 text-ink-soft">
          <span aria-hidden="true" className="type-latin-small">
            Pianta del Foro
          </span>
          <span className="type-note">
            {currentArea ? t('shell.plan.youAreAt', { numeral: currentArea.numeral }) : '—'}
          </span>
        </div>
      </div>

      <LegendList
        areas={areas}
        currentAreaId={currentArea?.id}
        hoveredBuilding={hovered}
        onHoverBuilding={setHovered}
        onNavigate={onNavigate}
        navLabel={t('shell.nav.main')}
      />

      <div className="flex-1" />

      {footerExtra}

      <p className="type-note text-ink-soft">{t(ROLE_FOOTNOTE_KEY[role])}</p>
    </div>
  )
}

export type SidebarDrawerProps = {
  open: boolean
  onClose: () => void
  returnFocusTo?: RefObject<HTMLElement | null>
  role: ShellRole
  areas: ShellArea[]
  currentArea?: ShellArea
  footerExtra?: ReactNode
}

const DRAWER_TRANSITION_MS = 260

/** Tablet (768–1023) slide-in drawer hosting the same 216px Sidebar panel with focus trap. */
export function SidebarDrawer({
  open,
  onClose,
  returnFocusTo,
  role,
  areas,
  currentArea,
  footerExtra,
}: SidebarDrawerProps) {
  const t = useT()
  const drawerRef = useRef<HTMLDivElement>(null)
  const { mounted, shown } = useAnimatedPresence(open, DRAWER_TRANSITION_MS)
  useFocusTrap(drawerRef, open, onClose, returnFocusTo)

  if (!mounted) return null

  return (
    <>
      <div
        aria-hidden="true"
        onClick={open ? onClose : undefined}
        className={cn(
          'vedute-scrim fixed inset-0 z-30 hidden md:block lg:hidden',
          shown && 'is-open',
        )}
      />
      <div
        ref={drawerRef}
        id="vedute-sidebar-drawer"
        role={open ? 'dialog' : undefined}
        aria-modal={open || undefined}
        aria-hidden={!open || undefined}
        aria-label={open ? t('shell.nav.main') : undefined}
        inert={!open}
        tabIndex={-1}
        className={cn(
          'vedute-drawer fixed inset-y-0 left-0 z-35 hidden flex-col overflow-y-auto border-r border-ink-faint bg-paper outline-none md:flex lg:hidden',
          shown && 'is-open',
          !open && 'pointer-events-none',
        )}
      >
        <div className="flex items-center justify-end px-2 pt-1">
          <Button variant="icon" aria-label={t('shell.nav.close')} onClick={onClose}>
            <X aria-hidden="true" />
          </Button>
        </div>
        <Sidebar
          role={role}
          areas={areas}
          currentArea={currentArea}
          onNavigate={onClose}
          footerExtra={footerExtra}
          className="h-auto flex-1 border-r-0 pt-0"
        />
      </div>
    </>
  )
}
