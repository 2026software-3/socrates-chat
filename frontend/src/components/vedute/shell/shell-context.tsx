import { createContext, useContext, useEffect } from 'react'
import type { ShellArea } from './areas'

export type ShellHeaderContextValue = {
  mode: 'work' | 'focus'
  area?: ShellArea
  headerSlot: HTMLDivElement | null
  registerPageHeader: () => () => void
  setFocusTitle: (title: string | null) => void
}

export const ShellHeaderContext = createContext<ShellHeaderContextValue | null>(null)

/**
 * Connects `<Page>` / `<PageHeader>` to the active Vedute shell:
 * - In Work shell: reserves the full-bleed `PageHeader` portal slot so the shell hides its fallback banner.
 * - In Focus shell: updates the `BackHeader` title string without touching array/JSX dependencies.
 */
export function useShellHeaderBridge(title: string) {
  const ctx = useContext(ShellHeaderContext)
  const mode = ctx?.mode
  const registerPageHeader = ctx?.registerPageHeader
  const setFocusTitle = ctx?.setFocusTitle

  useEffect(() => {
    if (mode !== 'work' || !registerPageHeader) return
    return registerPageHeader()
  }, [mode, registerPageHeader])

  useEffect(() => {
    if (mode !== 'focus' || !setFocusTitle) return
    setFocusTitle(title)
    return () => setFocusTitle(null)
  }, [mode, setFocusTitle, title])

  return ctx
}
