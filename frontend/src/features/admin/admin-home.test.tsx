import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { AdminHome } from '@/features/admin/admin-home'
import type { DiagnosticEvent, Statistics, SystemStatus } from '@/features/admin/diagnostics-model'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const REQUEST_ID = '11111111-1111-4111-8111-111111111111'

const event: DiagnosticEvent = {
  occurred_at: '2026-11-12T03:00:00Z',
  request_id: REQUEST_ID,
  kind: 'ai',
  failure_class: 'rate_limited',
  function: 'chat_reply',
  route: '/api/conversations/{id}/messages',
  status: 200,
  error_code: 'ai_unavailable',
  user_id: null,
  classroom_id: null,
  duration_ms: 120,
  retry_count: 1,
  model: 'synthetic-model',
}

const STATS: Statistics = {
  ai_status: 'has_failures',
  ai_calls: 12,
  ai_failures: 3,
  ai_invalid_outputs: 1,
  login_failures: 4,
  account_locks: 2,
}

const status = (overrides: Partial<SystemStatus> = {}): SystemStatus => ({
  service: 'ok',
  database: 'ok',
  period: '24h',
  diagnostics_available: true,
  statistics: STATS,
  recent_errors: [event],
  ...overrides,
})

/** 記錄每次狀態請求用的期間。 */
function mockStatus(build: (period: string) => SystemStatus | Response = () => status()) {
  const periods: string[] = []
  server.use(
    http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })),
    http.get('/api/dashboard/class', () => HttpResponse.json({ students_total: 0 })),
    http.get('/api/topics', () => HttpResponse.json([])),
    http.get('/api/admin/status', ({ request }) => {
      const period = new URL(request.url).searchParams.get('period') ?? ''
      periods.push(period)
      const body = build(period)
      return body instanceof Response ? body : HttpResponse.json(body)
    }),
  )
  return periods
}

function card(title: string): HTMLElement {
  const found = screen.getByText(title).closest('[data-slot="card"]')
  expect(found).not.toBeNull()
  return found as HTMLElement
}

it('shows the number of active topics on the admin home page', async () => {
  mockMe(makeMe({ admin: true }))
  mockStatus()
  server.use(
    http.get('/api/topics', () =>
      HttpResponse.json([
        { id: '1', title: 'Active one', is_active: true },
        { id: '2', title: 'Active two', is_active: true },
        { id: '3', title: 'Inactive', is_active: false },
      ]),
    ),
  )

  renderApp(<AdminHome />)
  expect(await within(card('啟用中的題目')).findByText('2')).toBeInTheDocument()
})

