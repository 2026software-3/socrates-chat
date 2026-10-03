import { fireEvent, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { AvailablePage } from '@/features/student/available-page'
import type { Topic } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const AVAILABLE: Topic[] = [
  { id: 'top-1', title: 'Topic One', description: 'Topic desc', category: 'Ethics', is_active: true },
  { id: 'top-2', title: 'Topic Two', description: 'Other desc', category: null, is_active: true },
]

function mockAvailable(data: Topic[] = AVAILABLE) {
  server.use(http.get('/api/available', () => HttpResponse.json(data)))
}

function renderPage() {
  mockMe(makeMe({ student: true }))
  return renderApp(
    <Routes>
      <Route path="/available" element={<AvailablePage />} />
      <Route path="/conversations" element={<p>list page</p>} />
      <Route path="/conversations/:id" element={<p>chat page</p>} />
    </Routes>,
    { route: '/available' },
  )
}

describe('student available page', () => {
  it('lists the available topics with their description and category', async () => {
    mockAvailable()
    renderPage()
    expect(await screen.findByText('Topic One')).toBeInTheDocument()
    expect(screen.getByText('Topic desc')).toBeInTheDocument()
    expect(screen.getByText('Ethics')).toBeInTheDocument()
    expect(screen.getByText('Topic Two')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '我的對話' })).toHaveAttribute('href', '/conversations')
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
    expect(screen.getByRole('button', { name: /Topic Two/ })).toBeEnabled()
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
    await userEvent.click(await screen.findByRole('button', { name: /Topic Two/ }))
    await waitFor(() => expect(screen.getAllByRole('alert')).toHaveLength(2))
    expect(screen.getByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
  })

  it('shows invalid_source and reloads the list', async () => {
    let loads = 0
    server.use(
      http.get('/api/available', () => {
        loads += 1
        return HttpResponse.json(loads === 1 ? AVAILABLE : [AVAILABLE[0]])
      }),
      http.post('/api/conversations', () => apiError(400, 'invalid_source')),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Topic Two/ }))
    expect(await screen.findByRole('alert')).toHaveTextContent('請選擇可用的題目。')
    await waitFor(() => expect(screen.queryByText('Topic Two')).not.toBeInTheDocument())
    expect(loads).toBe(2)
    expect(screen.getByRole('button', { name: /Topic One/ })).toBeEnabled()
  })

  it('shows an empty state when nothing is available', async () => {
    mockAvailable([])
    renderPage()
    expect(await screen.findByText('目前沒有可討論的題目，請稍後再來看看。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    let fail = true
    server.use(http.get('/api/available', () => (fail ? apiError(500, 'internal') : HttpResponse.json(AVAILABLE))))
    renderPage()
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    fail = false
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('Topic Two')).toBeInTheDocument()
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
    expect(await screen.findByText('Topic Two')).toBeInTheDocument()
  })
})
