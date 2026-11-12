import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Me, Roles } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

/** `/account`（F-37、S-18）：教師與管理者更改密碼的地方，所有已開通的角色都進得去；全部使用合成資料。 */

const emptyDashboard = { total_conversations: 0, completed: 0, total_turns: 0, recent: [] }

/** 學生的帳號頁會讀個人儀表板；教師與管理者不會（見下面的測試）。 */
function mockDashboard(data: object = emptyDashboard) {
  server.use(http.get('/api/dashboard/me', () => HttpResponse.json(data)))
}

const open = async (roles: Partial<Roles>, overrides: Partial<Me> = {}) => {
  mockMe(makeMe(roles, overrides))
  renderApp(null, { route: '/account' })
  await screen.findByRole('heading', { level: 1, name: '帳號' })
}

/** 「更改密碼」是頁面上的一個按鈕，表單在按下後的彈出視窗裡（S-18）。 */
const changeButton = () => screen.getByRole('button', { name: '更改密碼' })
const noChangeButton = () => expect(screen.queryByRole('button', { name: '更改密碼' })).not.toBeInTheDocument()
async function openDialog() {
  await userEvent.click(changeButton())
  return within(await screen.findByRole('dialog', { name: '更改密碼' }))
}

describe('the account page', () => {
  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
    ['a teacher who is also an admin', { teacher: true, admin: true }],
  ])('offers only the change-password form to %s', async (_label, roles) => {
    await open(roles)
    // 平常只有一個按鈕，不攤開表單
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    const form = await openDialog()
    expect(form.getByLabelText('目前密碼')).toHaveAttribute('autocomplete', 'current-password')
    expect(form.getByLabelText('新密碼')).toBeInTheDocument()
    expect(form.getByLabelText('確認新密碼')).toBeInTheDocument()
    // 教師與管理者的帳號頁只放更改密碼：顯示名稱、Google 連結與儀表板內容是學生的
    expect(screen.queryByLabelText('顯示名稱')).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Google 帳號' })).not.toBeInTheDocument()
    expect(screen.queryByText('已完成對話')).not.toBeInTheDocument()
  })

  it('does not send an admin-only account back to the operations home', async () => {
    await open({ admin: true })
    expect(screen.queryByRole('heading', { name: '系統維運' })).not.toBeInTheDocument()
    expect(changeButton()).toBeInTheDocument()
  })

  it('changes the password from /account', async () => {
    const posts: unknown[] = []
    server.use(
      http.post('/api/auth/change-password', async ({ request }) => {
        posts.push(await request.json())
        return new HttpResponse(null, { status: 204 })
      }),
    )
    await open({ teacher: true })
    const form = await openDialog()
    await userEvent.type(form.getByLabelText('目前密碼'), 'old-pass-123')
    await userEvent.type(form.getByLabelText('新密碼'), 'new-pass-456')
    await userEvent.type(form.getByLabelText('確認新密碼'), 'new-pass-456')
    await userEvent.click(form.getByRole('button', { name: '更改密碼' }))
    // 成功後彈出視窗關閉，確認訊息留在頁面上
    expect(await screen.findByRole('status')).toHaveTextContent('密碼已更改，其他裝置已登出。')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(posts).toEqual([{ current_password: 'old-pass-123', new_password: 'new-pass-456' }])
  })

  it('shows localized backend errors', async () => {
    server.use(http.post('/api/auth/change-password', () => apiError(400, 'invalid_current_password')))
    await open({ admin: true })
    const form = await openDialog()
    await userEvent.type(form.getByLabelText('目前密碼'), 'wrong-pass-1')
    await userEvent.type(form.getByLabelText('新密碼'), 'new-pass-456')
    await userEvent.type(form.getByLabelText('確認新密碼'), 'new-pass-456')
    await userEvent.click(form.getByRole('button', { name: '更改密碼' }))
    expect(await form.findByRole('alert')).toHaveTextContent('目前密碼不正確。')
  })

  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
  ])('only explains that %s without a built-in password cannot change it here', async (_label, roles) => {
    await open(roles, { can_change_password: false })
    expect(screen.getByText('這個帳號不能在這裡更改密碼。')).toBeInTheDocument()
    noChangeButton()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('shows only the "not enabled yet" page to an account without a role', async () => {
    mockMe(makeMe({}, { email: 'outsider@example.com' }))
    renderApp(null, { route: '/account' })
    expect(await screen.findByRole('heading', { name: '尚未開通' })).toBeInTheDocument()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
  })

  it('sends anonymous visitors to the login page', async () => {
    mockMe(null)
    renderApp(null, { route: '/account' })
    expect(await screen.findByRole('button', { name: '登入' })).toBeInTheDocument()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
  })

  it('is not in the navigation list: it is reached from the avatar at the top right', async () => {
    await open({ teacher: true })
    const nav = screen.getByRole('navigation', { name: 'main' })
    expect(within(nav).queryByRole('link', { name: '帳號' })).not.toBeInTheDocument()
  })
})

