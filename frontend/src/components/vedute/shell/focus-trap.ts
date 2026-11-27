import { useEffect, useRef, type RefObject } from 'react'

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(', ')

function getFocusable(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(
    (el) => !el.hasAttribute('aria-hidden') && el.getAttribute('tabindex') !== '-1',
  )
}

/**
 * Traps Tab/Shift+Tab inside a modal surface (drawer or bottom sheet), closes on Escape,
 * and restores focus to the trigger element when closed.
 */
export function useFocusTrap(
  containerRef: RefObject<HTMLElement | null>,
  open: boolean,
  onClose: () => void,
  returnFocusTo?: RefObject<HTMLElement | null>,
) {
  const prevFocusedRef = useRef<HTMLElement | null>(null)
  const wasOpenRef = useRef(false)

  useEffect(() => {
    if (open) {
      wasOpenRef.current = true
      prevFocusedRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null

      const frame = requestAnimationFrame(() => {
        const container = containerRef.current
        if (!container) return
        const focusables = getFocusable(container)
        ;(focusables[0] ?? container).focus()
      })

      const onKeyDown = (e: KeyboardEvent) => {
        if (e.key === 'Escape') {
          e.preventDefault()
          onClose()
          return
        }
        if (e.key !== 'Tab') return
        const container = containerRef.current
        if (!container) return
        const focusables = getFocusable(container)
        if (focusables.length === 0) {
          e.preventDefault()
          container.focus()
          return
        }
        const first = focusables[0]!
        const last = focusables[focusables.length - 1]!
        if (e.shiftKey) {
          if (document.activeElement === first || !container.contains(document.activeElement)) {
            e.preventDefault()
            last.focus()
          }
        } else if (document.activeElement === last || !container.contains(document.activeElement)) {
          e.preventDefault()
          first.focus()
        }
      }

      document.addEventListener('keydown', onKeyDown)
      return () => {
        cancelAnimationFrame(frame)
        document.removeEventListener('keydown', onKeyDown)
      }
    }

    if (wasOpenRef.current) {
      wasOpenRef.current = false
      const target = returnFocusTo?.current ?? prevFocusedRef.current
      if (target && target.isConnected) target.focus()
      prevFocusedRef.current = null
    }
  }, [containerRef, open, onClose, returnFocusTo])
}
