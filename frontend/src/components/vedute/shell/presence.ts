import { useEffect, useState } from 'react'
import { useMediaQuery } from '@/hooks/use-media-query'

/**
 * Coordinates mount/unmount with CSS enter/exit transitions for overlays (drawer, bottom sheet).
 * - Under `prefers-reduced-motion: reduce`: mounts and unmounts immediately in the final state.
 * - Otherwise: mounts at the closed transform, flips `shown` after two animation frames so
 *   the browser plays the enter transition, and keeps mounted for `durationMs` on close so
 *   the exit transition plays before unmounting.
 */
export function useAnimatedPresence(open: boolean, durationMs: number): { mounted: boolean; shown: boolean } {
  const reduced = useMediaQuery('(prefers-reduced-motion: reduce)', false)
  const [present, setPresent] = useState(open)
  const [shown, setShown] = useState(open)

  useEffect(() => {
    if (reduced) return

    if (open) {
      let raf2 = 0
      const raf1 = requestAnimationFrame(() => {
        setPresent(true)
        raf2 = requestAnimationFrame(() => setShown(true))
      })
      return () => {
        cancelAnimationFrame(raf1)
        cancelAnimationFrame(raf2)
      }
    }

    const raf = requestAnimationFrame(() => setShown(false))
    if (!present) {
      return () => cancelAnimationFrame(raf)
    }
    const timer = window.setTimeout(() => setPresent(false), durationMs)
    return () => {
      cancelAnimationFrame(raf)
      window.clearTimeout(timer)
    }
  }, [open, reduced, durationMs, present])

  if (reduced) {
    return { mounted: open, shown: open }
  }
  return {
    mounted: open || present,
    shown: open && shown,
  }
}
