import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { TeachersPage } from '@/features/admin/teachers-page'
import { apiError, renderApp } from '@/test/render'
import { server } from '@/test/server'

/** 有狀態的假後端：記錄收到的請求。 */
function mockTeachers(initial: string[]) {
  const emails = [...initial]
  const calls: { method: string; url: string; body?: unknown }[] = []
  server.use(
    http.get('/api/admin/teachers', () => HttpResponse.json({ emails })),
    http.post('/api/admin/teachers', async ({ request }) => {
      const body = (await request.json()) as { email: string }
      calls.push({ method: 'POST', url: '/api/admin/teachers', body })
      if (!emails.includes(body.email)) emails.push(body.email)
      return new HttpResponse(null, { status: 204 })
    }),
    http.delete('/api/admin/teachers/:email', ({ request }) => {
      calls.push({ method: 'DELETE', url: new URL(request.url).pathname })
      const raw = new URL(request.url).pathname.split('/').pop() ?? ''
      const email = decodeURIComponent(raw)
      const i = emails.indexOf(email)
      if (i >= 0) emails.splice(i, 1)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return { emails, calls }
}

const render = () => renderApp(<TeachersPage />)

describe('TeachersPage', () => {
  it('lists teachers sorted by email with a count and the explanation', async () => {
    mockTeachers(['b@example.com', 'a@example.com'])
    render()
    const list = await screen.findByRole('list', { name: '教師清單' })
    const items = within(list).getAllByRole('listitem')
    expect(items.map((li) => li.textContent)).toEqual([
      expect.stringContaining('a@example.com'),
      expect.stringContaining('b@example.com'),
    ])
    expect(screen.getByText('共 2 位教師')).toBeInTheDocument()
    expect(screen.getByText(/下次登入時成為教師/)).toBeInTheDocument()
  })

  it('shows a loading state, then an empty state', async () => {
    mockTeachers([])
    render()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('還沒有教師。請在上方新增。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    server.use(http.get('/api/admin/teachers', () => apiError(500, 'internal', 'req-9')))
    render()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByText('錯誤代碼：req-9')).toBeInTheDocument()
    mockTeachers(['a@example.com'])
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('a@example.com')).toBeInTheDocument()
  })

  it('adds a teacher and refreshes the list', async () => {
    const { calls } = mockTeachers(['a@example.com'])
    render()
    await screen.findByText('a@example.com')
    await userEvent.type(screen.getByLabelText('教師電子郵件'), ' new+tag@example.com ')
    await userEvent.click(screen.getByRole('button', { name: '新增教師' }))
    expect(await screen.findByText('new+tag@example.com')).toBeInTheDocument()
    expect(calls).toEqual([{ method: 'POST', url: '/api/admin/teachers', body: { email: 'new+tag@example.com' } }])
    expect(screen.getByLabelText('教師電子郵件')).toHaveValue('')
    expect(screen.getByText('共 2 位教師')).toBeInTheDocument()
  })

  it('does not duplicate an already existing teacher', async () => {
    mockTeachers(['a@example.com'])
    render()
    await screen.findByText('a@example.com')
    await userEvent.type(screen.getByLabelText('教師電子郵件'), 'a@example.com')
    await userEvent.click(screen.getByRole('button', { name: '新增教師' }))
    await screen.findByText('已新增 a@example.com。')
    expect(screen.getAllByText('a@example.com')).toHaveLength(1)
    expect(screen.getByText('共 1 位教師')).toBeInTheDocument()
  })

  it('shows a translated inline error for an invalid email', async () => {
    mockTeachers([])
    server.use(http.post('/api/admin/teachers', () => apiError(400, 'invalid_email')))
    render()
    await screen.findByText('還沒有教師。請在上方新增。')
    await userEvent.type(screen.getByLabelText('教師電子郵件'), 'nope')
    await userEvent.click(screen.getByRole('button', { name: '新增教師' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('電子郵件格式不正確。')
    expect(screen.getByLabelText('教師電子郵件')).toHaveValue('nope')
  })

  it('disables the add button while empty and while pending (no double submit)', async () => {
    mockTeachers([])
    let posts = 0
    let release!: () => void
    const gate = new Promise<void>((r) => (release = r))
    server.use(
      http.post('/api/admin/teachers', async () => {
        posts += 1
        await gate
        return new HttpResponse(null, { status: 204 })
      }),
    )
    render()
    await screen.findByText('還沒有教師。請在上方新增。')
    expect(screen.getByRole('button', { name: '新增教師' })).toBeDisabled()
    await userEvent.type(screen.getByLabelText('教師電子郵件'), 'a@example.com{Enter}')
    const pending = await screen.findByRole('button', { name: '新增中…' })
    expect(pending).toBeDisabled()
    await userEvent.type(screen.getByLabelText('教師電子郵件'), '{Enter}')
    release()
    await waitFor(() => expect(screen.getByRole('button', { name: '新增教師' })).toBeInTheDocument())
    expect(posts).toBe(1)
  })

  it('removes a teacher after confirmation, encoding the email in the path', async () => {
    const { calls } = mockTeachers(['a+b@example.com', 'z@example.com'])
    render()
    await screen.findByText('a+b@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 a+b@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog).toHaveTextContent('a+b@example.com')
    expect(calls).toHaveLength(0)
    await userEvent.click(within(dialog).getByRole('button', { name: '移除' }))
    await waitFor(() => expect(screen.queryByText('a+b@example.com')).not.toBeInTheDocument())
    expect(calls).toEqual([{ method: 'DELETE', url: `/api/admin/teachers/${encodeURIComponent('a+b@example.com')}` }])
    expect(calls[0].url).toBe('/api/admin/teachers/a%2Bb%40example.com')
  })

  it('cancelling the confirmation sends nothing', async () => {
    const { calls } = mockTeachers(['a@example.com'])
    render()
    await screen.findByText('a@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 a@example.com' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '取消' }))
    expect(calls).toHaveLength(0)
    expect(screen.getByText('a@example.com')).toBeInTheDocument()
  })

  it('shows a translated error and refreshes when the removal returns 404', async () => {
    mockTeachers(['a@example.com'])
    server.use(http.delete('/api/admin/teachers/:email', () => apiError(404, 'not_found')))
    render()
    await screen.findByText('a@example.com')
    // 列表在移除前已被別人改過
    server.use(http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })))
    await userEvent.click(screen.getByRole('button', { name: '移除 a@example.com' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '移除' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('找不到資料。')
    expect(await screen.findByText('還沒有教師。請在上方新增。')).toBeInTheDocument()
  })

  it('keeps the dialog open with the error inside it when the removal fails with 500', async () => {
    mockTeachers(['a@example.com'])
    server.use(http.delete('/api/admin/teachers/:email', () => apiError(500, 'internal')))
    render()
    await screen.findByText('a@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 a@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '移除' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(within(screen.getByRole('alertdialog')).getByRole('button', { name: '移除' })).toBeEnabled()
  })

  it('shows a retryable error next to the stale list when a reload fails', async () => {
    mockTeachers(['a@example.com'])
    render()
    await screen.findByText('a@example.com')
    server.use(http.get('/api/admin/teachers', () => apiError(500, 'internal')))
    await userEvent.type(screen.getByLabelText('教師電子郵件'), 'b@example.com')
    await userEvent.click(screen.getByRole('button', { name: '新增教師' }))
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByText('a@example.com')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重試' })).toBeInTheDocument()
  })

  it('filters by search text', async () => {
    mockTeachers(['alice@example.com', 'bob@example.com'])
    render()
    await screen.findByText('alice@example.com')
    await userEvent.type(screen.getByLabelText('搜尋教師'), 'BOB')
    expect(screen.queryByText('alice@example.com')).not.toBeInTheDocument()
    expect(screen.getByText('bob@example.com')).toBeInTheDocument()
    expect(screen.getByText('顯示 1 / 2 位教師')).toBeInTheDocument()
    await userEvent.clear(screen.getByLabelText('搜尋教師'))
    await userEvent.type(screen.getByLabelText('搜尋教師'), 'zzz')
    expect(screen.getByText('沒有符合搜尋條件的教師。')).toBeInTheDocument()
  })

  it('renders in English', async () => {
    mockTeachers(['a@example.com'])
    renderApp(<TeachersPage />, { lang: 'en' })
    expect(await screen.findByText('Teachers: 1')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Add teacher' })).toBeInTheDocument()
  })

  it('generates a temporary password and shows it once', async () => {
    mockTeachers(['a@example.com'])
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
    await userEvent.click(screen.getByRole('button', { name: '我已記下，關閉' }))
    await waitFor(() => expect(screen.queryByDisplayValue('ABCD2345WXYZ')).not.toBeInTheDocument())
  })

  it('shows an error in the dialog when generating fails', async () => {
    mockTeachers(['a@example.com'])
    server.use(http.post('/api/admin/users/reset-password', () => apiError(500, 'internal')))
    render()
    await userEvent.click(await screen.findByRole('button', { name: '重設 a@example.com 的密碼' }))
    await userEvent.click(screen.getByRole('button', { name: '重設密碼' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(screen.queryByLabelText('臨時密碼')).not.toBeInTheDocument()
  })
})
