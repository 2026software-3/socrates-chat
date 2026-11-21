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

const settingsCard = () => screen.getByRole('form', { name: '個人設定' })

/** `/api/me` 的 GET 與 PATCH 共用一份可變的使用者，模擬後端保存名稱。 */
function mockProfileApi(roles: Partial<Roles>, initial: string | null) {
  let name = initial
  const patches: unknown[] = []
  server.use(
    http.get('/api/me', () => HttpResponse.json(makeMe(roles, { display_name: name }))),
    http.patch('/api/me', async ({ request }) => {
      const body = (await request.json()) as { display_name: string | null }
      patches.push(body)
      name = body.display_name?.trim() || null
      return HttpResponse.json(makeMe(roles, { display_name: name }))
    }),
  )
  return patches
}

describe('the account page', () => {
  it.each<[string, Partial<Roles>, string]>([
    ['a teacher', { teacher: true }, '教師'],
    ['an admin-only account', { admin: true }, '管理者'],
    ['a teacher who is also an admin', { teacher: true, admin: true }, '管理者 / 教師'],
  ])('gives %s the same 個人設定 form as students, plus read-only account info', async (_label, roles, roleText) => {
    await open(roles)
    const card = within(settingsCard())
    // 與學生同一個表單：同一個標題、可編輯的顯示名稱、「儲存」與「更改密碼」
    expect(card.getByRole('heading', { level: 2, name: '個人設定' })).toBeInTheDocument()
    expect(card.getByLabelText('顯示名稱')).toHaveValue('Test User')
    expect(card.getByRole('button', { name: '儲存' })).toBeInTheDocument()
    expect(card.getByRole('button', { name: '更改密碼' })).toBeInTheDocument()
    // 電子郵件與角色是唯讀的帳號資訊（教師與管理者才有這兩列）
    expect(card.getByText('電子郵件')).toBeInTheDocument()
    expect(card.getByText('user@example.com')).toBeInTheDocument()
    expect(card.getByText('角色')).toBeInTheDocument()
    expect(card.getByText(roleText)).toBeInTheDocument()
    expect(card.getAllByRole('textbox')).toHaveLength(1)
    // 沒有學生才有的內容
    expect(screen.queryByRole('heading', { name: 'Google 帳號' })).not.toBeInTheDocument()
    expect(screen.queryByText('已完成對話')).not.toBeInTheDocument()
  })

  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
  ])('lets %s rename themselves, and the header follows', async (_label, roles) => {
    const patches = mockProfileApi(roles, '舊名稱')
    renderApp(null, { route: '/account' })
    const input = await screen.findByLabelText('顯示名稱')
    await userEvent.clear(input)
    await userEvent.type(input, '王老師')
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已儲存')
    expect(patches).toEqual([{ display_name: '王老師' }])
    expect(within(screen.getByRole('complementary')).getByText('王老師')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '王老師' })).toHaveAttribute('href', '/account')
  })

  it('clearing a teacher name sends null and the header falls back to the email', async () => {
    const patches = mockProfileApi({ teacher: true }, '王老師')
    renderApp(null, { route: '/account' })
    await userEvent.clear(await screen.findByLabelText('顯示名稱'))
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已儲存')
    expect(patches).toEqual([{ display_name: null }])
    expect(screen.getByRole('link', { name: 'user@example.com' })).toBeInTheDocument()
    expect(screen.getByLabelText('顯示名稱')).toHaveValue('')
  })

  it('applies the same 30-character limit to teachers', async () => {
    mockProfileApi({ teacher: true }, null)
    renderApp(null, { route: '/account' })
    const input = await screen.findByLabelText('顯示名稱')
    await userEvent.click(input)
    await userEvent.paste('字'.repeat(31))
    expect(screen.getByText('31 / 30')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '儲存' })).toBeDisabled()
  })

  it('changing the password never saves the display name of a teacher either', async () => {
    const patches = mockProfileApi({ teacher: true }, '王老師')
    server.use(http.post('/api/auth/change-password', () => new HttpResponse(null, { status: 204 })))
    renderApp(null, { route: '/account' })
    await screen.findByLabelText('顯示名稱')
    const form = await openDialog()
    await userEvent.type(form.getByLabelText('目前密碼'), 'old-pass-123')
    await userEvent.type(form.getByLabelText('新密碼'), 'new-pass-456')
    await userEvent.type(form.getByLabelText('確認新密碼'), 'new-pass-456{Enter}')
    expect(await screen.findByText('密碼已更改，其他裝置已登出。')).toBeInTheDocument()
    expect(patches).toEqual([])
  })

  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
  ])('keeps the change-password form out of the page for %s until the button is pressed', async (_label, roles) => {
    await open(roles)
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    const form = await openDialog()
    expect(form.getByLabelText('目前密碼')).toHaveAttribute('autocomplete', 'current-password')
    expect(form.getByLabelText('新密碼')).toBeInTheDocument()
    expect(form.getByLabelText('確認新密碼')).toBeInTheDocument()
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
  ])('explains inside the card that %s without a built-in password cannot change it here', async (_label, roles) => {
    await open(roles, { can_change_password: false })
    // 表單照常可編輯顯示名稱，按鈕本來的位置改為說明（與學生同一句）
    expect(within(settingsCard()).getByText('這個帳號不能在這裡更改密碼。')).toBeInTheDocument()
    expect(within(settingsCard()).getByLabelText('顯示名稱')).toBeInTheDocument()
    expect(within(settingsCard()).getByText('user@example.com')).toBeInTheDocument()
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
    expect(within(screen.getByRole('complementary')).getByText('Test User')).toBeInTheDocument()
    expect(screen.getByRole('img', { name: 'Test User' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Test User' })).not.toBeInTheDocument()
  })

  it('keeps the name in the sidebar as plain text: the avatar is the only way in', async () => {
    await open({ teacher: true })
    expect(screen.getAllByRole('link', { name: 'Test User' })).toHaveLength(1)
    expect(within(screen.getByRole('complementary')).getByText('Test User')).not.toHaveAttribute('href')
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
    expect(within(screen.getByRole('form', { name: '個人設定' })).getByRole('button', { name: '更改密碼' })).toBeInTheDocument()
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

  it('keeps the other content and explains, in the settings, why a student cannot change the password', async () => {
    mockDashboard()
    await open({ student: true }, { can_change_password: false })
    expect(screen.getByLabelText('顯示名稱')).toBeInTheDocument()
    noChangeButton()
    expect(within(screen.getByRole('form', { name: '個人設定' })).getByText('這個帳號不能在這裡更改密碼。')).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('explains it to a student who is also a teacher, too', async () => {
    mockDashboard()
    await open({ student: true, teacher: true }, { can_change_password: false })
    expect(screen.getAllByText('這個帳號不能在這裡更改密碼。')).toHaveLength(1)
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
