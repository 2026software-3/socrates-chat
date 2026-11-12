import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http } from 'msw'
import { describe, expect, it } from 'vitest'
import { DiagnosticsPage } from '@/features/admin/diagnostics-page'
import type { DiagnosticEvent } from '@/features/admin/diagnostics-model'
import { apiError, makeMe, mockMe, pageOf, renderApp } from '@/test/render'
import { server } from '@/test/server'

const REQUEST_ID = '11111111-1111-4111-8111-111111111111'
const USER_ID = '22222222-2222-4222-8222-222222222222'
const CLASSROOM_ID = '33333333-3333-4333-8333-333333333333'

const aiEvent: DiagnosticEvent = {
  occurred_at: '2026-11-12T03:00:00Z',
  request_id: REQUEST_ID,
  kind: 'ai',
  failure_class: 'quota_exceeded',
  function: 'chat_reply',
  route: '/api/conversations/{id}/messages',
  status: 200,
  error_code: 'ai_unavailable',
  user_id: USER_ID,
  classroom_id: CLASSROOM_ID,
  duration_ms: 842,
  retry_count: 2,
  model: 'synthetic-model',
}

const loginEvent: DiagnosticEvent = {
  occurred_at: '2026-11-12T02:00:00Z',
  request_id: '44444444-4444-4444-8444-444444444444',
  kind: 'login',
  failure_class: 'invalid_credentials',
  function: 'password_login',
  route: '/api/auth/login',
  status: 401,
  error_code: 'invalid_credentials',
  user_id: null,
  classroom_id: null,
  duration_ms: null,
  retry_count: null,
  model: null,
}

/** 記錄每次診斷請求的查詢字串；`respond` 決定回應。 */
function mockDiagnostics(respond: (params: URLSearchParams) => Response = () => pageOf([aiEvent, loginEvent])) {
  const queries: URLSearchParams[] = []
  server.use(
    http.get('/api/admin/diagnostics', ({ request }) => {
      const params = new URL(request.url).searchParams
      queries.push(params)
      return respond(params)
    }),
  )
  return queries
}

const last = (queries: URLSearchParams[]) => queries[queries.length - 1]

