import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MetricsPage } from '@/features/admin/metrics-page'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const PROM_CONTENT_TYPE = 'text/plain; version=0.0.4; charset=utf-8'

const SAMPLE_METRICS = [
  '# HELP socrates_build_info Build version information.',
  '# TYPE socrates_build_info gauge',
  'socrates_build_info{version="6fc831d"} 1',
  '# TYPE socrates_process_uptime_seconds gauge',
  'socrates_process_uptime_seconds 7200',
  '# TYPE socrates_maintenance_mode gauge',
  'socrates_maintenance_mode 0',
  '# TYPE socrates_last_cleanup_timestamp_seconds gauge',
  'socrates_last_cleanup_timestamp_seconds 1763773200',
  '# TYPE socrates_db_pool_connections gauge',
  'socrates_db_pool_connections{state="active"} 2',
  'socrates_db_pool_connections{state="idle"} 8',
  'socrates_db_pool_connections{state="max"} 10',
  'socrates_db_pool_connections{state="size"} 10',
  '# TYPE socrates_live_replies_active gauge',
  'socrates_live_replies_active 3',
  '# TYPE socrates_voice_sessions_active gauge',
  'socrates_voice_sessions_active 1',
  '# TYPE socrates_service_up gauge',
  'socrates_service_up{service="asr"} 1',
  'socrates_service_up{service="tts"} 0',
  '# TYPE socrates_http_requests_in_flight gauge',
  'socrates_http_requests_in_flight 4',
  '# TYPE socrates_http_requests_total counter',
  'socrates_http_requests_total{method="GET",route="/api/catalog",status_class="2xx"} 90',
  'socrates_http_requests_total{method="POST",route="/api/conversations/{id}/messages",status_class="5xx"} 2',
  '# TYPE socrates_http_requests_aborted_total counter',
  'socrates_http_requests_aborted_total{method="POST",route="/api/conversations/{id}/messages"} 1',
  '# TYPE socrates_http_request_timeouts_total counter',
  'socrates_http_request_timeouts_total{route="/api/conversations/{id}/messages"} 1',
  '# TYPE socrates_http_request_errors_total counter',
  'socrates_http_request_errors_total{route="/api/conversations/{id}/messages",error_code="ai_unavailable"} 2',
  '# TYPE socrates_http_request_duration_seconds histogram',
  'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="0.1"} 90',
  'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="+Inf"} 90',
  'socrates_http_request_duration_seconds_sum{method="GET",route="/api/catalog"} 4.5',
  'socrates_http_request_duration_seconds_count{method="GET",route="/api/catalog"} 90',
  '# TYPE socrates_ai_requests_total counter',
  'socrates_ai_requests_total{function="chat_reply",outcome="ok"} 19',
  'socrates_ai_requests_total{function="argument_tree",outcome="timeout"} 1',
  '# TYPE socrates_ai_retries_total counter',
  'socrates_ai_retries_total{function="argument_tree"} 2',
  '# TYPE socrates_background_jobs_in_flight gauge',
  'socrates_background_jobs_in_flight{job="summary"} 1',
  '# TYPE socrates_background_jobs_total counter',
  'socrates_background_jobs_total{job="summary",outcome="ok"} 7',
  '# TYPE socrates_service_probes_total counter',
  'socrates_service_probes_total{service="asr",outcome="ok"} 12',
  '# TYPE socrates_complaint_mails_total counter',
  'socrates_complaint_mails_total{outcome="sent"} 3',
  '# TYPE socrates_diagnostic_writes_dropped_total counter',
  'socrates_diagnostic_writes_dropped_total{anonymous="false"} 2',
].join('\n')

function mockMetrics(respond: () => Response = () => new HttpResponse(SAMPLE_METRICS, { status: 200, headers: { 'content-type': PROM_CONTENT_TYPE } })) {
  let calls = 0
  server.use(
    http.get('/api/admin/metrics', () => {
      calls += 1
      return respond()
    }),
  )
  return () => calls
}

