import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { AvailablePage } from '@/features/student/available-page'
import { makeConversation } from '@/features/chat/test-data'
import type { Available, StudentActivity, TopicProgress } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

function progress(id: string, overrides: Partial<TopicProgress> = {}): TopicProgress {
  return { topic_id: id, title: `Act ${id}`, conversation_id: null, conversation_status: null, summary_ready: false, ...overrides }
}

function makeActivity(topics: TopicProgress[], overrides: Partial<StudentActivity> = {}): StudentActivity {
  return {
    id: 'act-1',
    title: 'Activity One',
    description: 'Activity desc',
    topic_ids: topics.map((t) => t.topic_id),
    opens_at: null,
    closes_at: null,
    classroom_id: 'room-a',
    classroom_name: '合成 A 班',
    status: 'published',
    is_archived: false,
    created_at: '2026-01-01T00:00:00Z',
    topics,
    completed: topics.length > 0 && topics.every((t) => t.summary_ready),
    ...overrides,
  }
}

const AVAILABLE: Available = {
  activities: [makeActivity([progress('A')])],
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
  server.use(http.get('/api/catalog', () => HttpResponse.json(data)))
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
    const act = await screen.findByRole('region', { name: '教室活動' })
    const bank = screen.getByRole('region', { name: '個人練習' })
    expect(within(act).getByText('Activity One')).toBeInTheDocument()
    expect(within(act).getByText('Activity desc')).toBeInTheDocument()
    expect(within(act).queryByText('Topic One')).not.toBeInTheDocument()
    expect(within(bank).getByText('Topic One')).toBeInTheDocument()
    expect(within(bank).getByText('題庫・Ethics')).toBeInTheDocument()
    expect(within(bank).queryByText('Activity One')).not.toBeInTheDocument()
  })

  it('groups activities under the name of each classroom', async () => {
    mockAvailable({
      activities: [
        makeActivity([progress('A')]),
        makeActivity([progress('B')], { id: 'act-2', title: 'Activity Two', classroom_id: 'room-b', classroom_name: '合成 B 班' }),
        makeActivity([progress('C')], { id: 'act-3', title: 'Activity Three' }),
      ],
      topics: AVAILABLE.topics,
    })
    renderPage()
    const act = await screen.findByRole('region', { name: '教室活動' })
    const roomA = within(act).getByRole('region', { name: '合成 A 班' })
    const roomB = within(act).getByRole('region', { name: '合成 B 班' })
    expect(within(roomA).getByText('Activity One')).toBeInTheDocument()
    expect(within(roomA).getByText('Activity Three')).toBeInTheDocument()
    expect(within(roomA).queryByText('Activity Two')).not.toBeInTheDocument()
    expect(within(roomB).getByText('Activity Two')).toBeInTheDocument()
    expect(within(roomB).queryByText('Activity One')).not.toBeInTheDocument()
  })

  it('orders classroom groups by name, not by the newest activity', async () => {
    mockAvailable({
      // 後端依建立時間新到舊：B 班的活動最新，但分組仍依教室名稱排序
      activities: [
        makeActivity([progress('B')], { id: 'act-2', title: 'Activity Two', classroom_id: 'room-b', classroom_name: '合成 B 班' }),
        makeActivity([progress('A')]),
      ],
      topics: AVAILABLE.topics,
    })
    renderPage()
    const act = await screen.findByRole('region', { name: '教室活動' })
    const names = within(act)
      .getAllByRole('heading', { level: 3 })
      .map((h) => h.textContent)
    expect(names).toEqual(['合成 A 班', '合成 B 班'])
  })

  it('drops a classroom group when search filters out all of its activities', async () => {
    mockAvailable({
      activities: [
        makeActivity([progress('A')]),
        makeActivity([progress('B')], { id: 'act-2', title: 'Activity Two', classroom_id: 'room-b', classroom_name: '合成 B 班' }),
      ],
      topics: AVAILABLE.topics,
    })
    renderPage()
    const act = await screen.findByRole('region', { name: '教室活動' })
    await userEvent.type(screen.getByRole('searchbox'), 'Two')
    expect(within(act).queryByRole('region', { name: '合成 A 班' })).not.toBeInTheDocument()
    expect(within(act).getByRole('region', { name: '合成 B 班' })).toBeInTheDocument()
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
    await userEvent.click(await screen.findByRole('button', { name: '開始討論：Act A' }))
    expect(await screen.findByText('chat page')).toBeInTheDocument()
    expect(body).toEqual({ activity_id: 'act-1', topic_id: 'A' })
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
    expect(screen.getByRole('button', { name: /Act A/ })).toBeEnabled()
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

  it('shows activity_already_started in words, not as a code', async () => {
    mockAvailable()
    server.use(http.post('/api/conversations', () => apiError(409, 'activity_already_started')))
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Act A/ }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      '你已經開始過這個活動，請從你的對話紀錄繼續，或刪除該場對話後重新開始。',
    )
    expect(screen.queryByText('activity_already_started')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Act A/ })).toBeEnabled()
  })

  it('sends the student to the archived view when their only conversation for the activity is archived', async () => {
    mockAvailable()
    const queries: string[] = []
    server.use(
      http.post('/api/conversations', () => apiError(409, 'activity_already_started')),
      http.get('/api/conversations', ({ request }) => {
        queries.push(new URL(request.url).search)
        return HttpResponse.json({ items: [makeConversation({ activity_id: 'act-1', is_archived: true })], next_cursor: null })
      }),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Act A/ }))
    expect(await screen.findByText('list page')).toBeInTheDocument()
    expect(queries).toEqual(['?archived=true&activity_id=act-1&limit=1'])
  })

  it('keeps the message when the existing conversation is not archived', async () => {
    mockAvailable()
    server.use(
      http.post('/api/conversations', () => apiError(409, 'activity_already_started')),
      http.get('/api/conversations', () => HttpResponse.json({ items: [], next_cursor: null })),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Act A/ }))
    expect(await screen.findByRole('alert')).toHaveTextContent('你已經開始過這個活動')
    expect(screen.queryByText('list page')).not.toBeInTheDocument()
  })

  it('shows the fetch error when the reload after a failed start also fails', async () => {
    let loads = 0
    server.use(
      http.get('/api/catalog', () => {
        loads += 1
        return loads === 1 ? HttpResponse.json(AVAILABLE) : apiError(500, 'internal')
      }),
      http.post('/api/conversations', () => apiError(400, 'invalid_conversation_source')),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Act A/ }))
    await waitFor(() => expect(screen.getAllByRole('alert')).toHaveLength(2))
    expect(screen.getByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
  })

  it('shows invalid_conversation_source and reloads the list', async () => {
    let loads = 0
    server.use(
      http.get('/api/catalog', () => {
        loads += 1
        return HttpResponse.json(loads === 1 ? AVAILABLE : { activities: [], topics: AVAILABLE.topics })
      }),
      http.post('/api/conversations', () => apiError(400, 'invalid_conversation_source')),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: /Act A/ }))
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
    server.use(http.get('/api/catalog', () => (fail ? apiError(500, 'internal') : HttpResponse.json(AVAILABLE))))
    renderPage()
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    fail = false
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('Activity One')).toBeInTheDocument()
  })

  it('shows a loading state first', async () => {
    server.use(
      http.get('/api/catalog', async () => {
        await new Promise((r) => setTimeout(r, 50))
        return HttpResponse.json(AVAILABLE)
      }),
    )
    renderPage()
    expect(await screen.findByRole('status')).toHaveTextContent('載入中')
    expect(await screen.findByText('Activity One')).toBeInTheDocument()
  })

  describe('multi-topic activity card', () => {
    const THREE = (done: number) =>
      ['A', 'B', 'C'].map((id, i) =>
        i < done
          ? progress(id, { conversation_id: `c-${id}`, conversation_status: 'ended', summary_ready: true })
          : progress(id),
      )

    it('shows progress and lets the student start the next topic', async () => {
      mockAvailable({ activities: [makeActivity(THREE(2))], topics: [] })
      let body: unknown
      server.use(
        http.post('/api/conversations', async ({ request }) => {
          body = await request.json()
          return HttpResponse.json({ id: 'c-new' }, { status: 201 })
        }),
      )
      renderPage()
      const card = (await screen.findByText('Activity One')).closest('article')!
      expect(within(card).getByText('已完成 2／3 題')).toBeInTheDocument()
      expect(within(card).queryByText('開放中')).toBeInTheDocument()
      expect(within(card).getAllByRole('listitem')).toHaveLength(3)
      expect(within(card).queryByRole('button', { name: '開始討論：Act A' })).not.toBeInTheDocument()
      await userEvent.click(within(card).getByRole('button', { name: '開始討論：Act C' }))
      expect(await screen.findByText('chat page')).toBeInTheDocument()
      expect(body).toEqual({ activity_id: 'act-1', topic_id: 'C' })
    })

    it('offers continue for an active conversation and does not start a new one', async () => {
      const topics = [progress('A', { conversation_id: 'c-A', conversation_status: 'active' }), progress('B')]
      mockAvailable({ activities: [makeActivity(topics)], topics: [] })
      let posts = 0
      server.use(http.post('/api/conversations', () => ((posts += 1), HttpResponse.json({}, { status: 201 }))))
      renderPage()
      expect(await screen.findByText('已完成 0／2 題')).toBeInTheDocument()
      expect(screen.queryByRole('button', { name: '開始討論：Act A' })).not.toBeInTheDocument()
      await userEvent.click(screen.getByRole('button', { name: '繼續：Act A' }))
      expect(await screen.findByText('chat page')).toBeInTheDocument()
      expect(posts).toBe(0)
    })

    it('offers view summary for an ended conversation and does not allow restarting it', async () => {
      const topics = [progress('A', { conversation_id: 'c-A', conversation_status: 'ended', summary_ready: true }), progress('B')]
      mockAvailable({ activities: [makeActivity(topics)], topics: [] })
      renderPage()
      expect(await screen.findByText('已完成 1／2 題')).toBeInTheDocument()
      expect(screen.queryByRole('button', { name: '開始討論：Act A' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: '繼續：Act A' })).not.toBeInTheDocument()
      await userEvent.click(screen.getByRole('button', { name: '查看總結：Act A' }))
      expect(await screen.findByText('chat page')).toBeInTheDocument()
    })

    it('marks the activity as completed when every topic has a summary', async () => {
      mockAvailable({ activities: [makeActivity(THREE(3))], topics: [] })
      renderPage()
      const card = (await screen.findByText('Activity One')).closest('article')!
      expect(within(card).getByText('已完成 3／3 題')).toBeInTheDocument()
      expect(within(card).getByText('活動已完成')).toBeInTheDocument()
      expect(within(card).queryByText('開放中')).not.toBeInTheDocument()
      expect(within(card).queryByRole('button', { name: /開始討論/ })).not.toBeInTheDocument()
      expect(within(card).getAllByRole('button', { name: /查看總結/ })).toHaveLength(3)
    })

    it('does not count an ended conversation whose summary is not ready as done', async () => {
      const topics = [progress('A', { conversation_id: 'c-A', conversation_status: 'ended', summary_ready: false })]
      mockAvailable({ activities: [makeActivity(topics)], topics: [] })
      renderPage()
      expect(await screen.findByText('已完成 0／1 題')).toBeInTheDocument()
      expect(screen.queryByText('活動已完成')).not.toBeInTheDocument()
      expect(screen.getByRole('button', { name: '查看總結：Act A' })).toBeInTheDocument()
    })
  })
})
