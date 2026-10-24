import type { ReactElement } from 'react'
import { Toaster } from '@/components/ui/sonner'
import type { Lang } from '@/i18n/define'
import { http, HttpResponse } from 'msw'
import type { Classroom, Roles } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

export const ROOM_A = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'
export const ROOM_B = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'

export const classroom = (id: string, name: string): Classroom => ({
  id,
  name,
  year_label: null,
  description: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
})

/** 教師看得到的教室；預設兩間，第一間會是名單頁預設選取的教室。 */
export function mockClassrooms(list: Classroom[] = [classroom(ROOM_A, '2026 春季班'), classroom(ROOM_B, '2026 秋季班')]) {
  server.use(http.get('/api/classrooms', () => HttpResponse.json(list)))
}

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
