import { act, screen, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { AiUsageMeter } from '@/features/usage/ai-usage-meter'
import { markAiUsageStale } from '@/lib/ai-usage-events'
import type { AiUsage } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const usage = (over: Partial<AiUsage> = {}): AiUsage => ({
  used: 12,
  limit: 200,
  window_secs: 86_400,
  resets_in_secs: 7_000,
  ...over,
})

/** 依序回傳；最後一個會一直重複。 */
function mockUsage(...list: AiUsage[]) {
  let i = 0
  server.use(http.get('/api/me/usage', () => HttpResponse.json(list[Math.min(i++, list.length - 1)])))
}

function renderMeter(lang: 'zh-TW' | 'en' | 'es' = 'zh-TW') {
  mockMe(makeMe({ student: true }, { language: lang }))
  return renderApp(<AiUsageMeter />, { route: '/', lang })
}

describe('AiUsageMeter', () => {
  it('shows used / limit as an accessible meter', async () => {
    mockUsage(usage())
    renderMeter()
    const meter = await screen.findByRole('meter', { name: 'AI 用量' })
    expect(meter).toHaveAttribute('aria-valuenow', '12')
    expect(meter).toHaveAttribute('aria-valuemin', '0')
    expect(meter).toHaveAttribute('aria-valuemax', '200')
    expect(screen.getByText('12 / 200')).toBeInTheDocument()
    expect(screen.queryByText('即將用完')).not.toBeInTheDocument()
  })

  it('warns when 80% or more is used', async () => {
    mockUsage(usage({ used: 160 }))
    renderMeter()
    expect(await screen.findByText('即將用完')).toBeInTheDocument()
  })

  it('says it is used up and when a slot frees (hours, then minutes)', async () => {
    mockUsage(usage({ used: 200, resets_in_secs: 7_000 }), usage({ used: 200, resets_in_secs: 600 }))
    renderMeter()
    expect(await screen.findByText('AI 用量已用完')).toBeInTheDocument()
    expect(screen.getByText('約 2 小時後釋出額度')).toBeInTheDocument()
    act(() => markAiUsageStale())
    expect(await screen.findByText('約 10 分鐘後釋出額度')).toBeInTheDocument()
  })

  it('reloads when something that may use the AI happened', async () => {
    mockUsage(usage({ used: 12 }), usage({ used: 13 }))
    renderMeter()
    expect(await screen.findByText('12 / 200')).toBeInTheDocument()
    act(() => markAiUsageStale())
    await waitFor(() => expect(screen.getByText('13 / 200')).toBeInTheDocument())
  })

  it('is translated', async () => {
    mockUsage(usage({ used: 200, resets_in_secs: 7_000 }))
    renderMeter('en')
    expect(await screen.findByText('AI usage limit reached')).toBeInTheDocument()
    expect(screen.getByRole('meter', { name: 'AI usage' })).toBeInTheDocument()
    expect(screen.getByText('A slot frees up in about 2 h')).toBeInTheDocument()
  })

  it('renders nothing when the usage cannot be loaded', async () => {
    server.use(http.get('/api/me/usage', () => HttpResponse.json({ error: { code: 'internal', request_id: 'r' } }, { status: 500 })))
    renderMeter()
    await waitFor(() => expect(screen.queryByRole('meter')).not.toBeInTheDocument())
  })
})