describe('system overview', () => {
  it('shows service, database, AI status and the failure counts for the last 24 hours', async () => {
    mockMe(makeMe({ admin: true }))
    const periods = mockStatus()

    renderApp(<AdminHome />)

    expect(await screen.findByRole('heading', { name: '系統概況' })).toBeInTheDocument()
    expect(await screen.findByText('有失敗')).toBeInTheDocument()
    expect(periods).toEqual(['24h'])
    expect(screen.getByRole('button', { name: '最近 24 小時' })).toHaveAttribute('aria-pressed', 'true')
    expect(within(card('AI 失敗次數')).getByText('3')).toBeInTheDocument()
    expect(within(card('AI 失敗次數')).getByText('期間內共 12 次呼叫')).toBeInTheDocument()
    expect(within(card('登入失敗')).getByText('4')).toBeInTheDocument()
    expect(within(card('帳號鎖定')).getByText('2')).toBeInTheDocument()
    expect(within(card('AI 輸出無法解析')).getByText('1')).toBeInTheDocument()
    expect(screen.getAllByText('正常').length).toBeGreaterThanOrEqual(2)
  })

  it('switches the statistics period to 7 or 30 days', async () => {
    mockMe(makeMe({ admin: true }))
    const periods = mockStatus((period) =>
      status({ period: period as '24h', statistics: { ...STATS, ai_failures: period === '7d' ? 9 : 3 } }),
    )
    const user = userEvent.setup()

    renderApp(<AdminHome />)
    await screen.findByText('有失敗')
    await user.click(screen.getByRole('button', { name: '最近 7 天' }))
    await waitFor(() => expect(within(card('AI 失敗次數')).getByText('9')).toBeInTheDocument())
    await user.click(screen.getByRole('button', { name: '最近 30 天' }))
    await waitFor(() => expect(periods).toEqual(['24h', '7d', '30d']))
    expect(screen.getByRole('button', { name: '最近 30 天' })).toHaveAttribute('aria-pressed', 'true')
  })

  it('says there is no data yet when the AI was not called in the period', async () => {
    mockMe(makeMe({ admin: true }))
    mockStatus(() =>
      status({
        statistics: {
          ai_status: 'no_data',
          ai_calls: 0,
          ai_failures: 0,
          ai_invalid_outputs: 0,
          login_failures: 0,
          account_locks: 0,
        },
        recent_errors: [],
      }),
    )

    renderApp(<AdminHome />)

    expect(await screen.findByText('尚無資料')).toBeInTheDocument()
    expect(screen.getByText('這段期間沒有錯誤。')).toBeInTheDocument()
    // 沒有失敗就沒有詳情連結
    expect(screen.queryByRole('link', { name: /查看詳情/ })).not.toBeInTheDocument()
  })

  it('links failure counts to the diagnostics page for the same period', async () => {
    mockMe(makeMe({ admin: true }))
    mockStatus()

    renderApp(<AdminHome />)

    const ai = await screen.findByRole('link', { name: 'AI 失敗次數 — 查看詳情' })
    const url = new URL(ai.getAttribute('href') ?? '', 'http://localhost')
    expect(url.pathname).toBe('/admin/diagnostics')
    expect(url.searchParams.get('kind')).toBe('ai')
    const from = new Date(url.searchParams.get('from') ?? '').getTime()
    expect(Math.abs(Date.now() - from - 24 * 3_600_000)).toBeLessThan(60_000)
    const lock = screen.getByRole('link', { name: '帳號鎖定 — 查看詳情' })
    expect(lock.getAttribute('href')).toContain('kind=account_lock')
    const invalid = screen.getByRole('link', { name: 'AI 輸出無法解析 — 查看詳情' })
    expect(invalid.getAttribute('href')).toContain('failure_class=invalid_output')
  })

  it('lists recent errors with a link to the request in the diagnostics page', async () => {
    mockMe(makeMe({ admin: true }))
    mockStatus()

    renderApp(<AdminHome />)

    const link = await screen.findByRole('link', { name: REQUEST_ID })
    expect(link).toHaveAttribute('href', `/admin/diagnostics?request_id=${REQUEST_ID}`)
    const row = link.closest('li') as HTMLElement
    expect(within(row).getByText('頻率限制')).toBeInTheDocument()
    expect(within(row).getByText('對話回覆')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '查看全部診斷' })).toHaveAttribute('href', '/admin/diagnostics')
  })

  it('keeps service and database status when diagnostics cannot be read', async () => {
    mockMe(makeMe({ admin: true }))
    mockStatus(() => status({ diagnostics_available: false, statistics: null, recent_errors: [] }))

    renderApp(<AdminHome />)

    expect(await screen.findByText('診斷資料暫時無法讀取。')).toBeInTheDocument()
    expect(screen.getAllByText('正常').length).toBeGreaterThanOrEqual(2)
    expect(screen.queryByText('近期錯誤')).not.toBeInTheDocument()
  })

  it('falls back to the public health endpoints when the status request fails', async () => {
    mockMe(makeMe({ admin: true }))
    mockStatus(() => apiError(401, 'unauthorized'))
    server.use(
      http.get('/health', () => HttpResponse.json({ status: 'ok' })),
      http.get('/ready', () => apiError(503, 'database_unavailable')),
    )

    renderApp(<AdminHome />)

    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(await screen.findByText('無法讀取系統狀態；以下是公開健康檢查的結果。')).toBeInTheDocument()
    const fallback = screen.getByText('無法讀取系統狀態；以下是公開健康檢查的結果。').closest('[data-slot="card"]')
    expect(within(fallback as HTMLElement).getByText('異常')).toBeInTheDocument()
    expect(within(fallback as HTMLElement).getByText('正常')).toBeInTheDocument()
    // 其餘維運入口仍可用
    expect(screen.getByText('啟用中的題目')).toBeInTheDocument()
  })

  it('offers a retry when the status request fails', async () => {
    mockMe(makeMe({ admin: true }))
    let calls = 0
    mockStatus(() => {
      calls += 1
      return calls === 1 ? (apiError(500, 'internal')) : status()
    })
    server.use(
      http.get('/health', () => HttpResponse.json({ status: 'ok' })),
      http.get('/ready', () => HttpResponse.json({ status: 'ok' })),
    )
    const user = userEvent.setup()

    renderApp(<AdminHome />)

    await user.click(await screen.findByRole('button', { name: '重試' }))
    expect(await screen.findByText('有失敗')).toBeInTheDocument()
  })

  it('never shows sensitive identifiers in the overview', async () => {
    mockMe(makeMe({ admin: true }))
    mockStatus(() =>
      status({ recent_errors: [{ ...event, user_id: '22222222-2222-4222-8222-222222222222' }] }),
    )

    const { container } = renderApp(<AdminHome />)
    await screen.findByText('近期錯誤')

    expect(container.textContent).not.toContain('22222222-2222-4222-8222-222222222222')
    expect(container.textContent).not.toContain('@')
  })
})
