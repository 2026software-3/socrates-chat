import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Roles } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

/** 個人設定的「更改密碼」（F-37、S-18）：所有有內建密碼的角色都能主動更改；全部使用合成資料。 */

function mockDashboard() {
  server.use(
    http.get('/api/dashboard/me', () =>
      HttpResponse.json({ total_conversations: 0, completed: 0, total_turns: 0, recent: [] }),
    ),
  )
}

/** 以指定角色開啟個人設定，`can_change_password` 由呼叫端決定（後端的 `/api/me` 欄位）。 */
async function openSettings(roles: Partial<Roles>, canChange = true) {
  mockDashboard()
  mockMe(makeMe(roles, { can_change_password: canChange }))
  renderApp(null, { route: '/account' })
  await screen.findByRole('heading', { name: '個人設定' })
}


/** 頁面上只有一個「更改密碼」按鈕；表單在按下後的彈出視窗裡（S-18）。 */
const section = () => screen.getByRole('dialog', { name: '更改密碼' })

async function openDialog() {
  await userEvent.click(screen.getByRole('button', { name: '更改密碼' }))
  await screen.findByRole('dialog', { name: '更改密碼' })
}

async function fill(current: string, next: string, confirm: string) {
  await openDialog()
  const form = within(section())
  await userEvent.type(form.getByLabelText('目前密碼'), current)
  await userEvent.type(form.getByLabelText('新密碼'), next)
  await userEvent.type(form.getByLabelText('確認新密碼'), confirm)
  await userEvent.click(form.getByRole('button', { name: '更改密碼' }))
}

