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

  it('puts the log inside a scroll area so long logs can be scrolled up and down', async () => {
    server.use(
      http.get('/api/admin/logs', () => HttpResponse.json({ items: [entry], capacity: 500 })),
    )

    renderApp(<LogsPage />)

    const table = await screen.findByRole('table', { name: '近期 request log' })
    const viewport = table.closest('[data-slot="scroll-area-viewport"]')
    expect(viewport).not.toBeNull()
    expect(viewport?.closest('[data-slot="scroll-area"]')).not.toBeNull()
  })

  it('keeps the log on screen and does not cancel a poll that takes longer than five seconds', async () => {
    const later = { ...entry, request_id: '22222222-2222-4222-8222-222222222222', route: '/api/admin/later' }
    let calls = 0
    let slowPollAborted = false
    let releaseSlowPoll: () => void = () => {}
    const slowPollReleased = new Promise<void>((resolve) => {
      releaseSlowPoll = resolve
    })
    server.use(
      http.get('/api/admin/logs', async ({ request }) => {
        calls += 1
        if (calls === 1) return HttpResponse.json({ items: [entry], capacity: 500 })
        if (calls === 2) {
          request.signal.addEventListener('abort', () => {
            slowPollAborted = true
          })
          await slowPollReleased
          return HttpResponse.json({ items: [later], capacity: 500 })
        }
        return HttpResponse.json({ items: [], capacity: 500 })
      }),
    )
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderApp(<LogsPage />)
      expect(await screen.findByText(entry.request_id)).toBeInTheDocument()

      // 第 5 秒開始的輪詢一直沒有回應；之後兩個輪詢間隔都過去了
      await act(() => vi.advanceTimersByTimeAsync(5000))
      await waitFor(() => expect(calls).toBe(2))
      await act(() => vi.advanceTimersByTimeAsync(10000))

      expect(calls).toBe(2)
      expect(slowPollAborted).toBe(false)
      expect(screen.getByText(entry.request_id)).toBeInTheDocument()

      releaseSlowPoll()
      expect(await screen.findByText(later.request_id)).toBeInTheDocument()
    } finally {
      vi.useRealTimers()
    }
  })

  it('starts a fresh poll when one has hung without ever answering', async () => {
    const later = { ...entry, request_id: '33333333-3333-4333-8333-333333333333' }
    let calls = 0
    server.use(
      http.get('/api/admin/logs', async () => {
        calls += 1
        if (calls === 2) await new Promise(() => {})
        return HttpResponse.json({ items: [calls === 1 ? entry : later], capacity: 500 })
      }),
    )
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderApp(<LogsPage />)
      expect(await screen.findByText(entry.request_id)).toBeInTheDocument()
      await act(() => vi.advanceTimersByTimeAsync(5000))
      await waitFor(() => expect(calls).toBe(2))
      await act(() => vi.advanceTimersByTimeAsync(40000))
      expect(await screen.findByText(later.request_id)).toBeInTheDocument()
    } finally {
      vi.useRealTimers()
    }
  })

  it('stops polling once the session is rejected', async () => {
    let calls = 0
    server.use(
      http.get('/api/admin/logs', () => {
        calls += 1
        if (calls === 1) return HttpResponse.json({ items: [entry], capacity: 500 })
        return HttpResponse.json({ error: { code: 'unauthorized', request_id: 'r1' } }, { status: 401 })
      }),
    )
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      renderApp(<LogsPage />)
      expect(await screen.findByText(entry.request_id)).toBeInTheDocument()
      await act(() => vi.advanceTimersByTimeAsync(5000))
      await waitFor(() => expect(calls).toBe(2))
      await act(() => vi.advanceTimersByTimeAsync(30000))
      expect(calls).toBe(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not poll while the tab is hidden', async () => {
    let calls = 0
    server.use(
      http.get('/api/admin/logs', () => {
        calls += 1
        return HttpResponse.json({ items: [entry], capacity: 500 })
      }),
    )
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true)
    try {
      renderApp(<LogsPage />)
      expect(await screen.findByText(entry.request_id)).toBeInTheDocument()
      await act(() => vi.advanceTimersByTimeAsync(20000))
      expect(calls).toBe(1)
    } finally {
      hidden.mockRestore()
      vi.useRealTimers()
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
