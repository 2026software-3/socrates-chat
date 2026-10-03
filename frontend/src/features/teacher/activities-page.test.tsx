import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { toast } from 'sonner'
import { afterEach, describe, expect, it } from 'vitest'
import { ActivitiesPage } from '@/features/teacher/activities-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { Activity, Topic } from '@/lib/types'
import { apiError } from '@/test/render'
import { server } from '@/test/server'

const topics: Topic[] = [
  {
    id: 't1',
    title: '電車難題',
    description: '',
    category: null,
    is_active: true,
  },
  {
    id: 't2',
    title: '自由意志',
    description: '',
    category: null,
    is_active: true,
  },
]

function act(over: Partial<Activity> = {}): Activity {
  return {
    id: 'a1',
    title: '第一週討論',
    description: '請討論公平',
    topic_id: 't1',
    status: 'draft',
    created_at: '2026-03-01T08:00:00Z',
    ...over,
  }
}

// sonner 的 toast 狀態是全域的，避免前一個測試的 toast 殘留
afterEach(() => {
  toast.dismiss()
})

function mockList(list: Activity[]) {
  server.use(
    http.get('/api/activities', () => HttpResponse.json(list)),
    http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
  )
}

function setup() {
  return renderTeacherPage(<ActivitiesPage />)
}

