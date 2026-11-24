import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { ServicesPage } from '@/features/admin/services-page'
import type { ServiceRow, ServicesView } from '@/features/admin/service-health-model'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const row = (overrides: Partial<ServiceRow>): ServiceRow => ({
  name: 'asr',
  label: { 'zh-TW': '語音辨識', en: 'Speech recognition' },
  impact: { 'zh-TW': '語音輸入', en: 'Voice input' },
  target: 'cascade-whisper:8101/openapi.json',
  status: 'healthy',
  failure: null,
  response_time_ms: 42,
  checked_at: new Date().toISOString(),
  last_healthy_at: '2026-11-22T01:00:00Z',
  status_since: new Date(Date.now() - 2 * 3600_000).toISOString(),
  availability: 0.9987,
  recent_events: [{ status: 'healthy', occurred_at: '2026-11-22T00:00:00Z', failure: null }],
  ...overrides,
})

const DATABASE = row({
  name: 'database',
  label: 'PostgreSQL',
  impact: { 'zh-TW': '全站' },
  target: null,
  availability: 1,
})
const TTS = row({
  name: 'tts',
  label: '語音合成',
  impact: '語音回覆',
  target: 'cascade-tts:8100/health',
  status: 'unhealthy',
  failure: 'timeout',
  response_time_ms: null,
  availability: 0.5,
  recent_events: [
    { status: 'unhealthy', occurred_at: '2026-11-22T00:30:00Z', failure: 'timeout' },
    { status: 'healthy', occurred_at: '2026-11-22T00:00:00Z', failure: null },
  ],
})
const CHECKING = row({ name: 'adapter', label: '轉接', impact: '回覆', status: 'checking', availability: null, recent_events: [] })

const view = (services: ServiceRow[], extra: Partial<ServicesView> = {}): ServicesView => ({
  services,
  history_available: true,
  period_days: 1,
  ...extra,
})

function mockServices(build: (days: string) => ServicesView | Response) {
  const periods: string[] = []
  server.use(
    http.get('/api/admin/services', ({ request }) => {
      const days = new URL(request.url).searchParams.get('period_days') ?? ''
      periods.push(days)
      const body = build(days)
      return body instanceof Response ? body : HttpResponse.json(body)
    }),
  )
  return periods
}

const card = (name: string) => screen.getByText(name).closest('[data-slot="card"]') as HTMLElement

