import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { describe, expect, it } from 'vitest'
import { LangSwitcher } from '@/components/lang-switcher'
import { TopicDetailPage } from '@/features/student/topic-detail-page'
import { AvailablePage } from '@/features/student/available-page'
import { makeConversation } from '@/features/chat/test-data'
import type { Available, Topic } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const topic = (over: Partial<Topic> = {}): Topic => ({
  id: 'top-1',
  title: 'Trolley problem',
  description: '',
  is_active: true,
  title_pending: false,
  pending_questions: [],
  guiding_questions: ['Who is responsible?', 'Does intent matter?'],
  ...over,
})

function mockCatalog(topics: Topic[], activities: Available['activities'] = []) {
  server.use(http.get('/api/catalog', () => HttpResponse.json({ activities, topics })))
}

function renderDetail(route = '/topics/top-1') {
  mockMe(makeMe({ student: true }))
  return renderApp(
    <Routes>
      <Route path="/" element={<AvailablePage />} />
      <Route path="/topics/:topicId" element={<TopicDetailPage />} />
      <Route path="/conversations/:id" element={<p>chat page</p>} />
    </Routes>,
    { route },
  )
}

describe('student topic detail page', () => {
  it('shows the title, guiding questions and a start button', async () => {
    mockCatalog([topic()])
    renderDetail()
    expect(await screen.findByRole('heading', { level: 1, name: 'Trolley problem' })).toBeInTheDocument()
    const section = screen.getByRole('region', { name: '引導問題' })
    const items = within(section).getAllByRole('listitem')
    expect(items.map((li) => li.textContent)).toEqual(['Who is responsible?', 'Does intent matter?'])
    expect(screen.getByRole('button', { name: '開始討論' })).toBeEnabled()
  })

  it('loads the English title and ordered questions when the student switches language', async () => {
    const me = makeMe({ student: true })
    mockMe(me)
    server.use(
      http.patch('/api/me', () => HttpResponse.json({ ...me, language: 'en' })),
      http.get('/api/catalog', ({ request }) => {
        const english = new URL(request.url).searchParams.get('language') === 'en'
        return HttpResponse.json({ activities: [], topics: [topic(english ? {
          title: 'A choice', guiding_questions: ['Who decides?', 'Why?'],
        } : { title: '選擇', guiding_questions: ['誰決定？', '為什麼？'] })] })
      }),
    )
    renderApp(<><LangSwitcher /><Routes><Route path="/topics/:topicId" element={<TopicDetailPage />} /></Routes></>, { route: '/topics/top-1' })
    await screen.findByRole('heading', { name: '選擇' })
    await userEvent.selectOptions(screen.getByRole('combobox'), 'en')
    await screen.findByRole('heading', { name: 'A choice' })
    expect(screen.getAllByRole('listitem').map((item) => item.textContent)).toEqual(['Who decides?', 'Why?'])
  })

  it('marks only the guiding question awaiting translation', async () => {
    mockCatalog([{ ...topic({ guiding_questions: ['Who is responsible?', '意圖重要嗎？'] }), pending_questions: [1] }])
    renderDetail()
    const items = await screen.findAllByRole('listitem')
    expect(within(items[0]).queryByRole('status')).toBeNull()
    expect(within(items[1]).getByRole('status')).toHaveTextContent('翻譯尚待完成')
  })

  it('hides the guiding questions block when the topic has none', async () => {
    mockCatalog([topic({ guiding_questions: [] })])
    renderDetail()
    await screen.findByRole('heading', { level: 1, name: 'Trolley problem' })
    expect(screen.queryByRole('region', { name: '引導問題' })).not.toBeInTheDocument()
    expect(screen.queryByText('引導問題')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '開始討論' })).toBeInTheDocument()
  })

  it('does not show the deprecated topic content', async () => {
    mockCatalog([topic({ description: 'Old content' })])
    renderDetail()
    await screen.findByRole('heading', { level: 1, name: 'Trolley problem' })
    expect(screen.queryByText('Old content')).not.toBeInTheDocument()
  })

  it('starts a practice conversation from the topic bank and opens it', async () => {
    mockCatalog([topic()])
    let body: unknown
    server.use(
      http.post('/api/conversations', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json(makeConversation({ id: 'c1' }), { status: 201 })
      }),
    )
    renderDetail()
    await userEvent.click(await screen.findByRole('button', { name: '開始討論' }))
    expect(await screen.findByText('chat page')).toBeInTheDocument()
    expect(body).toEqual({ topic_id: 'top-1' })
  })

  it('starts within the activity when opened from an activity topic', async () => {
    mockCatalog([topic()])
    let body: unknown
    server.use(
      http.post('/api/conversations', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json(makeConversation({ id: 'c1' }), { status: 201 })
      }),
    )
    renderDetail('/topics/top-1?activity=act-1')
    await userEvent.click(await screen.findByRole('button', { name: '開始討論' }))
    await screen.findByText('chat page')
    expect(body).toEqual({ activity_id: 'act-1', topic_id: 'top-1' })
  })

  it('shows a not-found message for an unknown topic', async () => {
    mockCatalog([topic()])
    renderDetail('/topics/missing')
    expect(await screen.findByText('找不到這個題目，它可能已被停用。')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '開始討論' })).not.toBeInTheDocument()
  })

  it('shows a start error inline and keeps the page', async () => {
    mockCatalog([topic()])
    server.use(http.post('/api/conversations', () => apiError(500, 'internal')))
    renderDetail()
    await userEvent.click(await screen.findByRole('button', { name: '開始討論' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(screen.getByRole('button', { name: '開始討論' })).toBeEnabled()
  })

  it('is reachable from the practice card on the available page', async () => {
    mockCatalog([topic()])
    renderDetail('/')
    const bank = await screen.findByRole('region', { name: '個人練習' })
    await userEvent.click(within(bank).getByRole('link', { name: '查看題目：Trolley problem' }))
    expect(await screen.findByRole('heading', { level: 1, name: 'Trolley problem' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: '引導問題' })).toBeInTheDocument()
  })

  it('is reachable from an activity topic row, keeping the activity', async () => {
    mockCatalog([topic()], [
      {
        id: 'act-1', title: 'Activity One', description: '', topic_ids: ['top-1'], opens_at: null, closes_at: null,
        classroom_id: 'room-a', classroom_name: '合成 A 班', status: 'published', is_archived: false,
        created_at: '2026-01-01T00:00:00Z', completed: false,
        topics: [{ topic_id: 'top-1', title: 'Trolley problem', conversation_id: null, conversation_status: null, summary_ready: false, title_pending: false }],
      },
    ])
    renderDetail('/')
    const act = await screen.findByRole('region', { name: '教室活動' })
    expect(within(act).getByRole('link', { name: '查看題目：Trolley problem' })).toHaveAttribute('href', '/topics/top-1?activity=act-1')
  })

  it('offers to continue instead of starting again when this activity topic already has a conversation', async () => {
    mockCatalog([topic()], [
      {
        id: 'act-1', title: 'Activity One', description: '', topic_ids: ['top-1'], opens_at: null, closes_at: null,
        classroom_id: 'room-a', classroom_name: '合成 A 班', status: 'published', is_archived: false,
        created_at: '2026-01-01T00:00:00Z', completed: false,
        topics: [{ topic_id: 'top-1', title: 'Trolley problem', conversation_id: 'c9', conversation_status: 'active', summary_ready: false, title_pending: false }],
      },
    ])
    renderDetail('/topics/top-1?activity=act-1')
    await screen.findByRole('heading', { level: 1, name: 'Trolley problem' })
    expect(screen.queryByRole('button', { name: '開始討論' })).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: /Trolley problem/ }))
    expect(await screen.findByText('chat page')).toBeInTheDocument()
  })
})