describe('DiagnosticsPage', () => {
  it('lists failure events with their classification and request ID', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })

    expect(await screen.findByRole('heading', { name: '診斷紀錄' })).toBeInTheDocument()
    const table = await screen.findByRole('table', { name: '診斷事件' })
    const rows = within(table).getAllByRole('row')
    expect(rows).toHaveLength(3)
    for (const text of ['AI', '額度不足', '對話回覆', '200', REQUEST_ID]) {
      expect(within(rows[1]).getByText(text)).toBeInTheDocument()
    }
    for (const text of ['登入', '帳號或密碼不符', '密碼登入', '401']) {
      expect(within(rows[2]).getByText(text)).toBeInTheDocument()
    }
    expect(last(queries).get('limit')).toBe('50')
    expect(screen.getByText(/用 request ID 到伺服器的容器日誌查詢/)).toBeInTheDocument()
  })

  it('shows every event field in the details and only internal identifiers', async () => {
    mockMe(makeMe({ admin: true }))
    mockDiagnostics()
    const user = userEvent.setup()

    const { container } = renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })

    const table = await screen.findByRole('table', { name: '診斷事件' })
    await user.click(within(table).getAllByRole('button', { name: '詳情' })[0])
    for (const text of [
      '/api/conversations/{id}/messages',
      'ai_unavailable',
      '842 ms',
      '2',
      'synthetic-model',
      USER_ID,
      CLASSROOM_ID,
    ]) {
      expect(within(table).getByText(text)).toBeInTheDocument()
    }
    expect(within(table).getByRole('button', { name: '收合' })).toHaveAttribute('aria-expanded', 'true')
    expect(container.textContent).not.toContain('@')

    await user.click(within(table).getByRole('button', { name: '收合' }))
    expect(within(table).queryByText('synthetic-model')).not.toBeInTheDocument()
  })

  it('marks login failures without an account instead of showing blanks', async () => {
    mockMe(makeMe({ admin: true }))
    mockDiagnostics(() => pageOf([loginEvent]))
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })

    await user.click(await screen.findByRole('button', { name: '詳情' }))
    expect(screen.getByText('無對應帳號')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '查看此帳號的紀錄' })).not.toBeInTheDocument()
  })

  it('queries by kind, failure class, request ID and time range', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics()
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })
    await screen.findByRole('table', { name: '診斷事件' })

    await user.selectOptions(screen.getByLabelText('類型'), 'ai')
    await user.selectOptions(screen.getByLabelText('失敗分類'), 'timeout')
    await user.type(screen.getByLabelText('Request ID'), REQUEST_ID)
    await user.type(screen.getByLabelText('起（含）'), '2026-11-01T08:00')
    await user.type(screen.getByLabelText('迄（不含）'), '2026-11-02T08:00')
    await user.click(screen.getByRole('button', { name: '查詢' }))

    await waitFor(() => expect(last(queries).get('kind')).toBe('ai'))
    const query = last(queries)
    expect(query.get('failure_class')).toBe('timeout')
    expect(query.get('request_id')).toBe(REQUEST_ID)
    expect(new Date(query.get('from') ?? '').getTime()).toBe(new Date('2026-11-01T08:00').getTime())
    expect(new Date(query.get('to') ?? '').getTime()).toBe(new Date('2026-11-02T08:00').getTime())
  })

  it('limits failure classes to those of the selected type and drops one that no longer fits', async () => {
    mockMe(makeMe({ admin: true }))
    mockDiagnostics()
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })
    await screen.findByRole('table', { name: '診斷事件' })
    const cls = screen.getByLabelText('失敗分類')
    await user.selectOptions(cls, 'quota_exceeded')

    await user.selectOptions(screen.getByLabelText('類型'), 'login')
    expect(within(cls).queryByRole('option', { name: '額度不足' })).not.toBeInTheDocument()
    expect(within(cls).getByRole('option', { name: '帳號停用' })).toBeInTheDocument()
    expect(cls).toHaveValue('')
  })

  it('rejects a malformed request ID and an inverted time range without querying', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics()
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })
    await screen.findByRole('table', { name: '診斷事件' })
    const before = queries.length

    await user.type(screen.getByLabelText('Request ID'), 'not-a-uuid')
    await user.click(screen.getByRole('button', { name: '查詢' }))
    expect(await screen.findByText('Request ID 格式不正確。')).toBeInTheDocument()

    await user.clear(screen.getByLabelText('Request ID'))
    await user.type(screen.getByLabelText('起（含）'), '2026-11-02T08:00')
    await user.type(screen.getByLabelText('迄（不含）'), '2026-11-01T08:00')
    await user.click(screen.getByRole('button', { name: '查詢' }))
    expect(await screen.findByText('起始時間必須早於結束時間。')).toBeInTheDocument()
    expect(queries).toHaveLength(before)
  })

  it('starts from the filters in the URL and lets the admin remove account and classroom filters', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics()
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, {
      route: `/admin/diagnostics?user_id=${USER_ID}&classroom_id=${CLASSROOM_ID}&kind=ai`,
    })

    expect(await screen.findByText(`帳號（內部識別碼）：${USER_ID}`)).toBeInTheDocument()
    expect(screen.getByText(`教室（內部識別碼）：${CLASSROOM_ID}`)).toBeInTheDocument()
    expect(screen.getByLabelText('類型')).toHaveValue('ai')
    expect(queries[0].get('user_id')).toBe(USER_ID)
    expect(queries[0].get('classroom_id')).toBe(CLASSROOM_ID)

    const chip = screen.getByText(`帳號（內部識別碼）：${USER_ID}`).parentElement as HTMLElement
    await user.click(within(chip).getByRole('button', { name: '移除篩選' }))
    await waitFor(() => expect(last(queries).has('user_id')).toBe(false))
    expect(last(queries).get('classroom_id')).toBe(CLASSROOM_ID)
    expect(screen.queryByText(`帳號（內部識別碼）：${USER_ID}`)).not.toBeInTheDocument()
  })

  it('jumps to the records of the same account, classroom or request from the details', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics()
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })
    await user.click((await screen.findAllByRole('button', { name: '詳情' }))[0])

    await user.click(screen.getByRole('button', { name: '查看此帳號的紀錄' }))
    await waitFor(() => expect(last(queries).get('user_id')).toBe(USER_ID))

    await user.click((await screen.findAllByRole('button', { name: '詳情' }))[0])
    await user.click(screen.getByRole('button', { name: '查看此教室的紀錄' }))
    await waitFor(() => expect(last(queries).get('classroom_id')).toBe(CLASSROOM_ID))
    expect(last(queries).get('user_id')).toBe(USER_ID)

    await user.click((await screen.findAllByRole('button', { name: '詳情' }))[0])
    await user.click(screen.getByRole('button', { name: '只看這個 request' }))
    await waitFor(() => expect(last(queries).get('request_id')).toBe(REQUEST_ID))
  })

  it('clears every filter', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics()
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: `/admin/diagnostics?kind=login&request_id=${REQUEST_ID}` })
    await screen.findByRole('table', { name: '診斷事件' })
    await user.click(screen.getByRole('button', { name: '清除篩選' }))

    await waitFor(() => expect(last(queries).has('kind')).toBe(false))
    expect(last(queries).has('request_id')).toBe(false)
    expect(screen.getByLabelText('Request ID')).toHaveValue('')
  })

  it('loads the next page with the cursor and keeps the first page', async () => {
    mockMe(makeMe({ admin: true }))
    const queries = mockDiagnostics((params) =>
      params.get('cursor') === 'next-1' ? pageOf([loginEvent]) : pageOf([aiEvent], 'next-1'),
    )
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })
    await screen.findByRole('table', { name: '診斷事件' })
    expect(screen.getAllByRole('row')).toHaveLength(2)

    await user.click(screen.getByRole('button', { name: '載入更多' }))
    await waitFor(() => expect(screen.getAllByRole('row')).toHaveLength(3))
    expect(last(queries).get('cursor')).toBe('next-1')
    expect(screen.queryByRole('button', { name: '載入更多' })).not.toBeInTheDocument()
  })

  it('shows an empty state, with different wording when filters are active', async () => {
    mockMe(makeMe({ admin: true }))
    mockDiagnostics(() => pageOf([]))

    const { unmount } = renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })
    expect(await screen.findByText('沒有失敗紀錄。')).toBeInTheDocument()
    unmount()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics?kind=ai' })
    expect(await screen.findByText('沒有符合條件的失敗紀錄。')).toBeInTheDocument()
  })

  it('shows a loading state while the first page is on its way', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(http.get('/api/admin/diagnostics', () => new Promise<Response>(() => {})))

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })

    expect(await screen.findByRole('status')).toBeInTheDocument()
    expect(screen.queryByRole('table')).not.toBeInTheDocument()
  })

  it('explains an unavailable diagnostics store and recovers on retry', async () => {
    mockMe(makeMe({ admin: true }))
    let calls = 0
    mockDiagnostics(() => {
      calls += 1
      return calls === 1 ? apiError(503, 'diagnostics_unavailable') : pageOf([aiEvent])
    })
    const user = userEvent.setup()

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics' })

    expect(await screen.findByText('診斷資料暫時無法讀取。')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByRole('table', { name: '診斷事件' })).toBeInTheDocument()
    expect(screen.queryByText('診斷資料暫時無法讀取。')).not.toBeInTheDocument()
  })

  it('shows the server-side filter error in plain language', async () => {
    mockMe(makeMe({ admin: true }))
    mockDiagnostics(() => apiError(400, 'invalid_filter'))

    renderApp(<DiagnosticsPage />, { route: '/admin/diagnostics?request_id=oops' })

    expect(await screen.findByText('篩選條件不正確。')).toBeInTheDocument()
  })
})
