import { http, HttpResponse } from 'msw'
import { setupWorker } from 'msw/browser'
import { E2E_ACCOUNTS, E2E_PASSWORD, E2E_SESSION_KEY, type E2eRole } from '@/test/e2e-accounts'
import { defaultHandlers } from '@/test/handlers'
import { stanceRadarHandlers } from '@/test/radar-stance-handlers'
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
]

export const worker = setupWorker(...stanceRadarHandlers, ...e2eHandlers, ...defaultHandlers)

/** 啟動 worker；沒有假資料的 /api 請求在 console 報錯，tests/e2e/fixtures.ts 會讓測試失敗。 */
export function startWorker() {
  return worker.start({
    quiet: true,
    onUnhandledRequest(request, print) {
      if (new URL(request.url).pathname.startsWith('/api/')) print.error()
    },
  })
}
