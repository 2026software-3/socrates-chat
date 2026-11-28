import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Me, Roles } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

/**
 * `/account`（F-37、S-18、F-41.17）：
 * - 學生：主標題為「個人檔案」，分成「個人設定」「統計資料」「隱私設定」三個頁籤。
 * - 教師與管理者：主標題維持「帳號」，更改密碼與檢視唯讀帳號資訊的地方；全部使用合成資料。
 */

const emptyDashboard = { total_conversations: 0, completed: 0, total_turns: 0, recent: [] }

/** 學生的個人檔案（統計資料頁籤）會讀個人儀表板；教師與管理者不會（見下面的測試）。 */
function mockDashboard(data: object = emptyDashboard) {
  server.use(http.get('/api/dashboard/me', () => HttpResponse.json(data)))
}

const openStaff = async (roles: Partial<Roles>, overrides: Partial<Me> = {}) => {
  mockMe(makeMe(roles, overrides))
  renderApp(null, { route: '/account' })
  await screen.findByRole('heading', { level: 1, name: '帳號' })
}

const openStudent = async (overrides: Partial<Me> = {}, dashboard: object = emptyDashboard) => {
  mockDashboard(dashboard)
  mockMe(makeMe({ student: true }, overrides))
  renderApp(null, { route: '/account' })
  await screen.findByRole('heading', { level: 1, name: '個人檔案' })
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

describe('the account page for teachers and admins', () => {
  it.each<[string, Partial<Roles>, string]>([
    ['a teacher', { teacher: true }, '教師'],
    ['an admin-only account', { admin: true }, '管理者'],
    ['a teacher who is also an admin', { teacher: true, admin: true }, '管理者 / 教師'],
  ])('gives %s the same 個人設定 form as students, plus read-only account info', async (_label, roles, roleText) => {
    await openStaff(roles)
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
    // 沒有學生才有的頁籤與內容
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Google 帳號' })).not.toBeInTheDocument()
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
    expect(within(screen.getByRole('banner')).getByRole('button', { name: '王老師' })).toBeInTheDocument()
  })

  it('clearing a teacher name sends null and the header falls back to the email', async () => {
    const patches = mockProfileApi({ teacher: true }, '王老師')
    renderApp(null, { route: '/account' })
    await userEvent.clear(await screen.findByLabelText('顯示名稱'))
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已儲存')
    expect(patches).toEqual([{ display_name: null }])
    expect(within(screen.getByRole('banner')).getByRole('button', { name: 'user@example.com' })).toBeInTheDocument()
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
    await openStaff(roles)
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    const form = await openDialog()
    expect(form.getByLabelText('目前密碼')).toHaveAttribute('autocomplete', 'current-password')
    expect(form.getByLabelText('新密碼')).toBeInTheDocument()
    expect(form.getByLabelText('確認新密碼')).toBeInTheDocument()
  })

  it('does not send an admin-only account back to the operations home', async () => {
    await openStaff({ admin: true })
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
    await openStaff({ teacher: true })
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
    await openStaff({ admin: true })
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
    await openStaff(roles, { can_change_password: false })
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

  it('is not in the navigation list for teachers: it is reached from the account menu at the top right', async () => {
    await openStaff({ teacher: true })
    const nav = screen.getByRole('navigation', { name: '主導覽' })
    expect(within(nav).queryByRole('link', { name: '帳號' })).not.toBeInTheDocument()
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

describe('the avatar account menu at the top right', () => {
  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
    ['a teacher who is also an admin', { teacher: true, admin: true }],
  ])('links %s with a built-in password to /account from the account menu', async (_label, roles) => {
    await openStaff(roles)
    const trigger = within(screen.getByRole('banner')).getByRole('button', { name: 'Test User' })
    expect(trigger).toHaveTextContent('T')
    await userEvent.click(trigger)
    expect(screen.getByRole('link', { name: '個人設定' })).toHaveAttribute('href', '/account')
  })

  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
  ])('does not link account settings for %s when the account cannot change its password', async (_label, roles) => {
    await openStaff(roles, { can_change_password: false })
    const trigger = within(screen.getByRole('banner')).getByRole('button', { name: 'Test User' })
    await userEvent.click(trigger)
    expect(screen.queryByRole('link', { name: '個人設定' })).not.toBeInTheDocument()
  })

  it.each<[string, Partial<Roles>]>([
    ['a student', { student: true }],
    ['a student who is also a teacher', { student: true, teacher: true }],
  ])('reaches /account via the student legend (VI 個人檔案) for %s', async (_label, roles) => {
    mockDashboard()
    mockMe(makeMe(roles))
    renderApp(null, { route: '/account' })
    await screen.findByRole('heading', { level: 1, name: '個人檔案' })
    const nav = screen.getByRole('navigation', { name: '主導覽' })
    expect(within(nav).getByRole('link', { name: /個人檔案/ })).toHaveAttribute('href', '/account')
  })

  it('keeps /account reachable for a student even without a built-in password', async () => {
    await openStudent({ can_change_password: false })
    const nav = screen.getByRole('navigation', { name: '主導覽' })
    expect(within(nav).getByRole('link', { name: /個人檔案/ })).toHaveAttribute('href', '/account')
  })
})

describe('the profile page for students (F-41.17 three tabs)', () => {
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

  it('renders three tabs under 個人檔案 and defaults to 個人設定', async () => {
    await openStudent({}, { total_conversations: 5, completed: 2, total_turns: 17, recent })
    const tablist = screen.getByRole('tablist', { name: '個人檔案分頁' })
    expect(within(tablist).getByRole('tab', { name: '個人設定', selected: true })).toBeInTheDocument()
    expect(within(tablist).getByRole('tab', { name: '統計資料', selected: false })).toBeInTheDocument()
    expect(within(tablist).getByRole('tab', { name: '隱私設定', selected: false })).toBeInTheDocument()

    // 個人設定：顯示名稱、Google 帳號、更改密碼
    expect(screen.getByRole('heading', { name: '個人設定' })).toBeInTheDocument()
    expect(screen.getByLabelText('顯示名稱')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Google 帳號' })).toBeInTheDocument()
    expect(within(settingsCard()).getByRole('button', { name: '更改密碼' })).toBeInTheDocument()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 1, name: '我的儀表板' })).not.toBeInTheDocument()
  })

  it('switches to 統計資料 tab and shows stats, recent discussions with 查看總結, and privacy note', async () => {
    await openStudent({}, { total_conversations: 5, completed: 2, total_turns: 17, recent })
    await userEvent.click(screen.getByRole('tab', { name: '統計資料' }))

    expect(await screen.findByText('已完成的討論')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
    expect(screen.getByText('進行中的討論')).toBeInTheDocument()
    expect(screen.getByText('3')).toBeInTheDocument()
    expect(screen.getByText('對話總數')).toBeInTheDocument()
    expect(screen.getByText('5')).toBeInTheDocument()

    expect(screen.getByText('電車難題')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /查看總結/ })).toHaveAttribute(
      'href',
      '/conversations/11111111-1111-4111-8111-111111111111',
    )
    expect(screen.getByText('統計只有你自己看得到')).toBeInTheDocument()
  })

  it('shows a retryable Vedute Alert when dashboard stats fail to load without hiding other tabs', async () => {
    server.use(http.get('/api/dashboard/me', () => apiError(500, 'internal')))
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/account' })
    await screen.findByRole('heading', { level: 1, name: '個人檔案' })

    await userEvent.click(screen.getByRole('tab', { name: '統計資料' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重試' })).toBeInTheDocument()
  })

  it('switches to 隱私設定 tab and shows disabled planned switches, locked switches, and preview card', async () => {
    await openStudent({ display_name: '蘇格拉底學生' })
    await userEvent.click(screen.getByRole('tab', { name: '隱私設定' }))

    // 最上方龐貝藍提示
    expect(screen.getByRole('status')).toHaveTextContent('公開功能還沒開放，規則待確認。目前所有資料只有你自己看得到。')

    // 規劃中（disabled off）項目
    const displayNameSwitch = screen.getByRole('switch', { name: '顯示名稱' })
    const avatarSwitch = screen.getByRole('switch', { name: '角色外觀' })
    expect(displayNameSwitch).toBeDisabled()
    expect(displayNameSwitch).not.toBeChecked()
    expect(avatarSwitch).toBeDisabled()
    expect(avatarSwitch).not.toBeChecked()
    expect(screen.getAllByText('規劃中・目前只有你看得到')).toHaveLength(2)

    // 永不公開（locked）項目
    for (const label of ['電子郵件', '統計資料', '對話內容', '學習總結']) {
      const sw = screen.getByRole('switch', { name: label })
      expect(sw).toBeDisabled()
      expect(sw).not.toBeChecked()
      expect(sw).toHaveAttribute('data-locked', 'true')
    }
    expect(screen.getAllByText('永不公開')).toHaveLength(4)

    // 右側預覽名片
    const previewHeading = screen.getByRole('heading', { name: '同學看到的樣子（預覽）' })
    const previewCard = previewHeading.closest('section') as HTMLElement
    expect(within(previewCard).getByText('蘇格拉底學生')).toBeInTheDocument()
  })

  it('keeps the other content and explains, in the settings, why a student cannot change the password', async () => {
    await openStudent({ can_change_password: false })
    expect(screen.getByLabelText('顯示名稱')).toBeInTheDocument()
    noChangeButton()
    expect(within(settingsCard()).getByText('這個帳號不能在這裡更改密碼。')).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('explains it to a student who is also a teacher, too', async () => {
    mockDashboard()
    mockMe(makeMe({ student: true, teacher: true }, { can_change_password: false }))
    renderApp(null, { route: '/account' })
    await screen.findByRole('heading', { level: 1, name: '個人檔案' })
    expect(screen.getAllByText('這個帳號不能在這裡更改密碼。')).toHaveLength(1)
  })

  it('reaches /account via VI 個人檔案 in the sidebar without old 帳號 or 我的儀表板 links', async () => {
    await openStudent()
    const nav = screen.getByRole('navigation', { name: '主導覽' })
    expect(within(nav).queryByRole('link', { name: '帳號' })).not.toBeInTheDocument()
    expect(within(nav).queryByRole('link', { name: '我的儀表板' })).not.toBeInTheDocument()
    expect(within(nav).getByRole('link', { name: /個人檔案/ })).toHaveAttribute('href', '/account')
  })
})