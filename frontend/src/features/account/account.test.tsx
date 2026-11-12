import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Me, Roles } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

/** `/account`（F-37、S-18）：教師與管理者更改密碼的地方，所有已開通的角色都進得去；全部使用合成資料。 */

const open = async (roles: Partial<Roles>, overrides: Partial<Me> = {}) => {
  mockMe(makeMe(roles, overrides))
  renderApp(null, { route: '/account' })
  await screen.findByRole('heading', { level: 1, name: '帳號' })
}

const section = () => screen.getByRole('region', { name: '更改密碼' })

describe('the account page', () => {
  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
    ['a teacher who is also an admin', { teacher: true, admin: true }],
    ['a student', { student: true }],
  ])('offers the change-password form to %s', async (_label, roles) => {
    await open(roles)
    const form = within(section())
    expect(form.getByLabelText('目前密碼')).toHaveAttribute('autocomplete', 'current-password')
    expect(form.getByLabelText('新密碼')).toBeInTheDocument()
    expect(form.getByLabelText('確認新密碼')).toBeInTheDocument()
    // 這頁只放更改密碼：顯示名稱與 Google 連結是學生個人設定的內容
    expect(screen.queryByLabelText('顯示名稱')).not.toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Google 帳號' })).not.toBeInTheDocument()
  })

  it('does not send an admin-only account back to the operations home', async () => {
    await open({ admin: true })
    expect(screen.queryByRole('heading', { name: '系統維運' })).not.toBeInTheDocument()
    expect(section()).toBeInTheDocument()
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
    const form = within(section())
    await userEvent.type(form.getByLabelText('目前密碼'), 'old-pass-123')
    await userEvent.type(form.getByLabelText('新密碼'), 'new-pass-456')
    await userEvent.type(form.getByLabelText('確認新密碼'), 'new-pass-456')
    await userEvent.click(form.getByRole('button', { name: '更改密碼' }))
    expect(await form.findByRole('status')).toHaveTextContent('密碼已更改，其他裝置已登出。')
    expect(posts).toEqual([{ current_password: 'old-pass-123', new_password: 'new-pass-456' }])
  })

  it('shows localized backend errors', async () => {
    server.use(http.post('/api/auth/change-password', () => apiError(400, 'invalid_current_password')))
    await open({ admin: true })
    const form = within(section())
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
    expect(screen.queryByRole('region', { name: '更改密碼' })).not.toBeInTheDocument()
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

  it('is not in the navigation list: it is reached from the name in the sidebar', async () => {
    await open({ teacher: true })
    const nav = screen.getByRole('navigation', { name: 'main' })
    expect(within(nav).queryByRole('link', { name: '帳號' })).not.toBeInTheDocument()
  })
})

describe('the name in the sidebar', () => {
  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
    ['a teacher who is also an admin', { teacher: true, admin: true }],
  ])('links %s with a built-in password to /account', async (_label, roles) => {
    await open(roles)
    expect(screen.getByRole('link', { name: 'Test User' })).toHaveAttribute('href', '/account')
  })

  it.each<[string, Partial<Roles>]>([
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
  ])('keeps the name of %s plain text when the account cannot change its password', async (_label, roles) => {
    await open(roles, { can_change_password: false })
    expect(screen.getByText('Test User')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Test User' })).not.toBeInTheDocument()
  })

  it.each<[string, Partial<Roles>]>([
    ['a student', { student: true }],
    ['a student who is also a teacher', { student: true, teacher: true }],
  ])('still links %s to the personal settings', async (_label, roles) => {
    await open(roles)
    expect(screen.getByRole('link', { name: 'Test User' })).toHaveAttribute('href', '/dashboard')
  })
})