describe('the avatar at the top right', () => {
  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
    ['a teacher who is also an admin', { teacher: true, admin: true }],
  ])('links %s with a built-in password to /account', async (_label, roles) => {
    await open(roles)
    const avatar = screen.getByRole('link', { name: 'Test User' })
    expect(avatar).toHaveAttribute('href', '/account')
    expect(avatar).toHaveTextContent('T')
    expect(within(screen.getByRole('banner')).getByRole('link', { name: 'Test User' })).toBe(avatar)
  })

  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
  ])('does not link the avatar of %s when the account cannot change its password', async (_label, roles) => {
    await open(roles, { can_change_password: false })
    expect(screen.getByText('Test User')).toBeInTheDocument()
    expect(screen.getByRole('img', { name: 'Test User' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Test User' })).not.toBeInTheDocument()
  })

  it('keeps the name in the sidebar as plain text: the avatar is the only way in', async () => {
    await open({ teacher: true })
    expect(screen.getAllByRole('link', { name: 'Test User' })).toHaveLength(1)
    expect(screen.getByText('Test User')).not.toHaveAttribute('href')
  })

  it.each<[string, Partial<Roles>]>([
    ['a student', { student: true }],
    ['a student who is also a teacher', { student: true, teacher: true }],
  ])('shows %s an avatar at the top right that links to their account page', async (_label, roles) => {
    mockDashboard()
    await open(roles)
    expect(within(screen.getByRole('banner')).getByRole('link', { name: 'Test User' })).toHaveAttribute('href', '/account')
  })

  it('keeps the avatar of a student linked even without a built-in password (the page still has their settings)', async () => {
    mockDashboard()
    await open({ student: true }, { can_change_password: false })
    expect(screen.getByRole('link', { name: 'Test User' })).toHaveAttribute('href', '/account')
  })
})

describe('the account page for students (the former personal dashboard, S-18)', () => {
  const recent = [
    {
      conversation_id: '11111111-1111-4111-8111-111111111111',
      title: '電車難題',
      claim: '不應該扳動',
      stance: '義務論',
      stage: 3,
      turn_count: 4,
      ended_at: '2026-11-10T08:00:00Z',
    },
  ]

  it('shows the personal settings and the dashboard content on one page', async () => {
    mockDashboard({ total_conversations: 5, completed: 2, total_turns: 17, recent })
    await open({ student: true })
    // 個人設定：顯示名稱、Google 帳號、更改密碼
    expect(screen.getByRole('heading', { name: '個人設定' })).toBeInTheDocument()
    expect(screen.getByLabelText('顯示名稱')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Google 帳號' })).toBeInTheDocument()
    expect(changeButton()).toBeInTheDocument()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    // 原個人儀表板的內容
    expect(await screen.findByText('已完成對話')).toBeInTheDocument()
    expect(screen.getByText('17')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '電車難題' })).toHaveAttribute('href', '/conversations/11111111-1111-4111-8111-111111111111')
    expect(screen.queryByRole('heading', { level: 1, name: '我的儀表板' })).not.toBeInTheDocument()
  })

  it('puts the settings above the dashboard content, as the old page did', async () => {
    mockDashboard({ total_conversations: 5, completed: 2, total_turns: 17, recent })
    await open({ student: true })
    const settings = screen.getByRole('heading', { name: '個人設定' })
    const stats = await screen.findByText('已完成對話')
    expect(settings.compareDocumentPosition(stats) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('keeps the other content when the account cannot change its password', async () => {
    mockDashboard()
    await open({ student: true }, { can_change_password: false })
    expect(screen.getByLabelText('顯示名稱')).toBeInTheDocument()
    noChangeButton()
  })

  it('shows a retryable error for the dashboard part without hiding the settings', async () => {
    server.use(http.get('/api/dashboard/me', () => apiError(500, 'internal')))
    await open({ student: true })
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByLabelText('顯示名稱')).toBeInTheDocument()
    expect(changeButton()).toBeInTheDocument()
  })

  it('has neither 「帳號」 nor 「我的儀表板」 in the sidebar: it is reached from the avatar', async () => {
    mockDashboard()
    await open({ student: true })
    const nav = screen.getByRole('navigation', { name: 'main' })
    expect(within(nav).queryByRole('link', { name: '帳號' })).not.toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: '我的儀表板' })).not.toBeInTheDocument()
    expect(within(screen.getByRole('banner')).getByRole('link', { name: 'Test User' })).toHaveAttribute('href', '/account')
  })

  it('does not ask for the dashboard data when a teacher or an admin opens the page', async () => {
    let asked = 0
    server.use(
      http.get('/api/dashboard/me', () => {
        asked += 1
        return HttpResponse.json(emptyDashboard)
      }),
    )
    for (const roles of [{ teacher: true }, { admin: true }] as Partial<Roles>[]) {
      const view = (mockMe(makeMe(roles)), renderApp(null, { route: '/account' }))
      await screen.findByRole('heading', { level: 1, name: '帳號' })
      await screen.findByRole('button', { name: '更改密碼' })
      view.unmount()
    }
    expect(asked).toBe(0)
  })
})
