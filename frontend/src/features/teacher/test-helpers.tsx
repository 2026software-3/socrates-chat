import type { ReactElement } from 'react'
import { Toaster } from '@/components/ui/sonner'
import type { Lang } from '@/i18n/define'
import type { Roles } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'

/** 以教師身分渲染單一頁面（含 Toaster，讓 toast 可被斷言）。 */
export function renderTeacherPage(ui: ReactElement, lang?: Lang, roles: Partial<Roles> = { teacher: true }) {
  // sonner 需要 matchMedia，jsdom 沒有
  if (typeof window.matchMedia !== 'function') {
    window.matchMedia = ((query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    })) as typeof window.matchMedia
  }
  mockMe(makeMe(roles))
  return renderApp(
    <>
      {ui}
      <Toaster />
    </>,
    { lang },
  )
}
