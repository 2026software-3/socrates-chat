import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { toast } from 'sonner'
import { afterEach, describe, expect, it } from 'vitest'
import { ActivitiesPanel } from '@/features/teacher/activities-page'
import { classroom, mockClassrooms, ROOM_A, renderTeacherPage } from '@/features/teacher/test-helpers'
import type { Activity, Topic } from '@/lib/types'
import { apiError, pageOf } from '@/test/render'
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
    classroom_id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
    title: '第一週討論',
    description: '請討論公平',
    topic_id: 't1',
    opens_at: null,
    closes_at: null,
    status: 'draft',
    is_archived: false,
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
    http.get('/api/activities', () => pageOf(list)),
    http.get('/api/topics', () => HttpResponse.json(topics)),
  )
}

function setup() {
  mockClassrooms()
  return renderTeacherPage(<ActivitiesPanel />)
}

describe('ActivitiesPanel', () => {
  it('lists activities with status badge and topic title', async () => {
    mockList([act(), act({ id: 'a2', title: '第二週', status: 'published', topic_id: 't2' })])
    setup()
    expect(await screen.findByText('第一週討論')).toBeInTheDocument()
    expect(screen.getByText('草稿')).toBeInTheDocument()
    expect(screen.getByText('已發布')).toBeInTheDocument()
    expect(within(screen.getByRole('table')).getByText('電車難題')).toBeInTheDocument()
    expect(within(screen.getByRole('table')).getByText('自由意志')).toBeInTheDocument()
  })

  it('loads the next page of activities on demand', async () => {
    const cursors: (string | null)[] = []
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', ({ request }) => {
        const cursor = new URL(request.url).searchParams.get('cursor')
        cursors.push(cursor)
        return cursor === 'next' ? pageOf([act({ id: 'a2', title: '第二頁的活動' })]) : pageOf([act()], 'next')
      }),
    )
    setup()
    expect(await screen.findByText('第一週討論')).toBeInTheDocument()
    expect(screen.queryByText('第二頁的活動')).not.toBeInTheDocument()
    expect(cursors).toEqual([null])
    await userEvent.click(screen.getByRole('button', { name: '載入更多' }))
    expect(await screen.findByText('第二頁的活動')).toBeInTheDocument()
    expect(screen.getByText('第一週討論')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '載入更多' })).not.toBeInTheDocument()
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
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => {
        calls += 1
        return calls === 1 ? apiError(500, 'internal') : pageOf([act()])
      }),
    )
    setup()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('第一週討論')).toBeInTheDocument()
  })

  it('does not send an activity without a topic and asks for one', async () => {
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
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('請選擇題目。')).toBeInTheDocument()
    expect(screen.getByLabelText('題目')).toHaveAttribute('aria-invalid', 'true')
    expect(posted).toBe(false)
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
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf(list)),
      http.post('/api/activities', async ({ request }) => {
        body = await request.json()
        list = [act({ title: '新活動', topic_id: 't2' })]
        return HttpResponse.json(list[0], { status: 201 })
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await userEvent.selectOptions(await screen.findByLabelText('題目'), 't2')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('活動已建立（草稿）。')).toBeInTheDocument()
    expect(body).toEqual({ title: '新活動', topic_id: 't2', classroom_id: ROOM_A })
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
    await userEvent.selectOptions(screen.getByLabelText('題目'), 't1')
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
    await userEvent.selectOptions(screen.getByLabelText('題目'), 't1')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('題目不存在。')).toBeInTheDocument()
  })

  it('archives an activity out of the default list and deletes it after typing the title', async () => {
    let list = [act()]
    const calls: string[] = []
    let deleteBody: unknown
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', ({ request }) => {
        calls.push(`list:${new URL(request.url).searchParams.get('archived') ?? ''}`)
        return pageOf(list)
      }),
      http.post('/api/activities/a1/archive', () => {
        list = []
        return HttpResponse.json(act({ is_archived: true }))
      }),
      http.delete('/api/activities/a1', async ({ request }) => {
        deleteBody = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '封存' }))
    expect(await screen.findByText('還沒有任何討論活動，請先建立一個。')).toBeInTheDocument()
    // 已封存檢視向後端要已封存的活動，並可直接刪除
    list = [act({ is_archived: true })]
    await userEvent.click(screen.getByRole('button', { name: '已封存' }))
    await screen.findByText('第一週討論')
    expect(calls.at(-1)).toBe('list:true')
    await userEvent.click(screen.getByRole('button', { name: '刪除' }))
    const confirm = screen.getByRole('button', { name: '永久刪除' })
    expect(confirm).toBeDisabled()
    await userEvent.type(screen.getByLabelText('請輸入「第一週討論」以確認'), '第一週討論')
    await userEvent.click(confirm)
    await waitFor(() => expect(deleteBody).toEqual({ confirm_title: '第一週討論' }))
    expect(await screen.findByText('活動已刪除。')).toBeInTheDocument()
  })

  it('does not keep showing the old rows while the archived view is loading', async () => {
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => { release = resolve })
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', async ({ request }) => {
        if (new URL(request.url).searchParams.get('archived') === 'true') {
          await gate
          return pageOf([])
        }
        return pageOf([act()])
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '已封存' }))
    await waitFor(() => expect(screen.queryByText('第一週討論')).not.toBeInTheDocument())
    release()
    expect(await screen.findByText('沒有已封存的活動。')).toBeInTheDocument()
  })

  it('edits an activity in a dialog and sends the changed fields', async () => {
    let list = [act()]
    let body: unknown
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf(list)),
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
    await userEvent.selectOptions(within(dialog).getByLabelText('題目'), 't2')
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
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act()])),
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

  it('offers no empty topic choice while editing', async () => {
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act()])),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    const select = within(dialog).getByLabelText('題目')
    await waitFor(() => expect(select).toHaveValue('t1'))
    expect(within(dialog).getByRole('option', { name: '請選擇題目' })).toBeDisabled()
  })

  it('shows the locked-topic message when the topic of an started activity is changed', async () => {
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act()])),
      http.patch('/api/activities/a1', () => apiError(409, 'activity_topic_locked')),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await userEvent.selectOptions(within(dialog).getByLabelText('題目'), 't2')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByText('此活動已有學生開始對話，不能更換題目。')).toBeInTheDocument()
    expect(screen.queryByText('activity_topic_locked')).not.toBeInTheDocument()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('keeps an inactive topic selected and labelled instead of showing none', async () => {
    mockList([act({ topic_id: 'gone' })])
    setup()
    expect(await within(await screen.findByRole('table')).findByText('（已停用或無法載入的題目）')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    expect(within(dialog).getByLabelText('題目')).toHaveValue('gone')
  })

  it('only offers active topics even though /api/topics lists every topic', async () => {
    server.use(
      http.get('/api/activities', () => pageOf([act({ topic_id: 't3' })])),
      http.get('/api/topics', () =>
        HttpResponse.json([...topics, { id: 't3', title: '已停用題目', description: '', category: null, is_active: false }]),
      ),
    )
    setup()
    const select = await screen.findByLabelText('題目')
    await waitFor(() => expect(within(select).getByRole('option', { name: '電車難題' })).toBeInTheDocument())
    expect(within(select).queryByRole('option', { name: '已停用題目' })).not.toBeInTheDocument()
    expect(await within(await screen.findByRole('table')).findByText('（已停用或無法載入的題目）')).toBeInTheDocument()
  })

  it('keeps the dialog open and shows the error when saving fails', async () => {
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act()])),
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
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => {
        calls += 1
        return calls === 2 ? apiError(500, 'internal') : pageOf([act()])
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
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act({ status })])),
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
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act()])),
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
    const row = btn.closest('tr')
    if (!row) throw new Error('row not found')
    expect(await within(row).findByRole('alert')).toBeInTheDocument()
    expect(count).toBe(1)
  })

  it('shows the frontend-raised message when every classroom is archived', async () => {
    mockList([])
    mockClassrooms([{ ...classroom(ROOM_A, '2026 春季班'), is_archived: true }])
    let posted = false
    server.use(
      http.post('/api/activities', () => {
        posted = true
        return HttpResponse.json(act(), { status: 201 })
      }),
    )
    renderTeacherPage(<ActivitiesPanel />)
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await userEvent.selectOptions(screen.getByLabelText('題目'), 't1')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('請先建立或選擇未封存的教室。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })

  it('treats a classroom that failed to load as not editable', async () => {
    server.use(
      http.get('/api/classrooms', () => apiError(500, 'internal')),
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([act()])),
    )
    renderTeacherPage(<ActivitiesPanel />)
    await screen.findByText('第一週討論')
    expect(screen.getByRole('button', { name: '發布' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '編輯' })).toBeDisabled()
  })

  it('explains a failed topic list and still blocks creating without a topic', async () => {
    let posted = false
    server.use(
      http.get('/api/activities', () => pageOf([])),
      http.get('/api/topics', () => apiError(500, 'internal')),
      http.post('/api/activities', () => {
        posted = true
        return HttpResponse.json(act(), { status: 201 })
      }),
    )
    setup()
    expect(await screen.findByText('無法載入題目清單，請重新整理頁面後再試。')).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('活動標題'), 'X')
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('請選擇題目。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })
})

describe('activity schedules', () => {
  it('rejects an end at or before the start without posting', async () => {
    mockList([])
    let posted = false
    server.use(http.post('/api/activities', () => { posted = true; return HttpResponse.json(act()) }))
    setup()
    await screen.findByLabelText('題目')
    await userEvent.type(screen.getByLabelText('活動標題'), 'scheduled')
    await userEvent.selectOptions(screen.getByLabelText('題目'), 't1')
    fireEvent.change(screen.getByLabelText('開始時間（選填）'), { target: { value: '2030-01-01T10:00' } })
    fireEvent.change(screen.getByLabelText('結束時間（選填）'), { target: { value: '2030-01-01T10:00' } })
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('結束時間必須晚於開始時間。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })

  it('rejects newly picked schedule times in the past and sets min to now', async () => {
    mockList([])
    let posted = false
    server.use(http.post('/api/classrooms/:id/activities', () => { posted = true; return HttpResponse.json(act()) }))
    setup()
    await screen.findByLabelText('活動標題')
    const opens = screen.getByLabelText('開始時間（選填）') as HTMLInputElement
    const closes = screen.getByLabelText('結束時間（選填）') as HTMLInputElement
    expect(opens.min).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/)
    expect(new Date(opens.min).getTime()).toBeGreaterThan(Date.now() - 2 * 60000)
    expect(new Date(opens.min).getTime()).toBeLessThanOrEqual(Date.now())
    await userEvent.type(screen.getByLabelText('活動標題'), 'past')
    await userEvent.selectOptions(screen.getByLabelText('題目'), 't1')
    fireEvent.change(opens, { target: { value: '2020-01-01T10:00' } })
    fireEvent.change(closes, { target: { value: '2030-01-01T10:00' } })
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('開始與結束時間不可早於現在。')).toBeInTheDocument()
    expect(posted).toBe(false)
    fireEvent.change(opens, { target: { value: '' } })
    fireEvent.change(closes, { target: { value: '2020-01-01T10:00' } })
    await userEvent.click(screen.getByRole('button', { name: '建立活動' }))
    expect(await screen.findByText('開始與結束時間不可早於現在。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })

  it('keeps unchanged past times saveable when editing', async () => {
    mockList([act({ opens_at: '2020-01-01T00:00:00Z', closes_at: '2020-02-01T00:00:00Z' })])
    let body: unknown
    server.use(http.patch('/api/activities/a1', async ({ request }) => {
      body = await request.json()
      return HttpResponse.json(act())
    }))
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(body).toBeDefined())
  })

  it('loads browser-local times and clears a schedule with null', async () => {
    const opens = '2030-01-01T00:00:00Z'
    mockList([act({ opens_at: opens, closes_at: null })])
    let body: unknown
    server.use(http.patch('/api/activities/a1', async ({ request }) => {
      body = await request.json()
      return HttpResponse.json(act())
    }))
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog')
    const input = within(dialog).getByLabelText('開始時間（選填）') as HTMLInputElement
    expect(new Date(input.value).toISOString()).toBe(opens.replace('Z', '.000Z'))
    fireEvent.change(input, { target: { value: '' } })
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(body).toMatchObject({ opens_at: null }))
  })

  it('edits schedules at minute precision and drops stored seconds', async () => {
    mockList([act({ opens_at: '2030-01-01T00:00:45Z', closes_at: null })])
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const input = within(await screen.findByRole('dialog')).getByLabelText('開始時間（選填）') as HTMLInputElement
    expect(input).toHaveAttribute('step', '60')
    expect(input.value).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/)
    expect(new Date(input.value).toISOString()).toBe('2030-01-01T00:00:00.000Z')
  })

  it('shows scheduled times, closed availability, and translated schedule errors', async () => {
    mockList([act({ status: 'published', opens_at: '2100-01-01T00:00:00Z', closes_at: null })])
    server.use(http.patch('/api/activities/a1', () => apiError(400, 'invalid_schedule')))
    setup()
    expect(await screen.findByText('未開放')).toBeInTheDocument()
    expect(screen.getByText(/2100/)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('結束時間必須晚於開始時間。')).toBeInTheDocument()
  })
})
