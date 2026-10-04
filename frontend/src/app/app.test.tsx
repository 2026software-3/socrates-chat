import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

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
    expect(screen.queryByRole('navigation', { name: 'main' })).not.toBeInTheDocument()
  })

  it('does not let a roleless account reach feature pages by URL', async () => {
    mockMe(makeMe())
    renderApp(null, { route: '/teacher/roster' })
    expect(await screen.findByRole('heading', { name: '尚未開通' })).toBeInTheDocument()
  })

  it('takes students straight to topic selection from the home page', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/available', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '想從哪個問題開始思考？' })).toBeInTheDocument()
  })

  const emptyClass = {
    masked: false,
    students_total: 0,
    students_participating: 0,
    completed: 0,
    positions: {},
    frameworks: {},
    topics: [],
  }

  it('takes teachers to the class dashboard — they do not take part in discussions', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/dashboard/class', () => HttpResponse.json(emptyClass)))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '班上論點分布' })).toBeInTheDocument()
    const nav = screen.getByRole('navigation', { name: 'main' })
    for (const name of ['選擇題目', '我的對話', '我的儀表板']) {
      expect(within(nav).queryByRole('link', { name })).not.toBeInTheDocument()
    }
  })

  it('sends a teacher who types a student URL back to the dashboard', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/dashboard/class', () => HttpResponse.json(emptyClass)))
    renderApp(null, { route: '/conversations' })
    expect(await screen.findByRole('heading', { name: '班上論點分布' })).toBeInTheDocument()
  })

  it('shows student navigation only to students', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: 'main' })
    expect(within(nav).getByRole('link', { name: '選擇題目' })).toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: '我的對話' })).toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: '學生帳號' })).not.toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: '教師管理' })).not.toBeInTheDocument()
  })

  it('blocks a student who types a teacher URL', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/teacher/roster' })
    expect(await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })).toBeInTheDocument()
  })

  it('shows teacher links to teachers but not admin links', async () => {
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: 'main' })
    expect(within(nav).getByRole('link', { name: '學生帳號' })).toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: '學生總結' })).toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: '教師管理' })).not.toBeInTheDocument()
  })

  function mockOpsData() {
    server.use(
      http.get('/api/admin/teachers', () => HttpResponse.json({ emails: ['t@example.com'] })),
      http.get('/api/roster', () => HttpResponse.json({ emails: ['a@example.com', 'b@example.com'] })),
      http.get('/api/students', () =>
        HttpResponse.json(
          ['a@example.com', 'b@example.com'].map((email) => ({
            email,
            display_name: null,
            has_account: true,
            disabled: false,
            completed_conversations: 0,
          })),
        ),
      ),
      http.get('/api/admin/topics', () => HttpResponse.json([])),
    )
  }

  it('gives an admin-only account just the operations pages', async () => {
    mockMe(makeMe({ admin: true }))
    mockOpsData()
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '系統維運' })).toBeInTheDocument()
    const nav = screen.getByRole('navigation', { name: 'main' })
    expect(within(nav).getAllByRole('link').map((a) => a.textContent)).toEqual(['總覽', '教師管理', '修課名單', '題目庫'])
    expect(await screen.findByText('2')).toBeInTheDocument()
  })

  it('keeps an admin-only account out of discussion and teacher pages', async () => {
    mockMe(makeMe({ admin: true }))
    mockOpsData()
    renderApp(null, { route: '/conversations' })
    expect(await screen.findByRole('heading', { name: '系統維運' })).toBeInTheDocument()
  })

  it('keeps the normal navigation for an admin who is also a teacher', async () => {
    mockMe(makeMe({ admin: true, teacher: true }))
    server.use(http.get('/api/dashboard/class', () => HttpResponse.json(emptyClass)))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: 'main' })
    for (const name of ['教師管理', '題目庫', '學生帳號', '班上分布']) {
      expect(within(nav).getByRole('link', { name })).toBeInTheDocument()
    }
    expect(within(nav).queryByRole('link', { name: '選擇題目' })).not.toBeInTheDocument()
  })

  it('switches language without reloading', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/available', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '想從哪個問題開始思考？' })
    await userEvent.selectOptions(screen.getByLabelText('語言'), 'English')
    expect(await screen.findByRole('heading', { name: 'Which question will you think about today?' })).toBeInTheDocument()
    expect(document.documentElement.lang).toBe('en')
    expect(localStorage.getItem('lang')).toBe('en')
  })

  it('shows the disabled page when the account has been disabled', async () => {
    server.use(http.get('/api/me', () => apiError(403, 'account_disabled')))
    renderApp(null, { route: '/' })
    expect(await screen.findByText('帳號已停用')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '登出' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'main' })).not.toBeInTheDocument()
  })

  it('shows an error with retry when /api/me fails with a server error', async () => {
    server.use(http.get('/api/me', () => apiError(500, 'internal')))
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    server.use(http.get('/api/me', () => HttpResponse.json(makeMe({ student: true }))))
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByRole('navigation', { name: 'main' })).toBeInTheDocument()
  })

  it('logs out and returns to the login page', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.post('/api/auth/logout', () => new HttpResponse(null, { status: 204 })))
    renderApp(null, { route: '/' })
    await userEvent.click(await screen.findByRole('button', { name: '登出' }))
    await waitFor(() => expect(screen.getByRole('button', { name: '使用 Google 登入' })).toBeInTheDocument())
  })

  it('shows a not-found page for unknown routes', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/nope' })
    expect(await screen.findByRole('heading', { name: '找不到資料。' })).toBeInTheDocument()
  })
})
