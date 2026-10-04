import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { afterEach, describe, expect, it } from 'vitest'
import { SummariesPage } from '@/features/teacher/summaries-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { TeacherSummary } from '@/lib/types'
import { apiError } from '@/test/render'
import { server } from '@/test/server'

function row(over: Partial<TeacherSummary> = {}): TeacherSummary {
  return {
    conversation_id: 'c1',
    title: '電車難題',
    ended_at: '2026-03-05T10:00:00Z',
    student_id: 's1',
    student_name: '王小明',
    student_email: 'ming@example.com',
    stance: '應該拉桿',
    reasons: '拯救較多人',
    ...over,
  }
}

const rows = [
  row(),
  row({
    conversation_id: 'c2',
    title: '自由意志',
    student_id: 's2',
    student_name: null,
    student_email: 'anon@example.com',
    stance: null,
    reasons: null,
  }),
]

afterEach(() => {
  server.events.removeAllListeners()
})

function cardOf(el: HTMLElement): HTMLElement {
  const li = el.closest('li')
  if (!li) throw new Error('card not found')
  return li
}

function setup(data: TeacherSummary[] = rows) {
  server.use(http.get('/api/teacher/summaries', () => HttpResponse.json(data)))
  return renderTeacherPage(<SummariesPage />)
}

describe('SummariesPage classification', () => {
  it('shows the claim and main school, and the masked text for admins as-is', async () => {
    server.use(
      http.get('/api/teacher/summaries', () =>
        HttpResponse.json([
          row({ claim: '該拉桿', framework: 'deontology' }),
          row({ conversation_id: 'c9', title: '管理者視角', claim: 'message', framework: 'message' }),
        ]),
      ),
    )
    renderTeacherPage(<SummariesPage />)
    const card = (await screen.findByText('電車難題')).closest('li') as HTMLElement
    expect(within(card).getByText('該拉桿')).toBeInTheDocument()
    expect(within(card).getByText('義務論')).toBeInTheDocument()
    const masked = screen.getByText('管理者視角').closest('li') as HTMLElement
    expect(within(masked).getAllByText('message')).toHaveLength(2)
  })
})

describe('SummariesPage', () => {
  it('lists student, topic, localized date and the stance and reasons', async () => {
    setup()
    const card = cardOf(await screen.findByText('王小明'))
    expect(within(card).getByText('ming@example.com')).toBeInTheDocument()
    expect(within(card).getByText('電車難題')).toBeInTheDocument()
    expect(within(card).getByText(/結束於 2026年3月5日/)).toBeInTheDocument()
    expect(within(card).getByText('應該拉桿')).toBeInTheDocument()
    expect(within(card).getByText('拯救較多人')).toBeInTheDocument()
  })

  it('shows placeholders for a missing name and empty fields', async () => {
    setup()
    const card = cardOf(await screen.findByText('anon@example.com'))
    expect(within(card).getByText('（未提供姓名）')).toBeInTheDocument()
    expect(within(card).getAllByText('（無內容）')).toHaveLength(2)
  })

  it('states that teachers cannot see conversation originals and never requests conversations', async () => {
    const seen: string[] = []
    server.events.on('request:start', ({ request }) => {
      seen.push(new URL(request.url).pathname)
    })
    setup()
    await screen.findByText('王小明')
    expect(screen.getByText(/教師看不到學生的對話原文/)).toBeInTheDocument()
    expect(screen.queryByRole('link')).not.toBeInTheDocument()
    expect(seen.some((p) => p.startsWith('/api/conversations'))).toBe(false)
  })

  it('filters by student name, email or title', async () => {
    setup()
    await screen.findByText('王小明')
    const search = screen.getByLabelText('搜尋學生或題目')
    await userEvent.type(search, '自由')
    expect(screen.queryByText('王小明')).not.toBeInTheDocument()
    expect(screen.getByText('anon@example.com')).toBeInTheDocument()
    await userEvent.clear(search)
    await userEvent.type(search, 'MING@')
    expect(screen.getByText('王小明')).toBeInTheDocument()
    expect(screen.queryByText('anon@example.com')).not.toBeInTheDocument()
    await userEvent.clear(search)
    await userEvent.type(search, 'nothing-here')
    expect(screen.getByText('沒有符合的總結。')).toBeInTheDocument()
  })

  it('renders the admin masking string as is', async () => {
    setup([row({ stance: 'message', reasons: 'message' })])
    const card = cardOf(await screen.findByText('王小明'))
    expect(within(card).getAllByText('message')).toHaveLength(2)
  })

  it('shows loading, then the empty state', async () => {
    setup([])
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('還沒有學生完成對話。')).toBeInTheDocument()
  })

  it('shows an error and retries', async () => {
    let calls = 0
    server.use(
      http.get('/api/teacher/summaries', () => {
        calls += 1
        return calls === 1 ? apiError(403, 'forbidden') : HttpResponse.json(rows)
      }),
    )
    renderTeacherPage(<SummariesPage />)
    expect(await screen.findByText('你沒有權限執行這個操作。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('王小明')).toBeInTheDocument()
  })

  it('formats the date in the active language', async () => {
    server.use(http.get('/api/teacher/summaries', () => HttpResponse.json([row()])))
    renderTeacherPage(<SummariesPage />, 'en')
    expect(await screen.findByText(/Ended Mar 5, 2026/)).toBeInTheDocument()
  })
})
