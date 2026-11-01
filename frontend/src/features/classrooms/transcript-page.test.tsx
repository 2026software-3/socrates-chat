import { screen, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Roles, Transcript } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

function transcript(over: Partial<Transcript> = {}): Transcript {
  return {
    conversation_id: 'c1',
    classroom_id: 'room-1',
    title: '電車難題',
    language: 'zh-TW',
    ended_at: '2026-03-05T10:00:00Z',
    student_id: 's1',
    student_name: '王小明',
    messages: [
      { id: 'm1', role: 'student', content: '我會拉桿。', source: 'text', question_type: null, created_at: '2026-03-05T09:00:00Z' },
      { id: 'm2', role: 'ai', content: '為什麼五個人比一個人重要？', source: 'text', question_type: 'clarify', created_at: '2026-03-05T09:00:05Z' },
      { id: 'm3', role: 'student', content: '因為傷害較少。', source: 'speech-browser', question_type: null, created_at: '2026-03-05T09:01:00Z' },
    ],
    ...over,
  }
}

function setup(data: Transcript | Response = transcript(), roles: Partial<Roles> = { teacher: true }, prefix = '/teacher') {
  server.use(
    http.get('/api/summaries/c1/messages', () => (data instanceof Response ? data : HttpResponse.json(data))),
    http.get('/api/classrooms', () => HttpResponse.json([])),
  )
  mockMe(makeMe(roles))
  return renderApp(null, { route: `${prefix}/classrooms/room-1/conversations/c1` })
}

describe('TranscriptPage', () => {
  it('shows the whole conversation in order, labelled by who spoke', async () => {
    setup()
    expect(await screen.findByRole('heading', { name: '電車難題' })).toBeInTheDocument()
    const items = within(screen.getByRole('list', { name: '對話內容' })).getAllByRole('listitem')
    expect(items).toHaveLength(3)
    expect(within(items[0]).getByText('王小明')).toBeInTheDocument()
    expect(within(items[0]).getByText('我會拉桿。')).toBeInTheDocument()
    expect(within(items[1]).getByText('AI')).toBeInTheDocument()
    expect(within(items[1]).getByText('為什麼五個人比一個人重要？')).toBeInTheDocument()
    expect(within(items[2]).getByText('語音輸入')).toBeInTheDocument()
  })

  it('is read-only: no message box and no send button', async () => {
    setup()
    await screen.findByRole('heading', { name: '電車難題' })
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '送出' })).not.toBeInTheDocument()
  })

  it('links back to the student page in the same classroom', async () => {
    setup()
    await screen.findByRole('heading', { name: '電車難題' })
    expect(screen.getByRole('link', { name: '王小明' })).toHaveAttribute('href', '/teacher/classrooms/room-1/students/s1')
  })

  it('shows the fixed string for an admin without any original text', async () => {
    const masked = transcript({
      messages: transcript().messages.map((m) => ({ ...m, content: 'message' })),
    })
    setup(masked, { admin: true }, '/admin')
    await screen.findByRole('heading', { name: '電車難題' })
    expect(screen.getAllByText('message')).toHaveLength(3)
    expect(screen.queryByText('我會拉桿。')).not.toBeInTheDocument()
  })

  it('shows the not-found error when the conversation is not available to this teacher', async () => {
    setup(apiError(404, 'not_found'))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.queryByRole('list', { name: '對話內容' })).not.toBeInTheDocument()
  })
})
