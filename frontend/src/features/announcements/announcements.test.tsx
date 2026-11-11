import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const NOTICE = { id: 'a1', kind: 'general', text: '週五晚上 10 點起停機更新' }
const OTHER = { id: 'a2', kind: 'downtime', text: '預計 30 分鐘後恢復' }

/** 回傳指定公告，並記錄請求的 lang 參數。 */
function mockAnnouncements(announcements: object[], maintenance = false) {
  const langs: (string | null)[] = []
  server.use(
    http.get('/api/announcements', ({ request }) => {
      langs.push(new URL(request.url).searchParams.get('lang'))
      return HttpResponse.json({ maintenance, announcements })
    }),
  )
  return langs
}

describe('announcement banner', () => {
  it('shows active announcements on the login page, in the current language', async () => {
    mockMe(null)
    const langs = mockAnnouncements([NOTICE])
    renderApp(null, { route: '/login', lang: 'en' })
    expect(await screen.findByText(NOTICE.text)).toBeInTheDocument()
    expect(langs).toContain('en')
  })

  it('shows announcements after login, above the page content', async () => {
    mockMe(makeMe({ student: true }))
    mockAnnouncements([NOTICE, OTHER])
    renderApp(null, { route: '/' })
    const region = await screen.findByRole('region', { name: '公告' })
    expect(within(region).getByText(NOTICE.text)).toBeInTheDocument()
    expect(within(region).getByText(OTHER.text)).toBeInTheDocument()
  })

  it('remembers a dismissed announcement by id, and keeps showing the others', async () => {
    mockMe(null)
    mockAnnouncements([NOTICE, OTHER])
    const first = renderApp(null, { route: '/login' })
    await screen.findByText(NOTICE.text)
    await userEvent.click(screen.getAllByRole('button', { name: '關閉公告' })[0])
    expect(screen.queryByText(NOTICE.text)).not.toBeInTheDocument()
    expect(screen.getByText(OTHER.text)).toBeInTheDocument()
    first.unmount()

    renderApp(null, { route: '/login' })
    expect(await screen.findByText(OTHER.text)).toBeInTheDocument()
    expect(screen.queryByText(NOTICE.text)).not.toBeInTheDocument()
  })

  it('shows nothing, without an error, when the announcements cannot be loaded', async () => {
    mockMe(null)
    server.use(http.get('/api/announcements', () => apiError(500, 'internal')))
    renderApp(null, { route: '/login' })
    expect(await screen.findByLabelText('學號或電子郵件')).toBeInTheDocument()
    await waitFor(() => expect(screen.queryByRole('region', { name: '公告' })).not.toBeInTheDocument())
    expect(screen.queryByText('系統發生錯誤，請稍後再試。')).not.toBeInTheDocument()
  })
})

describe('maintenance mode', () => {
  it('shows the maintenance page with the announcements when the API answers maintenance', async () => {
    server.use(http.get('/api/me', () => apiError(503, 'maintenance')))
    mockAnnouncements([NOTICE])
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('heading', { name: '系統維護中' })).toBeInTheDocument()
    expect(await screen.findByText(NOTICE.text)).toBeInTheDocument()
    // 不洩漏時間或原因，只有固定文字
    expect(screen.getByText(/暫時無法使用/)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '管理者登入' })).toHaveAttribute('href', '/login')
  })

  it('leaves the maintenance page once the API answers again', async () => {
    let maintenance = true
    server.use(
      http.get('/api/me', () => (maintenance ? apiError(503, 'maintenance') : HttpResponse.json(makeMe({ student: true })))),
    )
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '系統維護中' })
    maintenance = false
    await userEvent.click(screen.getByRole('button', { name: '重新檢查' }))
    expect(await screen.findByRole('navigation', { name: 'main' })).toBeInTheDocument()
  })

  it('tells users on the login page that only admins can sign in', async () => {
    mockMe(null)
    mockAnnouncements([], true)
    renderApp(null, { route: '/login' })
    expect(await screen.findByText('系統維護中，目前僅管理者可登入。')).toBeInTheDocument()
  })

  it('translates the refused password login', async () => {
    mockMe(null)
    server.use(http.post('/api/auth/login', () => apiError(503, 'maintenance')))
    renderApp(null, { route: '/login' })
    await userEvent.type(await screen.findByLabelText('學號或電子郵件'), 'a@example.com')
    await userEvent.type(screen.getByLabelText('密碼'), 'password-123')
    await userEvent.click(screen.getByRole('button', { name: '登入' }))
    expect(await screen.findByText('系統維護中，請稍後再試。')).toBeInTheDocument()
  })

  it('shows the redirect error from a refused Google login', async () => {
    mockMe(null)
    renderApp(null, { route: '/login?error=maintenance' })
    expect(await screen.findByRole('alert')).toHaveTextContent('系統維護中，目前僅管理者可登入。')
  })
})
