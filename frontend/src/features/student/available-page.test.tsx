import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { AvailablePage } from '@/features/student/available-page'
import type { Available } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const AVAILABLE: Available = {
  activities: [
    {
      id: 'act-1',
      title: 'Activity One',
      description: 'Activity desc',
      topic_id: null,
      status: 'published',
      created_at: '2026-01-01T00:00:00Z',
    },
  ],
  topics: [
    {
      id: 'top-1',
      title: 'Topic One',
      description: 'Topic desc',
      category: 'Ethics',
      is_active: true,
    },
  ],
}

function mockAvailable(data: Available = AVAILABLE) {
  server.use(http.get('/api/available', () => HttpResponse.json(data)))
}

function renderPage() {
  mockMe(makeMe({ student: true }))
  return renderApp(
    <Routes>
      <Route path="/" element={<AvailablePage />} />
      <Route path="/conversations" element={<p>list page</p>} />
      <Route path="/conversations/:id" element={<p>chat page</p>} />
    </Routes>,
    { route: '/' },
  )
}

describe('student available page', () => {
  it('shows teacher activities and the topic bank as distinguishable sections', async () => {
    mockAvailable()
    renderPage()
    const act = await screen.findByRole('region', { name: '教師活動' })
    const bank = screen.getByRole('region', { name: '題庫題目' })
    expect(within(act).getByText('Activity One')).toBeInTheDocument()
    expect(within(act).getByText('Activity desc')).toBeInTheDocument()
    expect(within(act).queryByText('Topic One')).not.toBeInTheDocument()
    expect(within(bank).getByText('Topic One')).toBeInTheDocument()
    expect(within(bank).getByText('Ethics')).toBeInTheDocument()
    expect(within(bank).queryByText('Activity One')).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: '我的對話' })).toHaveAttribute('href', '/conversations')
  })

  it('starts a conversation from an activity', async () => {
    mockAvailable()
    let body: unknown
    server.use(
      http.post('/api/conversations', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({ id: 'c-1' }, { status: 201 })
      }),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Activity One/ }))
    expect(await screen.findByText('chat page')).toBeInTheDocument()
    expect(body).toEqual({ activity_id: 'act-1' })
  })

  it('starts a conversation from a topic', async () => {
    mockAvailable()
    let body: unknown
    server.use(
      http.post('/api/conversations', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({ id: 'c-2' }, { status: 201 })
      }),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Topic One/ }))
    expect(await screen.findByText('chat page')).toBeInTheDocument()
    expect(body).toEqual({ topic_id: 'top-1' })
  })

  it('does not create two conversations on double click', async () => {
    mockAvailable()
    let calls = 0
    server.use(
      http.post('/api/conversations', async () => {
        calls += 1
        await new Promise((r) => setTimeout(r, 50))
        return HttpResponse.json({ id: 'c-3' }, { status: 201 })
      }),
    )
    renderPage()
    const btn = await screen.findByRole('button', { name: /Topic One/ })
    // 同步連點兩次（React 尚未重繪），只有 ref 防護能擋住第二次
    fireEvent.click(btn)
    fireEvent.click(btn)
    expect(await screen.findByText('chat page')).toBeInTheDocument()
    expect(calls).toBe(1)
  })

  it('shows a network failure and keeps buttons enabled', async () => {
    mockAvailable()
    server.use(http.post('/api/conversations', () => HttpResponse.error()))
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Topic One/ }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Topic One/ })).toBeEnabled()
    expect(screen.getByRole('button', { name: /Activity One/ })).toBeEnabled()
  })

  it('scrolls the start error into view', async () => {
    mockAvailable()
    server.use(http.post('/api/conversations', () => apiError(500, 'internal')))
    const spy = vi.fn()
    Element.prototype.scrollIntoView = spy
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Topic One/ }))
    await screen.findByRole('alert')
    expect(spy).toHaveBeenCalled()
  })

  it('names start buttons with a translated pattern', async () => {
    mockAvailable()
    renderPage()
    expect(await screen.findByRole('button', { name: '開始討論：Topic One' })).toBeInTheDocument()
  })

  it('shows the fetch error when the reload after a failed start also fails', async () => {
    let loads = 0
    server.use(
      http.get('/api/available', () => {
        loads += 1
        return loads === 1 ? HttpResponse.json(AVAILABLE) : apiError(500, 'internal')
      }),
      http.post('/api/conversations', () => apiError(400, 'invalid_source')),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Activity One/ }))
    await waitFor(() => expect(screen.getAllByRole('alert')).toHaveLength(2))
    expect(screen.getByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
  })

  it('shows invalid_source and reloads the list', async () => {
    let loads = 0
    server.use(
      http.get('/api/available', () => {
        loads += 1
        return HttpResponse.json(loads === 1 ? AVAILABLE : { activities: [], topics: AVAILABLE.topics })
      }),
      http.post('/api/conversations', () => apiError(400, 'invalid_source')),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Activity One/ }))
    expect(await screen.findByRole('alert')).toHaveTextContent('請選擇可用的活動或題目。')
    await waitFor(() => expect(screen.queryByText('Activity One')).not.toBeInTheDocument())
    expect(loads).toBe(2)
    expect(screen.getByRole('button', { name: /Topic One/ })).toBeEnabled()
  })

  it('shows an empty state when nothing is available', async () => {
    mockAvailable({ activities: [], topics: [] })
    renderPage()
    expect(await screen.findByText('目前沒有可討論的活動或題目，請稍後再來看看。')).toBeInTheDocument()
  })

  it('shows a per-section empty text when only one list is empty', async () => {
    mockAvailable({ activities: [], topics: AVAILABLE.topics })
    renderPage()
    expect(await screen.findByText('目前沒有開放的教師活動')).toBeInTheDocument()
    expect(screen.getByText('Topic One')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    let fail = true
    server.use(http.get('/api/available', () => (fail ? apiError(500, 'internal') : HttpResponse.json(AVAILABLE))))
    renderPage()
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    fail = false
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('Activity One')).toBeInTheDocument()
  })

  it('shows a loading state first', async () => {
    server.use(
      http.get('/api/available', async () => {
        await new Promise((r) => setTimeout(r, 50))
        return HttpResponse.json(AVAILABLE)
      }),
    )
    renderPage()
    expect(await screen.findByRole('status')).toHaveTextContent('載入中')
    expect(await screen.findByText('Activity One')).toBeInTheDocument()
  })
})
