import type { ReactElement } from 'react'
import { Toaster } from '@/components/ui/sonner'
import type { Lang } from '@/i18n/define'
import { makeMe, mockMe, renderApp } from '@/test/render'

/** 以教師身分渲染單一頁面（含 Toaster，讓 toast 可被斷言）。 */
export function renderTeacherPage(ui: ReactElement, lang?: Lang) {
  // sonner 需要 matchMedia，jsdom 沒有
  if (typeof window.matchMedia !== 'function') {
    window.matchMedia = ((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    })) as typeof window.matchMedia
  }
  mockMe(makeMe({ teacher: true }))
  return renderApp(
    <>
      {ui}
      <Toaster />
    </>,
    { lang },
  )
}
