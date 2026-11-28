import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const emptyClass = {
  masked: false,
  students_total: 0,
  students_participating: 0,
  completed: 0,
  activities: [],
}

const opsStatus = {
  service: 'ok',
  database: 'ok',
  period: '24h',
  diagnostics_available: true,
  statistics: null,
  recent_errors: [],
}

describe('app shell', () => {
  it('sends anonymous visitors to the login page', async () => {
    mockMe(null)
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('button', { name: '使用 Google 登入' })).toBeInTheDocument()
  })

  it('shows a translated message for login errors from the backend', async () => {
    mockMe(null)
    renderApp(null, { route: '/login?error=email_not_verified' })
    expect(await screen.findByRole('alert')).toHaveTextContent('你的 Google 電子郵件尚未驗證。')
  })

  it('falls back to a generic login error for unknown codes', async () => {
    mockMe(null)
    renderApp(null, { route: '/login?error=whatever' })
    expect(await screen.findByRole('alert')).toHaveTextContent('登入失敗，請稍後再試。')
  })

  it('shows only the "not enabled yet" page to accounts without a role', async () => {
    mockMe(makeMe({}, { email: 'outsider@example.com' }))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '尚未開通' })).toBeInTheDocument()
    expect(screen.getByText('目前登入的帳號：outsider@example.com')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: '主導覽' })).not.toBeInTheDocument()
  })

  it('lets a roleless account re-check its enrolment or switch account from the pending page', async () => {
    mockMe(makeMe({}, { email: 'outsider@example.com' }))
    server.use(http.post('/api/auth/logout', () => new HttpResponse(null, { status: 204 })))
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '尚未開通' })

    // 被加入名單後，重新檢查會重讀登入者並放行
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })))
    await userEvent.click(screen.getByRole('button', { name: '我已被加入，重新檢查' }))
    expect(await screen.findByRole('heading', { name: '選擇題目' })).toBeInTheDocument()
  })

  it('offers a way to switch account from the pending page', async () => {
    mockMe(makeMe({}, { email: 'outsider@example.com' }))
    server.use(http.post('/api/auth/logout', () => new HttpResponse(null, { status: 204 })))
    renderApp(null, { route: '/' })
    await userEvent.click(await screen.findByRole('button', { name: '改用其他帳號登入' }))
    expect(await screen.findByRole('button', { name: '使用 Google 登入' })).toBeInTheDocument()
  })

  it('shows role-appropriate sidebar footnotes for teachers and admin-only accounts', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    const first = renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '我的教室' })
    expect(screen.getByText('教師與助教權限相同。')).toBeInTheDocument()
    first.unmount()

    mockMe(makeMe({ admin: true }))
    mockOpsData()
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '系統維運' })
    expect(screen.getByText('訊息內容一律顯示為 message。')).toBeInTheDocument()
  })

  it('shows the student roster footnote to students', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '選擇題目' })
    expect(screen.getByText('名單外的帳號看不到這張地圖。')).toBeInTheDocument()
  })

  it('does not let a roleless account reach feature pages by URL', async () => {
    mockMe(makeMe())
    renderApp(null, { route: '/teacher/roster' })
    expect(await screen.findByRole('heading', { name: '尚未開通' })).toBeInTheDocument()
  })

  it('takes students straight to topic selection from the home page', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '選擇題目' })).toBeInTheDocument()
  })

  it('takes teachers to their classrooms — they do not take part in discussions', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '我的教室' })).toBeInTheDocument()
    const nav = screen.getByRole('navigation', { name: '主導覽' })
    for (const name of ['個人練習', '我的對話', '個人檔案']) {
      expect(within(nav).queryByRole('link', { name })).not.toBeInTheDocument()
    }
  })

  it('sends a teacher who types a student URL back to the classrooms', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/conversations' })
    expect(await screen.findByRole('heading', { name: '我的教室' })).toBeInTheDocument()
  })

  it('shows student navigation only to students', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: '主導覽' })
    expect(within(nav).getByRole('link', { name: /我的教室/ })).toHaveAttribute('href', '/')
    expect(within(nav).getByRole('link', { name: /個人練習/ })).toHaveAttribute('href', '/#practice')
    expect(within(nav).getByRole('link', { name: /我的對話/ })).toHaveAttribute('href', '/conversations')
    expect(within(nav).getByRole('link', { name: /點數與任務/ })).toHaveAttribute('href', '/tasks')
    expect(within(nav).getByRole('link', { name: /我的蘇格拉底/ })).toHaveAttribute('href', '/parts')
    expect(within(nav).getByRole('link', { name: /個人檔案/ })).toHaveAttribute('href', '/account')
    expect(within(nav).queryByRole('link', { name: /學生帳號/ })).not.toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: /教師管理/ })).not.toBeInTheDocument()
  })

  it('blocks a student who types a teacher URL', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/teacher/roster' })
    expect(await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })).toBeInTheDocument()
  })

  it.each(['/admin/diagnostics', '/admin'])(
    'blocks a teacher who types the admin URL %s, without asking the backend for diagnostics',
    async (route) => {
      mockMe(makeMe({ teacher: true }))
      const requested: string[] = []
      server.use(
        http.get('/api/admin/status', ({ request }) => {
          requested.push(new URL(request.url).pathname)
          return HttpResponse.json({})
        }),
        http.get('/api/admin/diagnostics', ({ request }) => {
          requested.push(new URL(request.url).pathname)
          return HttpResponse.json({ items: [], next_cursor: null })
        }),
      )
      renderApp(null, { route })

      expect(await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })).toBeInTheDocument()
      expect(screen.queryByRole('heading', { name: '系統維運' })).not.toBeInTheDocument()
      expect(screen.queryByRole('heading', { name: '系統概況' })).not.toBeInTheDocument()
      expect(screen.queryByRole('heading', { name: '診斷紀錄' })).not.toBeInTheDocument()
      expect(requested).toEqual([])
    },
  )

  it('shows teacher links to teachers but not admin links', async () => {
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: '主導覽' })
    expect(within(nav).getByRole('link', { name: /教室管理/ })).toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: /題目庫/ })).toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: /共用書庫/ })).toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: /學生帳號/ })).toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: /學生總結/ })).toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: /教師管理/ })).not.toBeInTheDocument()
  })

  it('sends the retired dashboard and activities URLs to the classrooms', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/teacher/dashboard' })
    expect(await screen.findByRole('heading', { name: '我的教室' })).toBeInTheDocument()
  })

  it('lets a teacher open classroom management', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/teacher/classrooms' })
    expect(await screen.findByRole('heading', { name: '我的教室' })).toBeInTheDocument()
  })

  function mockOpsData() {
    server.use(
      http.get('/api/admin/teachers', () => HttpResponse.json({ emails: ['t@example.com'] })),
      http.get('/api/dashboard/class', () => HttpResponse.json({ ...emptyClass, masked: true, students_total: 2 })),
      http.get('/api/topics', () => HttpResponse.json([])),
      http.get('/api/admin/status', () => HttpResponse.json(opsStatus)),
    )
  }

  it('gives an admin-only account the 7 Vedute admin areas', async () => {
    mockMe(makeMe({ admin: true }))
    mockOpsData()
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '系統維運' })).toBeInTheDocument()
    const nav = screen.getByRole('navigation', { name: '主導覽' })
    expect(within(nav).getAllByRole('link').map((a) => a.getAttribute('href'))).toEqual([
      '/admin',
      '/admin/classrooms',
      '/admin/teachers',
      '/admin/books',
      '/admin/topics',
      '/admin/system-ai',
      '/admin/diagnostics',
    ])
    expect(await screen.findByText('2')).toBeInTheDocument()
  })

  it('keeps an admin-only account out of discussion and teacher pages', async () => {
    mockMe(makeMe({ admin: true }))
    mockOpsData()
    renderApp(null, { route: '/conversations' })
    expect(await screen.findByRole('heading', { name: '系統維運' })).toBeInTheDocument()
  })

  it('follows the current path for an admin who is also a teacher and offers workspace switching in the account menu', async () => {
    mockMe(makeMe({ admin: true, teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: '主導覽' })
    expect(within(nav).getByRole('link', { name: /教室管理/ })).toHaveAttribute('href', '/teacher/classrooms')
    expect(within(nav).getByRole('link', { name: /學生帳號/ })).toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: /個人練習/ })).not.toBeInTheDocument()

    // Account menu lets the multi-role user switch to the admin workspace
    await userEvent.click(within(screen.getByRole('banner')).getByRole('button', { name: 'Test User' }))
    expect(screen.getByRole('link', { name: '切換至管理者工作區' })).toHaveAttribute('href', '/admin')
  })

  it('switches language from the account menu without reloading', async () => {
    const me = makeMe({ student: true }, { language: 'zh-TW' })
    mockMe(me)
    const updates: unknown[] = []
    server.use(
      http.patch('/api/me', async ({ request }) => {
        updates.push(await request.json())
        return HttpResponse.json({ ...me, language: 'en' })
      }),
    )
    server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '選擇題目' })
    await userEvent.click(within(screen.getByRole('banner')).getByRole('button', { name: 'Test User' }))
    await userEvent.selectOptions(screen.getByLabelText('語言'), 'English')
    expect(await screen.findByRole('heading', { name: 'Choose a topic' })).toBeInTheDocument()
    await waitFor(() => expect(updates).toEqual([{ language: 'en' }]))
    // <html lang> 由 I18nProvider 的 useEffect 設定：畫面換了語言之後才會更新，要等它而不是立刻斷言
    await waitFor(() => expect(document.documentElement.lang).toBe('en'))
    expect(localStorage.getItem('lang')).toBe('en')
  })

  it('uses the authenticated account language instead of a stale device preference', async () => {
    localStorage.setItem('lang', 'en')
    mockMe(makeMe({ student: true }, { language: 'es' }))
    server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: 'Elegir un tema' })).toBeInTheDocument()
    // 同上：<html lang> 在被動效果裡更新，畫面出現西班牙文時它不一定已經更新（CI 上偶發看到 zh-TW）
    await waitFor(() => expect(document.documentElement.lang).toBe('es'))
  })

  it('shows the disabled page when the account has been disabled', async () => {
    server.use(http.get('/api/me', () => apiError(403, 'account_disabled')))
    renderApp(null, { route: '/' })
    expect(await screen.findByText('帳號已停用')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '返回登入' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: '主導覽' })).not.toBeInTheDocument()
  })

  it('shows an error with retry when /api/me fails with a server error', async () => {
    server.use(http.get('/api/me', () => apiError(500, 'internal')))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    server.use(http.get('/api/me', () => HttpResponse.json(makeMe({ student: true }))))
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByRole('navigation', { name: '主導覽' })).toBeInTheDocument()
  })

  it('logs out from the account menu and returns to the login page', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.post('/api/auth/logout', () => new HttpResponse(null, { status: 204 })))
    renderApp(null, { route: '/' })
    await userEvent.click(await within(await screen.findByRole('banner')).findByRole('button', { name: 'Test User' }))
    await userEvent.click(screen.getByRole('button', { name: '登出' }))
    await waitFor(() => expect(screen.getByRole('button', { name: '使用 Google 登入' })).toBeInTheDocument())
  })

  it('shows a not-found page for unknown routes', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/nope' })
    expect(await screen.findByRole('heading', { name: '找不到資料。' })).toBeInTheDocument()
  })
})

