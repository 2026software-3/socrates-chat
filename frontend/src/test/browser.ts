import { http, HttpResponse } from 'msw'
import { setupWorker } from 'msw/browser'
import { E2E_ACCOUNTS, E2E_PASSWORD, E2E_SESSION_KEY, type E2eRole } from '@/test/e2e-accounts'
import { defaultHandlers } from '@/test/handlers'
import type { Available, Classroom, Me, Roles } from '@/lib/types'

/**
 * Playwright 用的瀏覽器假後端（只在 `vite --mode e2e` 啟動，見 src/main.tsx）。
 * 正式建置不會載入這個檔案。資料全部是合成的。
 */

const ROLES: Record<E2eRole, Roles> = {
  student: { student: true, teacher: false, admin: false },
  teacher: { student: false, teacher: true, admin: false },
  admin: { student: false, teacher: false, admin: true },
}

const NAMES: Record<E2eRole, string> = { student: '合成學生', teacher: '合成教師', admin: '合成管理者' }

function currentRole(): E2eRole | null {
  const v = localStorage.getItem(E2E_SESSION_KEY)
  return v === 'student' || v === 'teacher' || v === 'admin' ? v : null
}

function me(role: E2eRole): Me {
  return {
    id: `00000000-0000-4000-8000-00000000000${role === 'student' ? 1 : role === 'teacher' ? 2 : 3}`,
    email: E2E_ACCOUNTS[role],
    display_name: NAMES[role],
    language: 'zh-TW',
    is_admin: role === 'admin',
    must_change_password: false,
    can_change_password: true,
    roles: ROLES[role],
  }
}

const apiError = (status: number, code: string) => HttpResponse.json({ error: { code, request_id: 'e2e' } }, { status })

const E2E_FAIL_ONCE_KEY = 'socrates:e2e-fail-once'
const E2E_TRACEPARENT = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'

function consumeFailOnce(pathname: string) {
  if (localStorage.getItem(E2E_FAIL_ONCE_KEY) !== pathname) return null
  localStorage.removeItem(E2E_FAIL_ONCE_KEY)
  return HttpResponse.json(
    { error: { code: 'internal', request_id: 'e2e-err-id' } },
    { status: 500, headers: { 'x-request-id': 'e2e-err-id', traceparent: E2E_TRACEPARENT } },
  )
}

const CREATED = '2026-09-01T00:00:00Z'

const CLASSROOMS: Classroom[] = [
  { id: 'room-a', name: '合成 A 班', year_label: '115', description: '週二上午', is_archived: false, created_at: CREATED, updated_at: CREATED },
]

const CATALOG: Available = {
  activities: [
    {
      id: 'act-1',
      title: '什麼是正義？',
      description: '從城邦的例子出發，討論正義是否只是強者的利益。',
      topic_ids: ['topic-justice', 'topic-courage'],
      opens_at: null,
      closes_at: null,
      classroom_id: 'room-a',
      classroom_name: '合成 A 班',
      status: 'published',
      is_archived: false,
      created_at: CREATED,
      completed: false,
      topics: [
        { topic_id: 'topic-justice', title: '正義是強者的利益嗎？', conversation_id: null, conversation_status: null, summary_ready: false },
        { topic_id: 'topic-courage', title: '勇氣是一種知識嗎？', conversation_id: null, conversation_status: null, summary_ready: false },
      ],
    },
  ],
  topics: [{ id: 'topic-virtue', title: '美德可以教嗎？', description: '美諾篇的老問題：美德能不能像算術一樣被教會。', is_active: true }],
}