describe('services page', () => {
  it('lists unhealthy services first with their details, in the viewer language order', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([DATABASE, row({ name: 'asr' }), TTS]))
    renderApp(<ServicesPage />)

    await screen.findByText('語音合成')
    const names = screen
      .getAllByRole('listitem')
      .map((li) => li.querySelector('[data-slot="card-title"]')?.textContent)
    expect(names).toEqual(['語音合成', 'PostgreSQL', '語音辨識'])
    const tts = within(card('語音合成'))
    expect(tts.getByText('異常 · 逾時')).toBeInTheDocument()
    expect(tts.getByText('語音回覆', { exact: false })).toBeInTheDocument()
    expect(tts.getByText('cascade-tts:8100/health')).toBeInTheDocument()
    expect(tts.getByText('50%')).toBeInTheDocument()
    expect(tts.getByText(/異常已持續/)).toBeInTheDocument()
    const asr = within(card('語音辨識'))
    expect(asr.getByText('42 毫秒')).toBeInTheDocument()
    expect(asr.getByText('99.87%')).toBeInTheDocument()
    expect(asr.getByText(/正常已持續 2 小時/)).toBeInTheDocument()
  })

  it('switches the period', async () => {
    mockMe(makeMe({ admin: true }))
    const periods = mockServices(() => view([DATABASE]))
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    await screen.findByText('PostgreSQL')
    await user.click(screen.getByRole('button', { name: '最近 7 天' }))
    await user.click(screen.getByRole('button', { name: '最近 30 天' }))
    await waitFor(() => expect(periods).toEqual(['1', '7', '30']))
  })

  it('filters by status and says so when nothing matches', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([DATABASE, TTS, CHECKING]))
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    await screen.findByText('語音合成')
    await user.click(screen.getByRole('button', { name: '異常' }))
    expect(screen.getByText('語音合成')).toBeInTheDocument()
    expect(screen.queryByText('PostgreSQL')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '檢查中／尚無資料' }))
    expect(screen.getByText('轉接')).toBeInTheDocument()
    expect(screen.queryByText('語音合成')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '正常' }))
    expect(screen.getByText('PostgreSQL')).toBeInTheDocument()
  })

  it('expands the recent status changes', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([TTS]))
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    await screen.findByText('語音合成')
    await user.click(screen.getByRole('button', { name: '最近的狀態變化' }))
    const events = within(card('語音合成')).getAllByRole('listitem')
    expect(events).toHaveLength(2)
    expect(events[0]).toHaveTextContent('異常')
    expect(events[0]).toHaveTextContent('逾時')
    expect(events[1]).toHaveTextContent('正常')
  })

  it('checks now: shows progress then the new result, without losing availability', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([TTS]))
    let calls = 0
    let release!: () => void
    const responseReady = new Promise<void>((resolve) => (release = resolve))
    server.use(
      http.post('/api/admin/services/check', async ({ request }) => {
        calls += 1
        expect(await request.json()).toEqual({ name: 'tts' })
        await responseReady
        return HttpResponse.json(
          row({ ...TTS, status: 'healthy', failure: null, response_time_ms: 7, availability: null, recent_events: [] }),
        )
      }),
    )
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    await screen.findByText('語音合成')
    const button = screen.getByRole('button', { name: '立即檢查 語音合成' })
    await user.click(button)
    try {
      expect(screen.getByRole('button', { name: '立即檢查 語音合成' })).toBeDisabled()
      expect(within(card('語音合成')).getAllByText('檢查中…').length).toBeGreaterThan(0)
    } finally {
      release()
    }
    await waitFor(() => expect(within(card('語音合成')).getByText('7 毫秒')).toBeInTheDocument())
    expect(within(card('語音合成')).getByText('50%')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '立即檢查 語音合成' })).toBeEnabled()
    expect(calls).toBe(1)
  })

  it('shows an understandable message on rate limit', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([TTS]))
    server.use(http.post('/api/admin/services/check', () => apiError(429, 'service_check_rate_limited')))
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    await screen.findByText('語音合成')
    await user.click(screen.getByRole('button', { name: '立即檢查 語音合成' }))
    expect(await screen.findByText('檢查太頻繁，請 10 秒後再試。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '立即檢查 語音合成' })).toBeEnabled()
  })

  it('explains how to configure services when only the database is monitored', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([DATABASE]))
    renderApp(<ServicesPage />)
    expect(await screen.findByText(/MONITORED_SERVICES/)).toBeInTheDocument()
    expect(screen.getByText('PostgreSQL')).toBeInTheDocument()
  })

  it('still shows current status when history is unavailable', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([TTS], { history_available: false }))
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    expect(await screen.findByText('語音合成')).toBeInTheDocument()
    expect(screen.getAllByText(/狀態資料暫時無法讀取/).length).toBeGreaterThan(0)
    expect(screen.getByText('異常 · 逾時')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '最近的狀態變化' }))
    expect(within(card('語音合成')).queryAllByRole('listitem')).toHaveLength(0)
  })

  it('shows a recoverable error and a loading state', async () => {
    mockMe(makeMe({ admin: true }))
    let calls = 0
    mockServices(() => (++calls === 1 ? apiError(500, 'internal') : view([DATABASE])))
    const user = userEvent.setup()
    renderApp(<ServicesPage />)
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('PostgreSQL')).toBeInTheDocument()
  })

  it('shows the checking and no-data states', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([CHECKING, row({ name: 'x', label: 'X', impact: 'Y', status: 'no_data' })]))
    renderApp(<ServicesPage />)
    expect(await screen.findByText('轉接')).toBeInTheDocument()
    expect(within(card('轉接')).getByText('檢查中')).toBeInTheDocument()
    expect(within(card('X')).getByText('尚無資料')).toBeInTheDocument()
  })

  it('never renders anything beyond host and path for the target', async () => {
    mockMe(makeMe({ admin: true }))
    mockServices(() => view([row({ name: 'asr' })]))
    renderApp(<ServicesPage />)
    await screen.findByText('語音辨識')
    expect(document.body.textContent).not.toMatch(/secret|token|password|http:\/\//i)
  })
})
