import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { afterEach, describe, expect, it } from 'vitest'
import { SummariesPage } from '@/features/teacher/summaries-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { Page, TeacherSummary } from '@/lib/types'
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

function page(items: TeacherSummary[], next_cursor: string | null = null): Page<TeacherSummary> {
  return { items, next_cursor }
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
  server.use(http.get('/api/summaries', () => HttpResponse.json(page(data))))
  return renderTeacherPage(<SummariesPage />)
}

describe('SummariesPage classification', () => {
  it('shows the claim and main school, and the masked text for admins as-is', async () => {
    server.use(
      http.get('/api/summaries', () =>
        HttpResponse.json(
          page([
            row({ claim: '該拉桿', framework: 'deontology' }),
            row({ conversation_id: 'c9', title: '管理者視角', claim: 'message', framework: 'message' }),
          ]),
        ),
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

  it('sends the search to the API and shows the server results, not a client-side filter', async () => {
    const seen: (string | null)[] = []
    server.use(
      http.get('/api/summaries', ({ request }) => {
        const q = new URL(request.url).searchParams.get('q')
        seen.push(q)
        return HttpResponse.json(page(q ? [rows[1]] : [rows[0]]))
      }),
    )
    renderTeacherPage(<SummariesPage />)
    await screen.findByText('王小明')
    // 目前這頁沒有的學生，也要搜得到（結果由後端決定）
    await userEvent.type(screen.getByLabelText('搜尋學生或題目'), '  anon@  {enter}')
    expect(await screen.findByText('anon@example.com')).toBeInTheDocument()
    expect(screen.queryByText('王小明')).not.toBeInTheDocument()
    expect(seen).toEqual([null, 'anon@'])
  })

  it('shows the no-match state, keeps the search box, and clearing it restores the list', async () => {
    server.use(
      http.get('/api/summaries', ({ request }) => {
        const q = new URL(request.url).searchParams.get('q')
        return HttpResponse.json(page(q ? [] : rows))
      }),
    )
    renderTeacherPage(<SummariesPage />)
    await screen.findByText('王小明')
    const search = screen.getByLabelText('搜尋學生或題目')
    await userEvent.type(search, 'nothing-here{enter}')
    expect(await screen.findByText('沒有符合的總結。')).toBeInTheDocument()
    expect(screen.queryByText('還沒有學生完成對話。')).not.toBeInTheDocument()
    await userEvent.clear(search)
    await userEvent.keyboard('{enter}')
    expect(await screen.findByText('王小明')).toBeInTheDocument()
  })

  it('goes back to the first page when the search changes', async () => {
    const seen: { cursor: string | null; q: string | null }[] = []
    server.use(
      http.get('/api/summaries', ({ request }) => {
        const params = new URL(request.url).searchParams
        seen.push({ cursor: params.get('cursor'), q: params.get('q') })
        if (params.get('q')) return HttpResponse.json(page([rows[1]]))
        return HttpResponse.json(params.get('cursor') ? page([rows[1]]) : page([rows[0]], 'c2'))
      }),
    )
    renderTeacherPage(<SummariesPage />)
    await screen.findByText('王小明')
    await userEvent.click(screen.getByRole('button', { name: '下一頁' }))
    await screen.findByText('anon@example.com')
    expect(screen.getByRole('button', { name: '上一頁' })).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('搜尋學生或題目'), 'anon{enter}')
    await waitFor(() => expect(seen.at(-1)).toEqual({ cursor: null, q: 'anon' }))
    expect(screen.queryByRole('button', { name: '上一頁' })).not.toBeInTheDocument()
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
      http.get('/api/summaries', () => {
        calls += 1
        return calls === 1 ? apiError(403, 'forbidden') : HttpResponse.json(page(rows))
      }),
    )
    renderTeacherPage(<SummariesPage />)
    expect(await screen.findByText('你沒有權限執行這個操作。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('王小明')).toBeInTheDocument()
  })

  it('navigates next and previous pages using the opaque cursor', async () => {
    const cursors: (string | null)[] = []
    server.use(
      http.get('/api/summaries', ({ request }) => {
        const cursor = new URL(request.url).searchParams.get('cursor')
        cursors.push(cursor)
        return HttpResponse.json(cursor ? page([rows[1]]) : page([rows[0]], 'opaque+/='))
      }),
    )
    renderTeacherPage(<SummariesPage />)
    await screen.findByText('王小明')
    expect(screen.queryByRole('button', { name: '上一頁' })).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '下一頁' }))
    expect(await screen.findByText('anon@example.com')).toBeInTheDocument()
    expect(screen.queryByText('王小明')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '下一頁' })).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '上一頁' }))
    expect(await screen.findByText('王小明')).toBeInTheDocument()
    expect(cursors).toEqual([null, 'opaque+/=', null])
  })

  it('formats the date in the active language', async () => {
    server.use(http.get('/api/summaries', () => HttpResponse.json(page([row()]))))
    renderTeacherPage(<SummariesPage />, 'en')
    expect(await screen.findByText(/Ended Mar 5, 2026/)).toBeInTheDocument()
  })
})
