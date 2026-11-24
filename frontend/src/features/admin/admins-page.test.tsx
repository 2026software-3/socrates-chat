import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { AdminsPage } from '@/features/admin/admins-page'
import { apiError, renderApp } from '@/test/render'
import { server } from '@/test/server'

function mockAdmins(initial: string[], issued: string | null = 'TEMP2345WXYZ') {
  const emails = [...initial]
  const bodies: unknown[] = []
  server.use(
    http.get('/api/admin/admins', () => HttpResponse.json({ emails })),
    http.post('/api/admin/admins', async ({ request }) => {
      const body = (await request.json()) as { email: string }
      bodies.push(body)
      if (!emails.includes(body.email)) emails.push(body.email)
      return HttpResponse.json({ email: body.email, temporary_password: issued })
    }),
  )
  return { emails, bodies }
}

const render = () => renderApp(<AdminsPage />)

describe('AdminsPage', () => {
  it('lists admins sorted by email with a count', async () => {
    mockAdmins(['b@example.com', 'a@example.com'])
    render()
    const list = await screen.findByRole('list', { name: '管理者清單' })
    expect(within(list).getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      expect.stringContaining('a@example.com'),
      expect.stringContaining('b@example.com'),
    ])
    expect(screen.getByText('共 2 位管理者')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    server.use(http.get('/api/admin/admins', () => apiError(500, 'internal', 'req-9')))
    render()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    mockAdmins(['a@example.com'])
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('共 1 位管理者')).toBeInTheDocument()
  })

  it('adds an admin and shows the temporary password once', async () => {
    const { bodies } = mockAdmins(['a@example.com'])
    render()
    await userEvent.type(await screen.findByLabelText('管理者電子郵件'), 'new@example.com')
    await userEvent.click(screen.getByRole('button', { name: '新增管理者' }))
    expect(await screen.findByLabelText('臨時密碼')).toHaveValue('TEMP2345WXYZ')
    expect(bodies).toEqual([{ email: 'new@example.com' }])
    await userEvent.click(screen.getByRole('button', { name: '我已記下，關閉' }))
    await waitFor(() => expect(screen.queryByDisplayValue('TEMP2345WXYZ')).not.toBeInTheDocument())
    expect(await screen.findByText('共 2 位管理者')).toBeInTheDocument()
  })

  it('promotes an existing account without a temporary password', async () => {
    mockAdmins(['a@example.com'], null)
    render()
    await userEvent.type(await screen.findByLabelText('管理者電子郵件'), 'old@example.com')
    await userEvent.click(screen.getByRole('button', { name: '新增管理者' }))
    expect(await screen.findByText('已將 old@example.com 設為管理者，密碼未變動。')).toBeInTheDocument()
    expect(screen.queryByLabelText('臨時密碼')).not.toBeInTheDocument()
  })

  it('shows the server error when adding fails', async () => {
    mockAdmins(['a@example.com'])
    server.use(http.post('/api/admin/admins', () => apiError(500, 'internal')))
    render()
    await userEvent.type(await screen.findByLabelText('管理者電子郵件'), 'new@example.com')
    await userEvent.click(screen.getByRole('button', { name: '新增管理者' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
  })

  it('resets another admin password from the list', async () => {
    mockAdmins(['a@example.com'])
    const bodies: unknown[] = []
    server.use(
      http.post('/api/admin/users/reset-password', async ({ request }) => {
        bodies.push(await request.json())
        return HttpResponse.json({ email: 'a@example.com', temporary_password: 'ABCD2345WXYZ' })
      }),
    )
    render()
    await userEvent.click(await screen.findByRole('button', { name: '重設 a@example.com 的密碼' }))
    await userEvent.click(screen.getByRole('button', { name: '重設密碼' }))
    expect(await screen.findByLabelText('臨時密碼')).toHaveValue('ABCD2345WXYZ')
    expect(bodies).toEqual([{ email: 'a@example.com' }])
  })

  it('renders in English', async () => {
    mockAdmins(['a@example.com'])
    renderApp(<AdminsPage />, { lang: 'en' })
    expect(await screen.findByText('Admins: 1')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Add admin' })).toBeInTheDocument()
  })
})