/** Playwright 的假後端：先比對這裡的端點，再落到 Vitest 共用的預設端點。 */
const e2eHandlers = [
  http.get('/api/me', () => {
    const role = currentRole()
    return role ? HttpResponse.json(me(role)) : apiError(401, 'unauthorized')
  }),
  http.post('/api/auth/login', async ({ request }) => {
    const body = (await request.json()) as { account?: string; password?: string }
    const role = (Object.keys(E2E_ACCOUNTS) as E2eRole[]).find((r) => E2E_ACCOUNTS[r] === body.account)
    if (!role || body.password !== E2E_PASSWORD) return apiError(401, 'invalid_credentials')
    localStorage.setItem(E2E_SESSION_KEY, role)
    return HttpResponse.json({ must_change_password: false })
  }),
  http.post('/api/auth/logout', () => {
    localStorage.removeItem(E2E_SESSION_KEY)
    return new HttpResponse(null, { status: 204 })
  }),
  // 學生首頁（選題）
  http.get('/api/catalog', () => HttpResponse.json(CATALOG)),
  http.get('/api/points', () => HttpResponse.json({ balance: 128 })),
  http.get('/api/tasks', () => HttpResponse.json([])),
  http.get('/api/conversations', () => HttpResponse.json({ items: [], next_cursor: null })),
  http.get('/api/conversations/e2e-conv-1', () =>
    HttpResponse.json({
      id: 'e2e-conv-1',
      activity_id: 'act-1',
      activity_title: '什麼是正義？',
      topic_id: 'topic-justice',
      title: '正義是強者的利益嗎？',
      description: '',
      language: 'zh-TW',
      status: 'active',
      stage: 1,
      turn_count: 1,
      converge_ready: false,
      created_at: CREATED,
      updated_at: CREATED,
      ended_at: null,
      is_archived: false,
      classroom_name: '合成 A 班',
      classroom_archived: false,
      messages: [
        {
          id: 'e2e-m1',
          role: 'student',
          content: '我認為正義不只是強者的利益。',
          source: 'text',
          question_type: null,
          created_at: CREATED,
        },
        {
          id: 'e2e-m2',
          role: 'ai',
          content: '那你認為判斷一件事是否正義的標準是什麼？',
          source: 'text',
          question_type: 'clarify',
          created_at: CREATED,
        },
      ],
      generating: false,
      voice_realtime: false,
      guiding_questions: [],
    }),
  ),
  // 教師首頁（我的教室）
  http.get('/api/classrooms', () => HttpResponse.json(CLASSROOMS)),
  // 管理者首頁
  http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [E2E_ACCOUNTS.teacher] })),
  http.get('/api/dashboard/class', () =>
    HttpResponse.json({ masked: true, students_total: 1, students_participating: 0, completed: 0, activities: [] }),
  ),
  http.get('/api/topics', () => HttpResponse.json(CATALOG.topics)),
  http.get('/api/admin/status', ({ request }) =>
    HttpResponse.json({
      service: 'ok',
      database: 'ok',
      period: new URL(request.url).searchParams.get('period') ?? '24h',
      diagnostics_available: true,
      statistics: { ai_status: 'ok', ai_calls: 0, ai_failures: 0, ai_invalid_outputs: 0, login_failures: 0, account_locks: 0 },
      recent_errors: [],
    }),
  ),
  http.get('/api/admin/diagnostics', ({ request }) => {
    const failed = consumeFailOnce('/api/admin/diagnostics')
    if (failed) return failed
    const params = new URL(request.url).searchParams
    const kind = params.get('kind')
    const failureClass = params.get('failure_class')
    const requestId = params.get('request_id')
    const all = [
      {
        occurred_at: '2026-11-02T00:45:00Z',
        request_id: '11111111-1111-4111-8111-111111111111',
        kind: 'ai',
        failure_class: 'rate_limited',
        function: 'chat_reply',
        route: '/api/conversations/{id}/messages',
        status: 200,
        error_code: 'ai_unavailable',
        user_id: '22222222-2222-4222-8222-222222222222',
        classroom_id: '33333333-3333-4333-8333-333333333333',
        duration_ms: 420,
        retry_count: 1,
        model: 'synthetic-model',
      },
      {
        occurred_at: '2026-11-02T00:30:00Z',
        request_id: '44444444-4444-4444-8444-444444444444',
        kind: 'login',
        failure_class: 'invalid_credentials',
        function: 'password_login',
        route: '/api/auth/login',
        status: 401,
        error_code: 'invalid_credentials',
        user_id: null,
        classroom_id: null,
        duration_ms: null,
        retry_count: null,
        model: null,
      },
    ]
    const items = all.filter(
      (item) =>
        (!kind || item.kind === kind) &&
        (!failureClass || item.failure_class === failureClass) &&
        (!requestId || item.request_id === requestId),
    )
    return HttpResponse.json({ items, next_cursor: null })
  }),
  http.get('/api/admin/services', ({ request }) => {
    const days = Number(new URL(request.url).searchParams.get('period_days') ?? '1')
    return HttpResponse.json({
      period_days: Number.isFinite(days) ? days : 1,
      history_available: true,
      services: [
        {
          name: 'database',
          label: 'PostgreSQL',
          impact: { 'zh-TW': '全站', en: 'Entire system' },
          target: null,
          status: 'healthy',
          failure: null,
          response_time_ms: 3,
          checked_at: '2026-11-02T00:59:30Z',
          last_healthy_at: '2026-11-02T00:59:30Z',
          status_since: '2026-11-01T01:00:00Z',
          availability: 1,
          recent_events: [{ status: 'healthy', occurred_at: '2026-11-01T01:00:00Z', failure: null }],
        },
        {
          name: 'tts',
          label: { 'zh-TW': '語音合成', en: 'Speech synthesis' },
          impact: { 'zh-TW': '語音回覆', en: 'Voice reply' },
          target: 'cascade-tts:8100/health',
          status: 'unhealthy',
          failure: 'timeout',
          response_time_ms: null,
          checked_at: '2026-11-02T00:59:30Z',
          last_healthy_at: '2026-11-02T00:30:00Z',
          status_since: '2026-11-02T00:45:00Z',
          availability: 0.96,
          recent_events: [
            { status: 'unhealthy', occurred_at: '2026-11-02T00:45:00Z', failure: 'timeout' },
            { status: 'healthy', occurred_at: '2026-11-01T01:00:00Z', failure: null },
          ],
        },
      ],
      voice_nodes: [],
    })
  }),
  http.post('/api/admin/services/check', async ({ request }) => {
    const body = (await request.json()) as { name?: string }
    return HttpResponse.json({
      name: body.name ?? 'tts',
      label: body.name === 'database' ? 'PostgreSQL' : { 'zh-TW': '語音合成', en: 'Speech synthesis' },
      impact: body.name === 'database' ? { 'zh-TW': '全站' } : { 'zh-TW': '語音回覆' },
      target: body.name === 'database' ? null : 'cascade-tts:8100/health',
      status: 'healthy',
      failure: null,
      response_time_ms: 14,
      checked_at: '2026-11-02T01:00:00Z',
      last_healthy_at: '2026-11-02T01:00:00Z',
      status_since: '2026-11-02T01:00:00Z',
      availability: null,
      recent_events: [],
    })
  }),
  http.get('/api/admin/metrics', () =>
    consumeFailOnce('/api/admin/metrics') ??
    new HttpResponse(
      [
        '# TYPE socrates_build_info gauge',
        'socrates_build_info{version="e2e-build"} 1',
        '# TYPE socrates_process_uptime_seconds gauge',
        'socrates_process_uptime_seconds 3600',
        '# TYPE socrates_maintenance_mode gauge',
        'socrates_maintenance_mode 0',
        '# TYPE socrates_db_pool_connections gauge',
        'socrates_db_pool_connections{state="active"} 2',
        'socrates_db_pool_connections{state="idle"} 8',
        'socrates_db_pool_connections{state="max"} 10',
        'socrates_db_pool_connections{state="size"} 10',
        '# TYPE socrates_http_requests_in_flight gauge',
        'socrates_http_requests_in_flight 2',
        '# TYPE socrates_live_replies_active gauge',
        'socrates_live_replies_active 1',
        '# TYPE socrates_voice_sessions_active gauge',
        'socrates_voice_sessions_active 0',
        '# TYPE socrates_service_up gauge',
        'socrates_service_up{service="database"} 1',
        'socrates_service_up{service="tts"} 0',
        '# TYPE socrates_http_requests_total counter',
        'socrates_http_requests_total{method="GET",route="/api/catalog",status_class="2xx"} 84',
        'socrates_http_requests_total{method="POST",route="/api/conversations/{id}/messages",status_class="2xx"} 16',
        '# TYPE socrates_http_request_duration_seconds histogram',
        'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="0.1"} 84',
        'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="+Inf"} 84',
        'socrates_http_request_duration_seconds_sum{method="GET",route="/api/catalog"} 4.2',
        'socrates_http_request_duration_seconds_count{method="GET",route="/api/catalog"} 84',
        '# TYPE socrates_ai_requests_total counter',
        'socrates_ai_requests_total{function="chat_reply",outcome="ok"} 16',
        '# TYPE socrates_background_jobs_total counter',
        'socrates_background_jobs_total{job="summary",outcome="ok"} 5',
      ].join('\n'),
      { status: 200, headers: { 'content-type': 'text/plain; version=0.0.4; charset=utf-8' } },
    ),
  ),
]

export const worker = setupWorker(...e2eHandlers, ...defaultHandlers)

/** 啟動 worker；沒有假資料的 /api 請求在 console 報錯，tests/e2e/fixtures.ts 會讓測試失敗。 */
export function startWorker() {
  return worker.start({
    quiet: true,
    onUnhandledRequest(request, print) {
      if (new URL(request.url).pathname.startsWith('/api/')) print.error()
    },
  })
}
