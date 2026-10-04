import { render } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import type { ReactElement } from 'react'
import { MemoryRouter } from 'react-router'
import { AppRoutes } from '@/app/routes'
import { AuthProvider } from '@/auth/auth-context'
import { TooltipProvider } from '@/components/ui/tooltip'
import { I18nProvider } from '@/i18n'
import type { Lang } from '@/i18n/define'
import type { Me, Roles } from '@/lib/types'
import { server } from '@/test/server'

const NO_ROLES: Roles = { admin: false, teacher: false, student: false }

/** 合成的登入者。 */
export function makeMe(roles: Partial<Roles> = {}, overrides: Partial<Me> = {}): Me {
  const r = { ...NO_ROLES, ...roles }
  return {
    id: '00000000-0000-4000-8000-000000000001',
    email: 'user@example.com',
    display_name: 'Test User',
    is_admin: r.admin,
    must_change_password: false,
    roles: r,
    ...overrides,
  }
}

/** 後端錯誤回應（只有 code 與 request_id，S-12.1）。 */
export function apiError(status: number, code: string, requestId = 'req-test-1') {
  return HttpResponse.json({ error: { code, request_id: requestId } }, { status })
}

/** `/api/me` 回傳指定使用者；傳 `null` 代表未登入（401）。 */
export function mockMe(me: Me | null) {
  server.use(http.get('/api/me', () => (me ? HttpResponse.json(me) : apiError(401, 'unauthorized'))))
}

type Options = { route?: string; lang?: Lang }

/** 把整個 App（不含 BrowserRouter）渲染在 MemoryRouter 裡；預設語言為 zh-TW 以便斷言文字。 */
export function renderApp(ui: ReactElement | null = null, { route = '/', lang = 'zh-TW' }: Options = {}) {
  return render(
    <I18nProvider initial={lang}>
      <AuthProvider>
        <TooltipProvider>
          <MemoryRouter initialEntries={[route]}>{ui ?? <AppRoutes />}</MemoryRouter>
        </TooltipProvider>
      </AuthProvider>
    </I18nProvider>,
  )
}