// jsdom 不排版，只能檢查決定「固定在視窗內」的 class；實際效果要在瀏覽器確認
describe('sidebar stays in view on long pages', () => {
  it.each([
    ['student', { student: true }, '/'],
    ['teacher', { teacher: true }, '/teacher/classrooms'],
    ['admin', { admin: true }, '/admin'],
  ])('keeps the desktop sidebar of a %s pinned under the 52px topbar', async (_name, roles, route) => {
    mockMe(makeMe(roles))
    server.use(
      http.get('/api/classrooms', () => HttpResponse.json([])),
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })),
      http.get('/api/admin/status', () => HttpResponse.json(opsStatus)),
      // 服務健康度交給預設的假端點（不是 `[]`）
      http.get('/api/admin/*', ({ request }) => (new URL(request.url).pathname === '/api/admin/health' ? undefined : HttpResponse.json([]))),
      // 首頁各自會讀的資料；沒有處理的請求在卸載時被中止，會變成測試結束後的未處理拒絕
      http.get('/api/dashboard/class', () => HttpResponse.json(emptyClass)),
      http.get('/api/topics', () => HttpResponse.json([])),
    )
    renderApp(null, { route })
    const nav = await screen.findByRole('navigation', { name: '主導覽' })
    const aside = nav.closest('aside')
    expect(aside).not.toBeNull()
    expect(aside).toHaveClass('sticky', 'top-13', 'h-[calc(100svh-52px)]', 'overflow-y-auto')
  })
})

describe('app shell: AI usage', () => {
  it("shows the signed-in user's AI usage in the sidebar", async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/me/usage', () => HttpResponse.json({ used: 5, limit: 200, window_secs: 86400, resets_in_secs: 100 })))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('meter', { name: 'AI 用量' })).toHaveAttribute('aria-valuenow', '5')
  })

  it('shows it for teachers too', async () => {
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('meter', { name: 'AI 用量' })).toBeInTheDocument()
  })
})
