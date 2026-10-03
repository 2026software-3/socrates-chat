import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

describe('built-in login', () => {
  it('shows an email/password form next to the Google button', async () => {
    mockMe(null)
    renderApp(null, { route: '/login' })
    expect(await screen.findByLabelText('電子郵件')).toBeInTheDocument()
    expect(screen.getByLabelText('密碼')).toHaveAttribute('type', 'password')
    expect(screen.getByRole('button', { name: '登入' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '使用 Google 登入' })).toBeInTheDocument()
  })

  it('posts credentials and enters the app on success', async () => {
    let body: unknown
    let loggedIn = false
    server.use(
      http.get('/api/me', () => (loggedIn ? HttpResponse.json(makeMe({ student: true })) : apiError(401, 'unauthorized'))),
      http.post('/api/auth/login', async ({ request }) => {
        body = await request.json()
        loggedIn = true
        return HttpResponse.json({ must_change_password: false })
      }),
    )
    renderApp(null, { route: '/login' })
    await userEvent.type(await screen.findByLabelText('電子郵件'), 'a@example.com')
    await userEvent.type(screen.getByLabelText('密碼'), 'password-123')
    await userEvent.click(screen.getByRole('button', { name: '登入' }))
    expect(await screen.findByRole('navigation', { name: 'main' })).toBeInTheDocument()
    expect(body).toEqual({ email: 'a@example.com', password: 'password-123' })
  })

  it.each([
    ['invalid_credentials', 401, '電子郵件或密碼錯誤。'],
    ['too_many_attempts', 429, '嘗試次數過多，請 15 分鐘後再試。'],
  ])('shows a translated message for %s', async (code, status, text) => {
    mockMe(null)
    server.use(http.post('/api/auth/login', () => apiError(status, code)))
    renderApp(null, { route: '/login' })
    await userEvent.type(await screen.findByLabelText('電子郵件'), 'a@example.com')
    await userEvent.type(screen.getByLabelText('密碼'), 'wrong-password')
    await userEvent.click(screen.getByRole('button', { name: '登入' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(text)
  })
})

describe('forced password change', () => {
  const tempMe = () => makeMe({ student: true }, { must_change_password: true })

  it('sends accounts on a temporary password to the change page, hiding navigation', async () => {
    mockMe(tempMe())
    renderApp(null, { route: '/available' })
    expect(await screen.findByRole('heading', { name: '更改密碼' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'main' })).not.toBeInTheDocument()
  })

  it('rejects mismatched confirmation without calling the API', async () => {
    mockMe(tempMe())
    renderApp(null, { route: '/' })
    await userEvent.type(await screen.findByLabelText('目前密碼'), 'temp-password')
    await userEvent.type(screen.getByLabelText('新密碼'), 'new-password-1')
    await userEvent.type(screen.getByLabelText('確認新密碼'), 'new-password-2')
    await userEvent.click(screen.getByRole('button', { name: '更改密碼' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('兩次輸入的新密碼不一致。')
  })

  it('changes the password then releases the account into the app', async () => {
    let changed = false
    let body: unknown
    server.use(
      http.get('/api/me', () => HttpResponse.json(changed ? makeMe({ student: true }) : tempMe())),
      http.post('/api/auth/change-password', async ({ request }) => {
        body = await request.json()
        changed = true
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderApp(null, { route: '/' })
    await userEvent.type(await screen.findByLabelText('目前密碼'), 'temp-password')
    await userEvent.type(screen.getByLabelText('新密碼'), 'new-password-1')
    await userEvent.type(screen.getByLabelText('確認新密碼'), 'new-password-1')
    await userEvent.click(screen.getByRole('button', { name: '更改密碼' }))
    expect(await screen.findByRole('navigation', { name: 'main' })).toBeInTheDocument()
    expect(body).toEqual({ current_password: 'temp-password', new_password: 'new-password-1' })
  })

  it.each([
    ['invalid_current_password', '目前密碼不正確。'],
    ['invalid_new_password', '新密碼長度需介於 8 到 128 個字元。'],
    ['password_unchanged', '新密碼不能與目前密碼相同。'],
  ])('shows a translated message for %s', async (code, text) => {
    mockMe(tempMe())
    server.use(http.post('/api/auth/change-password', () => apiError(400, code)))
    renderApp(null, { route: '/' })
    await userEvent.type(await screen.findByLabelText('目前密碼'), 'temp-password')
    await userEvent.type(screen.getByLabelText('新密碼'), 'new-password-1')
    await userEvent.type(screen.getByLabelText('確認新密碼'), 'new-password-1')
    await userEvent.click(screen.getByRole('button', { name: '更改密碼' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent(text))
  })
})
