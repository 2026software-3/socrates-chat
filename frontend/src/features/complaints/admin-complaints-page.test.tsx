import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

type Row = {
  id: string
  category: string
  description: string
  conversation_id: string | null
  created_at: string
  status: 'open' | 'resolved'
  mail_status: 'pending' | 'sent' | 'failed' | 'skipped'
  submitter_name: string | null
  submitter_role: 'student' | 'teacher'
  classroom_name: string | null
}

const row = (over: Partial<Row> = {}): Row => ({
  id: 'k1',
  category: 'voice',
  description: '語音一直斷線',
  conversation_id: null,
  created_at: '2026-11-12T03:04:05Z',
  status: 'open',
  mail_status: 'sent',
  submitter_name: '王小明',
  submitter_role: 'student',
  classroom_name: null,
  ...over,
})

/** 依 `?status=` 篩選的假後端，回傳目前所有資料。 */
function mockList(rows: Row[], seen: string[] = []) {
  server.use(
    http.get('/api/admin/complaints', ({ request }) => {
      const status = new URL(request.url).searchParams.get('status')
      seen.push(status ?? 'all')
      return HttpResponse.json(status ? rows.filter((r) => r.status === status) : rows)
    }),
  )
}

describe('admin complaints', () => {
  beforeEach(() => {
    mockMe(makeMe({ admin: true }))
    server.use(
      http.get('/api/admin/classrooms', () => HttpResponse.json([])),
      http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })),
    )
  })

  it('is reachable from the navigation of an admin-only account', async () => {
    mockList([])
    renderApp(null, { route: '/admin' })
    const nav = await screen.findByRole('navigation', { name: 'main' })
    await userEvent.click(within(nav).getByRole('link', { name: '客訴' }))
    expect(await screen.findByRole('heading', { name: '客訴' })).toBeInTheDocument()
  })

  it('lists the raw description with submitter, category, role and mail status — no student number or email', async () => {
    mockList([
      row(),
      row({
        id: 'k2',
        category: 'content',
        description: '題目有錯字',
        submitter_name: null,
        submitter_role: 'teacher',
        conversation_id: '22222222-2222-4222-8222-222222222222',
        classroom_name: '哲學 A 班',
        mail_status: 'failed',
        status: 'resolved',
      }),
    ])
    renderApp(null, { route: '/admin/complaints' })
    expect(await screen.findByText('語音一直斷線')).toBeInTheDocument()
    const first = screen.getByText('語音一直斷線').closest('article')!
    expect(within(first).getByText('語音問題')).toBeInTheDocument()
    expect(within(first).getByText(/王小明/)).toBeInTheDocument()
    expect(within(first).getByText('已寄出')).toBeInTheDocument()
    const second = screen.getByText('題目有錯字').closest('article')!
    expect(within(second).getByText('寄信失敗')).toBeInTheDocument()
    expect(within(second).getByText(/哲學 A 班/)).toBeInTheDocument()
    expect(within(second).getByText(/22222222-2222-4222-8222-222222222222/)).toBeInTheDocument()
    expect(within(second).getByText(/教師/)).toBeInTheDocument()
  })

  it('filters by status through the query string', async () => {
    const seen: string[] = []
    mockList([row(), row({ id: 'k2', description: '已處理的', status: 'resolved' })], seen)
    renderApp(null, { route: '/admin/complaints' })
    await screen.findByText('語音一直斷線')
    await userEvent.click(screen.getByRole('button', { name: '已處理' }))
    await waitFor(() => expect(screen.queryByText('語音一直斷線')).not.toBeInTheDocument())
    expect(screen.getByText('已處理的')).toBeInTheDocument()
    expect(seen).toContain('resolved')
  })

  it('marks a complaint as resolved and then reopens it', async () => {
    const rows = [row()]
    mockList(rows)
    const patches: unknown[] = []
    server.use(
      http.patch('/api/admin/complaints/k1', async ({ request }) => {
        const body = (await request.json()) as { status: Row['status'] }
        patches.push(body)
        rows[0] = { ...rows[0], status: body.status }
        return HttpResponse.json(rows[0])
      }),
    )
    renderApp(null, { route: '/admin/complaints' })
    await userEvent.click(await screen.findByRole('button', { name: '標為已處理' }))
    await userEvent.click(await screen.findByRole('button', { name: '重新開啟' }))
    expect(patches).toEqual([{ status: 'resolved' }, { status: 'open' }])
  })

  it('offers to resend only mails that failed, and reloads afterwards', async () => {
    const rows = [row({ id: 'k1', mail_status: 'sent' }), row({ id: 'k2', description: '寄失敗的', mail_status: 'failed' })]
    mockList(rows)
    let resent = 0
    server.use(
      http.post('/api/admin/complaints/k2/resend', () => {
        resent += 1
        rows[1] = { ...rows[1], mail_status: 'pending' }
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderApp(null, { route: '/admin/complaints' })
    await screen.findByText('寄失敗的')
    expect(screen.getAllByRole('button', { name: '重寄通知信' })).toHaveLength(1)
    await userEvent.click(screen.getByRole('button', { name: '重寄通知信' }))
    expect(await screen.findByText('寄送中')).toBeInTheDocument()
    expect(resent).toBe(1)
  })

  it('explains skipped mail when SMTP is not configured', async () => {
    mockList([row({ mail_status: 'skipped' })])
    renderApp(null, { route: '/admin/complaints' })
    expect(await screen.findByText('未寄信（尚未設定 SMTP）')).toBeInTheDocument()
  })

  it('shows an empty state', async () => {
    mockList([])
    renderApp(null, { route: '/admin/complaints' })
    expect(await screen.findByText('目前沒有客訴。')).toBeInTheDocument()
  })

  it('shows a retryable error when loading fails', async () => {
    server.use(http.get('/api/admin/complaints', () => apiError(500, 'internal')))
    renderApp(null, { route: '/admin/complaints' })
    expect(await screen.findByRole('button', { name: '重試' })).toBeInTheDocument()
  })

  it('is closed to teachers', async () => {
    mockMe(makeMe({ teacher: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/admin/complaints' })
    await screen.findByRole('navigation', { name: 'main' })
    expect(screen.queryByRole('heading', { name: '客訴' })).not.toBeInTheDocument()
  })
})
