import { screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { AvailablePage } from '@/features/student/available-page'
import type { StudentActivity } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const CID = '22222222-2222-4222-8222-222222222222'
const activity: StudentActivity = {
  id: 'act-1',
  title: '合成活動',
  description: '說明',
  topic_ids: ['A'],
  opens_at: null,
  closes_at: null,
  classroom_id: CID,
  classroom_name: '合成教室',
  status: 'published',
  is_archived: false,
  created_at: '2026-01-01T00:00:00Z',
  topics: [{ topic_id: 'A', title: 'Act A', conversation_id: null, conversation_status: null, summary_ready: false }],
  completed: false,
}

function mockCatalog() {
  server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [activity], topics: [] })))
}

describe('classroom AI provider for students', () => {
  it('shows which provider powers the classroom, never a model or a key', async () => {
    mockMe(makeMe({ student: true }))
    mockCatalog()
    server.use(http.get(`/api/classrooms/${CID}/ai-provider`, () => HttpResponse.json({ provider: { id: 'openai', name: 'OpenAI' } })))
    renderApp(<AvailablePage />)
    expect(await screen.findByText('本教室的 AI 由 OpenAI 提供')).toBeInTheDocument()
    expect(document.body.textContent).not.toMatch(/sk-|gpt/)
  })

  it('shows nothing about a provider while the classroom has none (not set or transitional)', async () => {
    mockMe(makeMe({ student: true }))
    mockCatalog()
    renderApp(<AvailablePage />)
    await screen.findAllByText('合成教室')
    expect(screen.queryByText(/本教室的 AI 由/)).not.toBeInTheDocument()
  })

  it('states the general privacy rule', async () => {
    mockMe(makeMe({ student: true }))
    mockCatalog()
    renderApp(<AvailablePage />)
    expect(await screen.findByText('對話會送往教師為該教室選定的 AI 供應商，詳見教室資訊。')).toBeInTheDocument()
  })
})
