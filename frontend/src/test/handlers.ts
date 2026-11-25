import { http, HttpResponse } from 'msw'

/**
 * 所有測試共用的預設假端點（MSW）：Vitest 的 `server`（src/test/server.ts）與
 * Playwright 的瀏覽器 worker（src/test/browser.ts）都從這裡開始。
 */
export const defaultHandlers = [
  // 書庫的其他版本查找；沒有特別要測的測試：沒有候選
  http.get('/api/books/:id/editions', () => HttpResponse.json([])),
  // 外框會顯示 AI 用量；沒有特別要測的測試一律回用量很低的合成值
  http.get('/api/me/usage', () => HttpResponse.json({ used: 0, limit: 200, window_secs: 86400, resets_in_secs: null })),
  // 學生頂列會讀取點數餘額；沒有特別要測的測試回 0
  http.get('/api/points', () => HttpResponse.json({ balance: 0 })),
  // 沒有特別要測的測試：沒有公告、沒有維護
  http.get('/api/announcements', () => HttpResponse.json({ maintenance: false, announcements: [] })),
  http.get('/api/notifications', () => HttpResponse.json([])),
  // 教室的 AI 供應商（學生教室資訊）；沒有特別要測的測試：尚未設定
  http.get('/api/classrooms/:id/ai-provider', () => HttpResponse.json({ provider: null })),
  // 沒有特別要測單場雷達圖的測試：總結尚未成功，雷達圖面板不顯示
  http.get('/api/conversations/:id/radar', () =>
    HttpResponse.json({ status: 'summary_not_ready', retryable: false, scores: null, dimensions: null, rules_version: null, completed_at: null }),
  ),
  // 教室 AI 設定的模型欄位：沒有特別要測模型清單的測試，視為沒有 key（純文字輸入）
  http.get('/api/teacher/keys/models', ({ request }) =>
    HttpResponse.json({ provider: new URL(request.url).searchParams.get('provider'), status: 'no_key', models: [] }),
  ),
  // 後台首頁的服務健康度：沒有特別要測的測試回一個正常的合成值（有請求資料，避免與其他區塊的「尚無資料」撞字）
  http.get('/api/admin/health', () =>
    HttpResponse.json({
      status: 'healthy',
      period_days: 1,
      database: { status: 'healthy', response_time_ms: 2 },
      services: { total: 0, healthy: 0, unhealthy: 0, unknown: 0 },
      pool: { size: 1, idle: 1, max: 10 },
      process: { started_at: '2026-11-22T00:00:00Z', uptime_seconds: 3600, version: 'test' },
      maintenance: false,
      cleanup: { last_success_at: null },
      stats_available: true,
      requests: { total: 10, error_5xx_ratio: 0, p95_ms: 100 },
    }),
  ),
  // 沒開示範模式的後端：登入頁的示範帳號面板找不到清單就不顯示
  http.get('/api/demo/accounts', () => HttpResponse.json({ error: { code: 'not_found', request_id: 'test' } }, { status: 404 })),
]