describe('change password in the personal settings', () => {
  it('is a button, not an open form, until the student asks for it', async () => {
    await openSettings({ student: true })
    expect(screen.getByRole('button', { name: '更改密碼' })).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('新密碼')).not.toBeInTheDocument()
  })

  it('puts the button in the personal settings next to 儲存, not in a row or card of its own', async () => {
    await openSettings({ student: true })
    const settings = screen.getByRole('form', { name: '個人設定' })
    expect(within(settings).getByRole('button', { name: '更改密碼' })).toHaveAttribute('type', 'button')
    expect(within(settings).getByRole('button', { name: '儲存' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '密碼' })).not.toBeInTheDocument()
  })

  it('changing the password never saves the display name, even though the button sits in that form', async () => {
    const patches: unknown[] = []
    server.use(
      http.patch('/api/me', async ({ request }) => {
        patches.push(await request.json())
        return HttpResponse.json(makeMe({ student: true }))
      }),
      http.post('/api/auth/change-password', () => new HttpResponse(null, { status: 204 })),
    )
    await openSettings({ student: true })
    await fill('old-pass-123', 'new-pass-456', 'new-pass-456')
    expect(await screen.findByRole('status')).toHaveTextContent('密碼已更改')
    // 失敗與前端檢查的路徑也一樣：視窗裡按 Enter 或送出都不會碰到外層的個人設定表單
    await openDialog()
    await userEvent.type(within(section()).getByLabelText('目前密碼'), 'x{Enter}')
    await userEvent.keyboard('{Escape}')
    expect(patches).toEqual([])
  })

  it('opens a dialog with the three fields', async () => {
    await openSettings({ student: true })
    await openDialog()
    const form = within(section())
    expect(form.getByLabelText('目前密碼')).toHaveAttribute('autocomplete', 'current-password')
    expect(form.getByLabelText('新密碼')).toHaveAttribute('autocomplete', 'new-password')
    expect(form.getByLabelText('確認新密碼')).toBeInTheDocument()
  })

  // 教師與純管理者沒有個人設定頁（/dashboard 只給學生），他們在 /account 更改密碼，見 features/account

  it('has no button when the backend says the account cannot change its password, and says why in the settings', async () => {
    // 只用 Google 登入的帳號沒有內建密碼，或示範模式的示範帳號
    await openSettings({ student: true }, false)
    expect(screen.queryByRole('button', { name: '更改密碼' })).not.toBeInTheDocument()
    expect(screen.queryByLabelText('目前密碼')).not.toBeInTheDocument()
    // 不讓學生以為功能壞掉：個人設定裡（按鈕本來的位置）顯示說明，與教師、管理者的帳號頁同一句
    const settings = screen.getByRole('form', { name: '個人設定' })
    expect(within(settings).getByText('這個帳號不能在這裡更改密碼。')).toBeInTheDocument()
    expect(within(settings).getByRole('button', { name: '儲存' })).toBeInTheDocument()
    // 只是說明，不是錯誤
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('does not show the explanation to an account that can change its password', async () => {
    await openSettings({ student: true })
    expect(screen.queryByText('這個帳號不能在這裡更改密碼。')).not.toBeInTheDocument()
  })

  it.each([
    ['English', 'en', 'This account cannot change its password here.'],
    ['Spanish', 'es', 'Esta cuenta no puede cambiar su contraseña aquí.'],
  ] as const)('explains it in %s too', async (_name, lang, text) => {
    mockDashboard()
    // 畫面語言跟著使用者的偏好（`me.language`），所以從這裡設
    mockMe(makeMe({ student: true }, { can_change_password: false, language: lang }))
    renderApp(null, { route: '/account', lang })
    expect(await screen.findByText(text)).toBeInTheDocument()
  })

  it('sends the current and new password, clears the fields and says other devices were signed out', async () => {
    const posts: unknown[] = []
    server.use(
      http.post('/api/auth/change-password', async ({ request }) => {
        posts.push(await request.json())
        return new HttpResponse(null, { status: 204 })
      }),
    )
    await openSettings({ student: true })
    await fill('old-pass-123', 'new-pass-456', 'new-pass-456')

    // 成功後彈出視窗關閉，確認訊息留在頁面上
    expect(await screen.findByRole('status')).toHaveTextContent('密碼已更改，其他裝置已登出。')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(posts).toEqual([{ current_password: 'old-pass-123', new_password: 'new-pass-456' }])
    // 仍留在個人設定：目前的登入保持，不會被帶去登入頁
    expect(screen.getByRole('heading', { name: '個人設定' })).toBeInTheDocument()
    // 再開一次，欄位是空的：密碼不留在畫面或元件裡
    await openDialog()
    expect(within(section()).getByLabelText('目前密碼')).toHaveValue('')
    expect(within(section()).getByLabelText('新密碼')).toHaveValue('')
    expect(within(section()).getByLabelText('確認新密碼')).toHaveValue('')
    expect(within(section()).queryByRole('status')).not.toBeInTheDocument()
  })

  it('does not call the backend when the two new passwords differ', async () => {
    let called = false
    server.use(
      http.post('/api/auth/change-password', () => {
        called = true
        return new HttpResponse(null, { status: 204 })
      }),
    )
    await openSettings({ student: true })
    await fill('old-pass-123', 'new-pass-456', 'new-pass-457')
    expect(await within(section()).findByRole('alert')).toHaveTextContent('兩次輸入的新密碼不一致。')
    expect(called).toBe(false)
  })

  it.each([
    [400, 'invalid_current_password', '目前密碼不正確。'],
    [400, 'invalid_new_password', '新密碼長度需介於 8 到 128 個字元。'],
    [400, 'password_unchanged', '新密碼不能與目前密碼相同。'],
    [429, 'too_many_attempts', '嘗試次數過多，請 15 分鐘後再試。'],
    [403, 'demo_account', '示範帳號的密碼不能更改。'],
  ])('shows the localized message for %i %s and keeps what was typed', async (status, code, text) => {
    server.use(http.post('/api/auth/change-password', () => apiError(status, code)))
    await openSettings({ student: true })
    await fill('old-pass-123', 'new-pass-456', 'new-pass-456')
    expect(await within(section()).findByRole('alert')).toHaveTextContent(text)
    expect(within(section()).queryByRole('status')).not.toBeInTheDocument()
    // 失敗時保留輸入，使用者只要修正一欄；不把密碼清掉也不顯示出來
    await waitFor(() => expect(within(section()).getByLabelText('新密碼')).toHaveValue('new-pass-456'))
    expect(within(section()).getByLabelText('新密碼')).toHaveAttribute('type', 'password')
  })

  it('clears an earlier error once a retry succeeds', async () => {
    let attempts = 0
    server.use(
      http.post('/api/auth/change-password', () => {
        attempts += 1
        return attempts === 1 ? apiError(400, 'invalid_current_password') : new HttpResponse(null, { status: 204 })
      }),
    )
    await openSettings({ student: true })
    await fill('wrong-pass-1', 'new-pass-456', 'new-pass-456')
    expect(await within(section()).findByRole('alert')).toBeInTheDocument()
    await userEvent.clear(within(section()).getByLabelText('目前密碼'))
    await userEvent.type(within(section()).getByLabelText('目前密碼'), 'old-pass-123')
    await userEvent.click(within(section()).getByRole('button', { name: '更改密碼' }))
    expect(await screen.findByRole('status')).toHaveTextContent('密碼已更改')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('discards what was typed and any error when the dialog is closed with Escape', async () => {
    server.use(http.post('/api/auth/change-password', () => apiError(400, 'invalid_current_password')))
    await openSettings({ student: true })
    await fill('old-pass-123', 'new-pass-456', 'new-pass-456')
    expect(await within(section()).findByRole('alert')).toBeInTheDocument()
    await userEvent.keyboard('{Escape}')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    await openDialog()
    expect(within(section()).getByLabelText('目前密碼')).toHaveValue('')
    expect(within(section()).getByLabelText('新密碼')).toHaveValue('')
    expect(within(section()).queryByRole('alert')).not.toBeInTheDocument()
  })

  it('does not leave an old confirmation on the page when the dialog is opened again', async () => {
    server.use(http.post('/api/auth/change-password', () => new HttpResponse(null, { status: 204 })))
    await openSettings({ student: true })
    await fill('old-pass-123', 'new-pass-456', 'new-pass-456')
    expect(await screen.findByRole('status')).toBeInTheDocument()
    await openDialog()
    expect(screen.queryByText('密碼已更改，其他裝置已登出。')).not.toBeInTheDocument()
  })
})