function stubMobileViewport() {
  vi.stubGlobal(
    'matchMedia',
    vi.fn((query: string) => ({
      matches: !query.includes('min-width: 1024px'),
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    })),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('MetricsPage', () => {
  it('renders live runtime gauges, HTTP route table, AI functions, jobs, and dependencies', async () => {
    mockMe(makeMe({ admin: true }))
    mockMetrics()

    const { container } = renderApp(<MetricsPage />, { route: '/admin/metrics' })

    expect(await screen.findByRole('heading', { name: '系統指標' })).toBeInTheDocument()
    expect(await screen.findByText('版本 6fc831d')).toBeInTheDocument()
    expect(screen.getByText('已運行 2 小時')).toBeInTheDocument()
    expect(screen.getByText('使用中 2／閒置 8／上限 10')).toBeInTheDocument()
    expect(screen.getByText('asr: 正常')).toBeInTheDocument()
    expect(screen.getByText('tts: 異常')).toBeInTheDocument()

    // Route table on desktop
    const table = screen.getByRole('table', { name: '各路由請求統計' })
    const rows = within(table).getAllByRole('row')
    expect(rows).toHaveLength(3)
    expect(within(rows[1]).getByText('/api/conversations/{id}/messages')).toBeInTheDocument()
    expect(within(rows[2]).getByText('/api/catalog')).toBeInTheDocument()
    expect(within(rows[2]).getByText('50 毫秒')).toBeInTheDocument()

    // Top error codes & per-route aborted badge/note
    expect(screen.getByText('ai_unavailable: 2')).toBeInTheDocument()
    expect(within(rows[1]).getByText(/中途取消 1/)).toBeInTheDocument()

    // Localized AI function names (including argument_tree)
    expect(screen.getByText('對話回覆')).toBeInTheDocument()
    expect(screen.getByText('思考脈絡分析')).toBeInTheDocument()
    expect(screen.getByText('summary')).toBeInTheDocument()

    // Privacy: no emails or raw chat content
    expect(container.textContent).not.toContain('@')
  })

  it('switches the HTTP route table to cards on mobile/tablet viewports (< 1024px)', async () => {
    stubMobileViewport()
    mockMe(makeMe({ admin: true }))
    mockMetrics()

    renderApp(<MetricsPage />, { route: '/admin/metrics' })

    const list = await screen.findByRole('list', { name: '各路由請求統計' })
    expect(screen.queryByRole('table', { name: '各路由請求統計' })).not.toBeInTheDocument()
    expect(within(list).getByText('/api/catalog')).toBeInTheDocument()
    expect(within(list).getByText('/api/conversations/{id}/messages')).toBeInTheDocument()
  })

  it('toggles raw Prometheus exposition text and refreshes on demand', async () => {
    mockMe(makeMe({ admin: true }))
    let calls = 0
    mockMetrics(() => {
      calls += 1
      return new HttpResponse(
        calls === 1 ? SAMPLE_METRICS : SAMPLE_METRICS.replace('socrates_build_info{version="6fc831d"} 1', 'socrates_build_info{version="next999"} 1'),
        { status: 200, headers: { 'content-type': PROM_CONTENT_TYPE } },
      )
    })
    const user = userEvent.setup()

    renderApp(<MetricsPage />, { route: '/admin/metrics' })

    expect(await screen.findByText('版本 6fc831d')).toBeInTheDocument()
    const toggle = screen.getByRole('button', { name: '檢視原始 Prometheus 文字' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    await user.click(toggle)
    expect(screen.getByRole('button', { name: '收合原始 Prometheus 文字' })).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText(/socrates_http_requests_total/)).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '重新整理' }))
    await waitFor(() => expect(screen.getByText('版本 next999')).toBeInTheDocument())
    expect(calls).toBe(2)
  })

  it('shows a Vedute skeleton loading state while waiting', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(
      http.get('/api/admin/metrics', async () => {
        await delay('infinite')
        return new HttpResponse('')
      }),
    )

    renderApp(<MetricsPage />, { route: '/admin/metrics' })
    expect(await screen.findByRole('status')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重新整理中…' })).toBeDisabled()
  })

  it('shows an empty state when no samples are returned', async () => {
    mockMe(makeMe({ admin: true }))
    mockMetrics(() => new HttpResponse('# empty\n', { status: 200, headers: { 'content-type': PROM_CONTENT_TYPE } }))

    renderApp(<MetricsPage />, { route: '/admin/metrics' })
    expect(await screen.findByText('目前尚未收集到指標資料。')).toBeInTheDocument()
  })

  it('shows a recoverable error alert with traceparent when fetching metrics fails and recovers on retry', async () => {
    mockMe(makeMe({ admin: true }))
    const trace = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
    let calls = 0
    mockMetrics(() => {
      calls += 1
      return calls === 1
        ? HttpResponse.json(
            { error: { code: 'internal', request_id: 'req-trace-1' } },
            { status: 500, headers: { traceparent: trace } },
          )
        : new HttpResponse(SAMPLE_METRICS, { status: 200, headers: { 'content-type': PROM_CONTENT_TYPE } })
    })
    const user = userEvent.setup()

    renderApp(<MetricsPage />, { route: '/admin/metrics' })
    const alert = await screen.findByRole('alert')
    expect(within(alert).getByText(`Traceparent：${trace}`)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('版本 6fc831d')).toBeInTheDocument()
  })

  it('caps large route tables to 20 rows by default and expands on demand', async () => {
    mockMe(makeMe({ admin: true }))
    const lines = ['# TYPE socrates_http_requests_total counter']
    for (let i = 1; i <= 25; i += 1) {
      lines.push(`socrates_http_requests_total{method="GET",route="/api/r${i}",status_class="2xx"} ${30 - i}`)
    }
    mockMetrics(() => new HttpResponse(lines.join('\n'), { status: 200, headers: { 'content-type': PROM_CONTENT_TYPE } }))
    const user = userEvent.setup()

    renderApp(<MetricsPage />, { route: '/admin/metrics' })
    const table = await screen.findByRole('table', { name: '各路由請求統計' })
    // 1 header + 20 data rows
    expect(within(table).getAllByRole('row')).toHaveLength(21)

    await user.click(screen.getByRole('button', { name: '顯示全部 25 條路由' }))
    expect(within(table).getAllByRole('row')).toHaveLength(26)
    await user.click(screen.getByRole('button', { name: '只顯示前 20 條路由' }))
    expect(within(table).getAllByRole('row')).toHaveLength(21)
  })

  it('shows a Vedute success toast when copying raw metrics succeeds', async () => {
    mockMe(makeMe({ admin: true }))
    mockMetrics()
    const user = userEvent.setup()
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValueOnce(undefined)

    renderApp(<MetricsPage />, { route: '/admin/metrics' })
    await screen.findByText('版本 6fc831d')

    await user.click(screen.getByRole('button', { name: '複製原始文字' }))
    expect(writeText).toHaveBeenCalledWith(SAMPLE_METRICS)
    expect(await screen.findByRole('status')).toHaveTextContent('已複製原始 Prometheus 指標文字。')
    expect(screen.getByRole('button', { name: '已複製' })).toBeInTheDocument()
  })

  it('shows a Vedute error toast when clipboard copy fails', async () => {
    mockMe(makeMe({ admin: true }))
    mockMetrics()
    const user = userEvent.setup()
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValueOnce(new Error('denied'))

    renderApp(<MetricsPage />, { route: '/admin/metrics' })
    await screen.findByText('版本 6fc831d')

    await user.click(screen.getByRole('button', { name: '複製原始文字' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('無法複製到剪貼簿，請手動選取內容。')
  })
})
