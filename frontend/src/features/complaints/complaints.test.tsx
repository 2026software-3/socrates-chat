import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

type Posted = { category: string; description: string; conversation_id?: string }

function mockSubmit(onBody: (body: Posted) => void = () => {}) {
  const calls = { count: 0 }
  server.use(
    http.post('/api/complaints', async ({ request }) => {
      calls.count += 1
      onBody((await request.json()) as Posted)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return calls
}

describe('complaint form', () => {
  it('tells users the content is emailed to admins and not to enter passwords or others’ personal data', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/complaint' })
    expect(await screen.findByRole('heading', { name: '回報問題' })).toBeInTheDocument()
    const notice = screen.getByText(/電子郵件寄給系統管理者/)
    expect(notice).toHaveTextContent('密碼')
    expect(notice).toHaveTextContent('個人資料')
    expect(notice).toHaveTextContent('無法追蹤')
  })

  it('submits the category and trimmed description, then shows a confirmation and clears the form', async () => {
    mockMe(makeMe({ student: true }))
    let body: Posted | undefined
    mockSubmit((b) => (body = b))
    renderApp(null, { route: '/complaint' })
    await userEvent.click(await screen.findByRole('radio', { name: '語音問題' }))
    await userEvent.type(screen.getByLabelText('問題描述'), '  語音一直斷線  ')
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已送出')
    expect(body).toEqual({ category: 'voice', description: '語音一直斷線' })
    expect(screen.getByLabelText('問題描述')).toHaveValue('')
  })

  it('does not send an empty description', async () => {
    mockMe(makeMe({ student: true }))
    const calls = mockSubmit()
    renderApp(null, { route: '/complaint' })
    await userEvent.type(await screen.findByLabelText('問題描述'), '   ')
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('請填寫')
    expect(calls.count).toBe(0)
  })

  it('limits the description to 2000 characters and shows the count', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/complaint' })
    const box = await screen.findByLabelText('問題描述')
    expect(box).toHaveAttribute('maxlength', '2000')
    await userEvent.type(box, '你好')
    expect(screen.getByText('2 / 2000')).toBeInTheDocument()
  })

  it('attaches the conversation id only when opened from a conversation, and says so', async () => {
    mockMe(makeMe({ student: true }))
    let body: Posted | undefined
    mockSubmit((b) => (body = b))
    renderApp(null, { route: '/complaint?conversation=11111111-1111-4111-8111-111111111111' })
    expect(await screen.findByText(/不含對話內容/)).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('問題描述'), '回覆很慢')
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    await screen.findByRole('status')
    expect(body).toEqual({
      category: 'other',
      description: '回覆很慢',
      conversation_id: '11111111-1111-4111-8111-111111111111',
    })
  })

  it('sends no conversation id and shows no attachment note without the link parameter', async () => {
    mockMe(makeMe({ student: true }))
    let body: Posted | undefined
    mockSubmit((b) => (body = b))
    renderApp(null, { route: '/complaint' })
    await userEvent.type(await screen.findByLabelText('問題描述'), '登入有問題')
    expect(screen.queryByText(/不含對話內容/)).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    await screen.findByRole('status')
    expect(body).not.toHaveProperty('conversation_id')
  })

  it('shows a translated message when the rate limit is hit and keeps what was typed', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.post('/api/complaints', () => apiError(429, 'complaint_rate_limited')))
    renderApp(null, { route: '/complaint' })
    await userEvent.type(await screen.findByLabelText('問題描述'), '很多問題')
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('送出太頻繁')
    expect(screen.getByLabelText('問題描述')).toHaveValue('很多問題')
  })

  it('lets a teacher report a problem too, from the navigation', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: 'main' })
    await userEvent.click(within(nav).getByRole('link', { name: '回報問題' }))
    expect(await screen.findByLabelText('問題描述')).toBeInTheDocument()
  })

  it('does not show an admin-only account the form or its navigation entry', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(
      http.get('/api/admin/classrooms', () => HttpResponse.json([])),
      http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })),
    )
    renderApp(null, { route: '/complaint' })
    await screen.findByRole('navigation', { name: 'main' })
    expect(screen.queryByLabelText('問題描述')).not.toBeInTheDocument()
    expect(within(screen.getByRole('navigation', { name: 'main' })).queryByRole('link', { name: '回報問題' })).not.toBeInTheDocument()
  })

  it('does not show a pending (no role) account the form', async () => {
    mockMe(makeMe({}))
    renderApp(null, { route: '/complaint' })
    await waitFor(() => expect(screen.queryByLabelText('問題描述')).not.toBeInTheDocument())
  })
})

describe('report link in a conversation', () => {
  it('links the conversation page to the form with the conversation id', async () => {
    const { makeDetail } = await import('@/features/chat/test-data')
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/conversations/c1', () => HttpResponse.json(makeDetail({ id: 'c1' }))),
      http.get('/api/conversations/c1/summary', () => apiError(404, 'not_found')),
    )
    renderApp(null, { route: '/conversations/c1' })
    const link = await screen.findByRole('link', { name: '回報此對話的問題' })
    expect(link).toHaveAttribute('href', '/complaint?conversation=c1')
  })
})
