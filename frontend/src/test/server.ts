import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'

/** 測試用的假後端（MSW）。各測試以 `server.use(...)` 加上自己需要的端點。 */
export const server = setupServer(
  // 沒開示範模式的後端：登入頁的示範帳號面板找不到清單就不顯示
  http.get('/api/demo/accounts', () => HttpResponse.json({ error: { code: 'not_found', request_id: 'test' } }, { status: 404 })),
)
