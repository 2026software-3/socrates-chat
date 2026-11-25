import { useId, useRef, useState, type PointerEvent, type RefObject } from 'react'
import { useNavigate } from 'react-router'
import { X } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/vedute/button'
import { useT } from '@/i18n'
import type { BuildingId, ShellArea, ShellRole } from './areas'
import { useFocusTrap } from './focus-trap'
import { ForumPlan } from './forum-plan'
import { useAnimatedPresence } from './presence'
import { LegendList } from './sidebar'

export type PlanSheetProps = {
  open: boolean
  role: ShellRole
  areas: ShellArea[]
  currentArea?: ShellArea
  onClose: () => void
  returnFocusTo?: RefObject<HTMLElement | null>
}

const DRAG_DISMISS_PX = 90
const SHEET_TRANSITION_MS = 340

/**
 * Mobile (<768) bottom sheet with enlarged Nolli Forum Plan and 2-column legend
 * (Figma Nav/Plan Sheet 2885:4075, 0.32s cubic-bezier(.2,.7,.2,1)).
 */
export function PlanSheet({ open, areas, currentArea, onClose, returnFocusTo }: PlanSheetProps) {
  const t = useT()
  const navigate = useNavigate()
  const headingId = useId()
  const sheetRef = useRef<HTMLElement>(null)
  const [hovered, setHovered] = useState<BuildingId | null>(null)
  const [dragY, setDragY] = useState<number | null>(null)
  const dragStartRef = useRef<number | null>(null)

  const { mounted, shown } = useAnimatedPresence(open, SHEET_TRANSITION_MS)
  useFocusTrap(sheetRef, open, onClose, returnFocusTo)

  if (!mounted) return null

  function handleSelectArea(area: ShellArea) {
    navigate(area.to)
    onClose()
  }

  function handlePointerDown(e: PointerEvent<HTMLElement>) {
    if ((e.target as HTMLElement).closest('button')) return
    dragStartRef.current = e.clientY
    setDragY(0)
    e.currentTarget.setPointerCapture?.(e.pointerId)
  }

  function handlePointerMove(e: PointerEvent<HTMLElement>) {
    if (dragStartRef.current === null) return
    setDragY(Math.max(0, e.clientY - dragStartRef.current))
  }

  function handlePointerEnd() {
    if (dragStartRef.current === null) return
    const dy = dragY ?? 0
    dragStartRef.current = null
    setDragY(null)
    if (dy > DRAG_DISMISS_PX) onClose()
  }

  const whereText = currentArea
    ? t('shell.plan.where', { numeral: currentArea.numeral, label: t(currentArea.labelKey) })
    : ''

  return (
    <>
      <div
        aria-hidden="true"
        onClick={open ? onClose : undefined}
        className={cn(
          'vedute-scrim fixed inset-0 z-40 md:hidden',
          shown && 'is-open',
        )}
      />
      <section
        ref={sheetRef}
        id="vedute-plan-sheet"
        role={open ? 'dialog' : undefined}
        aria-modal={open || undefined}
        aria-hidden={!open || undefined}
        aria-labelledby={open ? headingId : undefined}
        inert={!open}
        tabIndex={-1}
        style={open && dragY !== null ? { transform: `translateY(${dragY}px)` } : undefined}
        className={cn(
          'vedute-double-rule-t vedute-sheet fixed inset-x-0 bottom-0 z-41 flex max-h-[88dvh] flex-col bg-paper outline-none md:hidden',
          shown && 'is-open',
          open && dragY !== null && 'is-dragging',
          !open && 'pointer-events-none',
        )}
      >
        <div
          aria-hidden="true"
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerEnd}
          onPointerCancel={handlePointerEnd}
          className="grid h-6.5 shrink-0 cursor-grab touch-none place-items-center"
        >
          <span className="h-1 w-10 rounded-full bg-ink-faint" />
        </div>

        <div
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerEnd}
          onPointerCancel={handlePointerEnd}
          className="flex shrink-0 touch-none items-center gap-3 pb-2"
        >
          <h2 id={headingId} className="type-latin-small tracking-[0.26em] text-ink">
            Pianta del Foro
          </h2>
          {whereText && <span className="type-note truncate text-ink-soft">{whereText}</span>}
          <Button
            variant="icon"
            aria-label={t('shell.plan.close')}
            onClick={onClose}
            className="ml-auto shrink-0"
          >
            <X aria-hidden="true" />
          </Button>
        </div>

        <div className="flex flex-col gap-3.5 overflow-y-auto pb-1">
          <div className="bg-surface/65">
            <ForumPlan
              areas={areas}
              currentAreaId={currentArea?.id}
              hoveredBuilding={hovered}
              onHoverBuilding={setHovered}
              onSelectArea={handleSelectArea}
            />
          </div>
          <p className="type-note text-ink-soft">{t('shell.plan.hint')}</p>
          <LegendList
            areas={areas}
            currentAreaId={currentArea?.id}
            hoveredBuilding={hovered}
            onHoverBuilding={setHovered}
            onNavigate={onClose}
            columns={2}
          />
        </div>
      </section>
    </>
  )
}