describe('ActivitiesPage', () => {
  it('lists activities with status badge and topic title', async () => {
    mockList([act(), act({ id: 'a2', title: '第二週', status: 'published', topic_id: null })])
    setup()
    expect(await screen.findByText('第一週討論')).toBeInTheDocument()
    expect(screen.getByText('草稿')).toBeInTheDocument()
    expect(screen.getByText('已發布')).toBeInTheDocument()
    expect(screen.getByText('題目：電車難題')).toBeInTheDocument()
  })

  it('shows loading, then an empty state', async () => {
    mockList([])
    setup()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('還沒有任何討論活動，請先建立一個。')).toBeInTheDocument()
  })

  it('shows a translated error and retries', async () => {
    let calls = 0
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => {
        calls += 1
        return calls === 1 ? apiError(500, 'internal') : HttpResponse.json([act()])
      }),
    )
    setup()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('第一週討論')).toBeInTheDocument()
  })

  it('rejects an empty title on the client without calling the server', async () => {
    mockList([])
    let posted = false
    server.use(
      http.post('/api/activities', () => {
        posted = true
        return HttpResponse.json(act(), { status: 201 })
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await userEvent.type(screen.getByLabelText('活動標題'), '   ')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('請輸入活動標題。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })

  it('creates an activity with the chosen topic and shows it as a draft', async () => {
    let list: Activity[] = []
    let body: unknown
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => HttpResponse.json(list)),
      http.post('/api/activities', async ({ request }) => {
        body = await request.json()
        list = [act({ title: '新活動', topic_id: 't2' })]
        return HttpResponse.json(list[0], { status: 201 })
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await userEvent.selectOptions(await screen.findByLabelText('題目（選填）'), 't2')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('活動已建立（草稿）。')).toBeInTheDocument()
    expect(body).toEqual({ title: '新活動', topic_id: 't2' })
    expect(await screen.findByText('草稿')).toBeInTheDocument()
    expect(screen.getByLabelText('活動標題')).toHaveValue('')
  })

  it('does not double submit while creating', async () => {
    mockList([])
    let count = 0
    server.use(
      http.post('/api/activities', async () => {
        count += 1
        await new Promise((r) => setTimeout(r, 60))
        return HttpResponse.json(act(), { status: 201 })
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await userEvent.type(screen.getByLabelText('活動標題'), 'X')
    const btn = screen.getByRole('button', { name: '建立活動' })
    await userEvent.dblClick(btn)
    await waitFor(() => expect(btn).toBeDisabled())
    await waitFor(() => expect(btn).toBeEnabled())
    expect(count).toBe(1)
  })

  it('shows the server error for a failed create', async () => {
    mockList([])
    server.use(http.post('/api/activities', () => apiError(400, 'invalid_topic')))
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await userEvent.type(screen.getByLabelText('活動標題'), 'X')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('題目不存在。')).toBeInTheDocument()
  })

  it('edits an activity in a dialog and sends the changed fields', async () => {
    let list = [act()]
    let body: unknown
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => HttpResponse.json(list)),
      http.patch('/api/activities/a1', async ({ request }) => {
        body = await request.json()
        list = [act({ title: '改名', topic_id: 't2' })]
        return HttpResponse.json(list[0])
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    const title = within(dialog).getByLabelText('活動標題')
    expect(title).toHaveValue('第一週討論')
    await userEvent.clear(title)
    await userEvent.type(title, '改名')
    await userEvent.selectOptions(within(dialog).getByLabelText('題目（選填）'), 't2')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('活動已更新。')).toBeInTheDocument()
    expect(body).toEqual({
      title: '改名',
      description: '請討論公平',
      topic_id: 't2',
    })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(await screen.findByText('改名')).toBeInTheDocument()
  })

  it('omits topic_id when the topic is unchanged', async () => {
    let body: unknown
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => HttpResponse.json([act()])),
      http.patch('/api/activities/a1', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json(act())
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('活動已更新。')).toBeInTheDocument()
    expect(body).toEqual({ title: '第一週討論', description: '請討論公平' })
  })

  it('does not offer to clear the topic when editing an activity that has one', async () => {
    mockList([act()])
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    const select = within(dialog).getByLabelText('題目（選填）')
    await waitFor(() => expect(select).toHaveValue('t1'))
    expect(within(dialog).getByRole('option', { name: '不指定題目' })).toBeDisabled()
  })

  it('keeps an inactive topic selected and labelled instead of showing none', async () => {
    mockList([act({ topic_id: 'gone' })])
    setup()
    expect(await screen.findByText('題目：（已停用或無法載入的題目）')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    expect(within(dialog).getByLabelText('題目（選填）')).toHaveValue('gone')
  })

  it('keeps the dialog open and shows the error when saving fails', async () => {
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => HttpResponse.json([act()])),
      http.patch('/api/activities/a1', () => apiError(400, 'invalid_topic')),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByText('題目不存在。')).toBeInTheDocument()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('shows an error with retry when a reload fails while old data is on screen', async () => {
    let calls = 0
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => {
        calls += 1
        return calls === 2 ? apiError(500, 'internal') : HttpResponse.json([act()])
      }),
      http.post('/api/activities/a1/publish', () => HttpResponse.json(act({ status: 'published' }))),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '發布' }))
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByText('第一週討論')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    await waitFor(() => expect(screen.queryByText('系統發生錯誤，請稍後再試。')).not.toBeInTheDocument())
  })

  it('validates an empty title in the edit dialog', async () => {
    mockList([act()])
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.clear(within(dialog).getByLabelText('活動標題'))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByText('請輸入活動標題。')).toBeInTheDocument()
  })

  it('publishes a draft, closes it, and reopens it', async () => {
    let status: Activity['status'] = 'draft'
    const calls: string[] = []
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => HttpResponse.json([act({ status })])),
      http.post('/api/activities/a1/publish', () => {
        calls.push('publish')
        status = 'published'
        return HttpResponse.json(act({ status }))
      }),
      http.post('/api/activities/a1/close', () => {
        calls.push('close')
        status = 'closed'
        return HttpResponse.json(act({ status }))
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '發布' }))
    expect(await screen.findByText('活動已發布。')).toBeInTheDocument()
    await userEvent.click(await screen.findByRole('button', { name: '停止開放' }))
    expect(await screen.findByText('活動已停止開放。')).toBeInTheDocument()
    await userEvent.click(await screen.findByRole('button', { name: '重新開放' }))
    await waitFor(() => expect(calls).toEqual(['publish', 'close', 'publish']))
    expect(await screen.findByText('已發布')).toBeInTheDocument()
  })

  it('disables the row button while a status change is pending and shows errors', async () => {
    let count = 0
    server.use(
      http.get('/api/available', () => HttpResponse.json({ activities: [], topics })),
      http.get('/api/activities', () => HttpResponse.json([act()])),
      http.post('/api/activities/a1/publish', async () => {
        count += 1
        await new Promise((r) => setTimeout(r, 60))
        return apiError(409, 'conflict')
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    const btn = screen.getByRole('button', { name: '發布' })
    await userEvent.dblClick(btn)
    await waitFor(() => expect(btn).toBeDisabled())
    const row = btn.closest('li')
    if (!row) throw new Error('row not found')
    expect(await within(row).findByRole('alert')).toBeInTheDocument()
    expect(count).toBe(1)
  })

  it('still works when the topic list fails to load', async () => {
    server.use(
      http.get('/api/activities', () => HttpResponse.json([])),
      http.get('/api/available', () => apiError(500, 'internal')),
    )
    setup()
    expect(await screen.findByText('無法載入題目清單，仍可建立不指定題目的活動。')).toBeInTheDocument()
    expect(screen.getByLabelText('活動標題')).toBeEnabled()
  })
})
