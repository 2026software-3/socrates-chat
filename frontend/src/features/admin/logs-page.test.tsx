import { act, screen, waitFor, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it, vi } from 'vitest'
import { LogsPage } from '@/features/admin/logs-page'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'

const entry = {
  created_at: '2026-10-26T12:34:56Z',
  request_id: '11111111-1111-4111-8111-111111111111',
  method: 'GET',
  route: '/api/dashboard/class',
  status: 503,
  duration_ms: 42,
  error_code: 'internal',
}

describe('LogsPage', () => {
  it('shows the in-memory request log fields and retention limits', async () => {
    server.use(
      http.get('/api/admin/logs', () => HttpResponse.json({ items: [entry], capacity: 500 })),
    )

    renderApp(<LogsPage />)

    expect(await screen.findByRole('heading', { name: '程式執行日誌' })).toBeInTheDocument()
    expect(screen.getByText(/最多保留最近 500 筆/)).toBeInTheDocument()
    const table = screen.getByRole('table', { name: '近期 request log' })
    const row = within(table).getAllByRole('row')[1]
    for (const text of ['GET', '/api/dashboard/class', '503', '42 ms', 'internal', entry.request_id]) {
      expect(within(row).getByText(text)).toBeInTheDocument()
    }
  })

  it('refreshes the log every five seconds', async () => {
    let calls = 0
    server.use(
      http.get('/api/admin/logs', () => {
        calls += 1
        return HttpResponse.json({ items: [], capacity: 500 })
      }),
    )
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderApp(<LogsPage />)
      await waitFor(() => expect(calls).toBe(1))
      await act(() => vi.advanceTimersByTimeAsync(5000))
      await waitFor(() => expect(calls).toBe(2))
    } finally {
      vi.useRealTimers()
    }
  })
})
