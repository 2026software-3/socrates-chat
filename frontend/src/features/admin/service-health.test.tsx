import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { AdminHome } from '@/features/admin/admin-home'
import type { HealthView } from '@/features/admin/service-health-model'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const health = (overrides: Partial<HealthView> = {}): HealthView => ({
  status: 'healthy',
  period_days: 1,
  database: { status: 'healthy', response_time_ms: 3 },
  services: { total: 4, healthy: 3, unhealthy: 1, unknown: 0 },
  pool: { size: 4, idle: 3, max: 10 },
  process: { started_at: '2026-11-20T00:00:00Z', uptime_seconds: 3 * 86400 + 4 * 3600, version: 'abc1234' },
  maintenance: false,
  cleanup: { last_success_at: '2026-11-22T01:00:00Z' },
  stats_available: true,
  requests: { total: 12345, error_5xx_ratio: 0.0123, p95_ms: 480 },
  ...overrides,
})

function mockHealth(build: (days: string) => HealthView | Response = () => health()) {
  const periods: string[] = []
  server.use(
    http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })),
    http.get('/api/dashboard/class', () => HttpResponse.json({ masked: false, students_total: 0, students_participating: 0, completed: 0, activities: [] })),
    http.get('/api/topics', () => HttpResponse.json([])),
    http.get('/api/admin/status', () =>
      HttpResponse.json({
        service: 'ok',
        database: 'ok',
        period: '24h',
        diagnostics_available: false,
        statistics: null,
        recent_errors: [],
      }),
    ),
    http.get('/api/admin/health', ({ request }) => {
      const days = new URL(request.url).searchParams.get('period_days') ?? ''
      periods.push(days)
      const body = build(days)
      return body instanceof Response ? body : HttpResponse.json(body)
    }),
  )
  return periods
}

function block(): HTMLElement {
  return screen.getByRole('region', { name: '服務健康度' })
}

describe('service health block on the admin home page', () => {
  it('shows the overall status and every indicator for the last 24 hours', async () => {
    mockMe(makeMe({ admin: true }))
    const periods = mockHealth()
    renderApp(<AdminHome />)

    expect(await within(block()).findByText('4 個中 3 個正常')).toBeInTheDocument()
    expect(periods).toEqual(['1'])
    const b = within(block())
    expect(b.getByTestId('overall-status')).toHaveTextContent('正常')
    expect(b.getByText('回應 3 毫秒')).toBeInTheDocument()
    expect(b.getByRole('link', { name: '查看服務' })).toHaveAttribute('href', '/admin/services')
    expect(b.getByText('使用中 1／閒置 3／上限 10')).toBeInTheDocument()
    expect(b.getByText('已運行 3 天 4 小時')).toBeInTheDocument()
    expect(b.getByText('版本 abc1234')).toBeInTheDocument()
    expect(b.getByText('關閉')).toBeInTheDocument()
    expect(b.getByText('12,345')).toBeInTheDocument()
    expect(b.getByText('1.23%')).toBeInTheDocument()
    expect(b.getByText('480 毫秒')).toBeInTheDocument()
  })

  it.each([
    ['degraded', '降級'],
    ['unhealthy', '異常'],
  ] as const)('shows the %s overall status', async (status, label) => {
    mockMe(makeMe({ admin: true }))
    mockHealth(() => health({ status }))
    renderApp(<AdminHome />)
    await waitFor(() => expect(within(block()).getByTestId('overall-status')).toHaveTextContent(label))
  })

  it('follows the existing period selector', async () => {
    mockMe(makeMe({ admin: true }))
    const periods = mockHealth((days) => health({ period_days: Number(days) }))
    const user = userEvent.setup()
    renderApp(<AdminHome />)
    await within(block()).findByText('已運行 3 天 4 小時')
    await user.click(screen.getByRole('button', { name: '最近 7 天' }))
    await user.click(screen.getByRole('button', { name: '最近 30 天' }))
    await waitFor(() => expect(periods).toEqual(['1', '7', '30']))
  })

  it('shows "no data yet" for request items when the period has no requests', async () => {
    mockMe(makeMe({ admin: true }))
    mockHealth(() => health({ requests: null }))
    renderApp(<AdminHome />)
    await within(block()).findByText('已運行 3 天 4 小時')
    expect(within(block()).getAllByText('尚無資料')).toHaveLength(3)
  })

  it('keeps the block usable when statistics cannot be read', async () => {
    mockMe(makeMe({ admin: true }))
    mockHealth(() => health({ stats_available: false, requests: null, database: { status: 'unhealthy', response_time_ms: null } }))
    renderApp(<AdminHome />)
    expect(await within(block()).findByText('統計資料暫時無法讀取。')).toBeInTheDocument()
    expect(within(block()).getAllByText('異常').length).toBeGreaterThan(0)
    // 其餘首頁內容不受影響
    expect(screen.getByRole('heading', { name: '系統概況' })).toBeInTheDocument()
  })

  it('shows a recoverable error with the public health fallback when the request fails', async () => {
    mockMe(makeMe({ admin: true }))
    let calls = 0
    mockHealth(() => (++calls === 1 ? apiError(400, 'invalid_health_period') : health()))
    server.use(
      http.get('/health', () => HttpResponse.json({ status: 'ok' })),
      http.get('/ready', () => HttpResponse.json({ error: { code: 'database_unavailable' } }, { status: 503 })),
    )
    const user = userEvent.setup()
    renderApp(<AdminHome />)
    expect(await within(block()).findByRole('alert')).toBeInTheDocument()
    expect(await within(block()).findByText('無法讀取服務健康度；以下是公開健康檢查的結果。')).toBeInTheDocument()
    await user.click(within(block()).getByRole('button', { name: '重試' }))
    expect(await within(block()).findByText('已運行 3 天 4 小時')).toBeInTheDocument()
  })

  it('shows a loading state while waiting', async () => {
    mockMe(makeMe({ admin: true }))
    mockHealth()
    server.use(
      http.get('/api/admin/health', async () => {
        await delay('infinite')
        return HttpResponse.json({})
      }),
    )
    renderApp(<AdminHome />)
    expect(await within(block()).findByRole('status')).toBeInTheDocument()
  })
})
