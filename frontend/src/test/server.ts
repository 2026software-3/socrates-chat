import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'

/** 測試用的假後端（MSW）。各測試以 `server.use(...)` 加上自己需要的端點。 */
export const server = setupServer(
  // 外框會顯示 AI 用量；沒有特別要測的測試一律回用量很低的合成值
  http.get('/api/me/usage', () => HttpResponse.json({ used: 0, limit: 200, window_secs: 86400, resets_in_secs: null })),
  // 沒有特別要測的測試：沒有公告、沒有維護
  http.get('/api/announcements', () => HttpResponse.json({ maintenance: false, announcements: [] })),
  // 沒開示範模式的後端：登入頁的示範帳號面板找不到清單就不顯示
  http.get('/api/demo/accounts', () => HttpResponse.json({ error: { code: 'not_found', request_id: 'test' } }, { status: 404 })),
)
