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

const topics3: Topic[] = [
  ...topics,
  { id: 't3', title: '正義是什麼', description: '', category: null, is_active: true },
]

function act(over: Partial<Activity> = {}): Activity {
  return {
    id: 'a1',
    classroom_id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
    title: '第一週討論',
    description: '請討論公平',
    topic_ids: ['t1'],
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

/** 列上的操作都要在確認對話框裡再按一次。 */
async function confirmAs(name: string) {
  const dialog = await screen.findByRole('alertdialog')
  await userEvent.click(within(dialog).getByRole('button', { name }))
}

/** 在表單的「新增題目」下拉選一個題目並按「加入題目」；scope 預設為整頁，編輯時傳對話框。 */
async function addTopic(id: string, scope?: HTMLElement) {
  const q = scope ? within(scope) : screen
  const select = await q.findByLabelText('新增題目')
  await waitFor(() => expect(select.querySelector(`option[value="${id}"]`)).not.toBeNull())
  await userEvent.selectOptions(select, id)
  await userEvent.click(q.getByRole('button', { name: '加入題目' }))
}

/** 開啟「建立討論活動」對話框。 */
async function openCreate() {
  await userEvent.click(screen.getByRole('button', { name: '+ 建立活動' }))
  await screen.findByRole('dialog', { name: '建立討論活動' })
}

describe('ActivitiesPanel', () => {
  it('lists activities with status badge and topic title', async () => {
    mockList([act(), act({ id: 'a2', title: '第二週', status: 'published', topic_ids: ['t2'] })])
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
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('請至少加入一個題目。')).toBeInTheDocument()
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
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), '   ')
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
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
        list = [act({ title: '新活動', topic_ids: ['t2'] })]
        return HttpResponse.json(list[0], { status: 201 })
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await addTopic('t2')
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('活動已建立（草稿）。')).toBeInTheDocument()
    expect(body).toEqual({ title: '新活動', topic_ids: ['t2'], classroom_id: ROOM_A })
    expect(await screen.findByText('草稿')).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
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
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), 'X')
    await addTopic('t1')
    const btn = screen.getByRole('button', { name: '儲存草稿' })
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
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), 'X')
    await addTopic('t1')
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('題目不存在、重複，或超過上限。')).toBeInTheDocument()
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
    await confirmAs('封存活動')
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
        list = [act({ title: '改名', topic_ids: ['t2'] })]
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
    await userEvent.click(within(dialog).getByRole('button', { name: '移除「電車難題」' }))
    await addTopic('t2', dialog)
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('活動已更新。')).toBeInTheDocument()
    expect(body).toEqual({
      title: '改名',
      description: '請討論公平',
      topic_ids: ['t2'],
    })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(await screen.findByText('改名')).toBeInTheDocument()
  })

  it('omits topic_ids when the topics are unchanged', async () => {
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

  it('shows the saved topics of an activity in order and keeps the picker for the rest', async () => {
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics3)),
      http.get('/api/activities', () => pageOf([act({ topic_ids: ['t2', 't1'] })])),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await waitFor(() => expect(within(dialog).getAllByRole('listitem')).toHaveLength(2))
    const [first, second] = within(dialog).getAllByRole('listitem')
    expect(first).toHaveTextContent('自由意志')
    expect(second).toHaveTextContent('電車難題')
    // 已選的題目不會再出現在「新增題目」下拉
    const picker = within(dialog).getByLabelText('新增題目')
    expect(within(picker).queryByRole('option', { name: '電車難題' })).not.toBeInTheDocument()
    expect(within(picker).getByRole('option', { name: '正義是什麼' })).toBeInTheDocument()
  })

  it('lists every topic of an activity in order in the table', async () => {
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics3)),
      http.get('/api/activities', () => pageOf([act({ topic_ids: ['t3', 't1', 'gone'] })])),
    )
    setup()
    const items = await within(await screen.findByRole('table')).findAllByRole('listitem')
    expect(items.map((c) => c.textContent)).toEqual(['正義是什麼', '電車難題', '（已停用或無法載入的題目）'])
  })

  it('creates an activity with several topics in the chosen order', async () => {
    let body: unknown
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics3)),
      http.get('/api/activities', () => pageOf([])),
      http.post('/api/activities', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json(act({ topic_ids: ['t3', 't2', 't1'] }), { status: 201 })
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), '多題活動')
    await addTopic('t1')
    await addTopic('t2')
    await addTopic('t3')
    // [t1, t2, t3] → 把 t3 上移兩次 → [t3, t1, t2]
    await userEvent.click(screen.getByRole('button', { name: '上移「正義是什麼」' }))
    await userEvent.click(screen.getByRole('button', { name: '上移「正義是什麼」' }))
    const order = () => within(screen.getByRole('dialog')).getAllByRole('listitem').map((li) => li.textContent)
    expect(order()).toEqual([
      expect.stringContaining('正義是什麼'),
      expect.stringContaining('電車難題'),
      expect.stringContaining('自由意志'),
    ])
    // 再把 t1 往下移 → [t3, t2, t1]
    await userEvent.click(screen.getByRole('button', { name: '下移「電車難題」' }))
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('活動已建立（草稿）。')).toBeInTheDocument()
    expect(body).toEqual({ title: '多題活動', topic_ids: ['t3', 't2', 't1'], classroom_id: ROOM_A })
  })

  it('removes a chosen topic and offers it again in the picker', async () => {
    mockList([])
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    await addTopic('t1')
    await userEvent.click(screen.getByRole('button', { name: '移除「電車難題」' }))
    expect(within(screen.getByRole('dialog')).queryByRole('listitem')).not.toBeInTheDocument()
    const picker = screen.getByLabelText('新增題目')
    expect(within(picker).getByRole('option', { name: '電車難題' })).toBeInTheDocument()
  })

  it('disables moving the first topic up and the last topic down', async () => {
    mockList([])
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    await addTopic('t1')
    await addTopic('t2')
    expect(screen.getByRole('button', { name: '上移「電車難題」' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '下移「電車難題」' })).toBeEnabled()
    expect(screen.getByRole('button', { name: '上移「自由意志」' })).toBeEnabled()
    expect(screen.getByRole('button', { name: '下移「自由意志」' })).toBeDisabled()
  })

  it('stops at five topics and explains the limit', async () => {
    const many: Topic[] = ['1', '2', '3', '4', '5', '6'].map((n) => ({
      id: `m${n}`, title: `題目${n}`, description: '', category: null, is_active: true,
    }))
    server.use(
      http.get('/api/topics', () => HttpResponse.json(many)),
      http.get('/api/activities', () => pageOf([])),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    for (const n of ['1', '2', '3', '4']) await addTopic(`m${n}`)
    expect(screen.queryByText('最多只能加入 5 個題目。')).not.toBeInTheDocument()
    await addTopic('m5')
    expect(await screen.findByText('最多只能加入 5 個題目。')).toBeInTheDocument()
    expect(screen.getByLabelText('新增題目')).toBeDisabled()
    expect(screen.getByRole('button', { name: '加入題目' })).toBeDisabled()
    // 移除一個後又可以加入
    await userEvent.click(screen.getByRole('button', { name: '移除「題目5」' }))
    expect(screen.getByLabelText('新增題目')).toBeEnabled()
  })

  it('shows the locked-topic message, keeps the saved topics fixed and still allows adding', async () => {
    const bodies: unknown[] = []
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics3)),
      http.get('/api/activities', () => pageOf([act({ topic_ids: ['t1', 't2'] })])),
      http.patch('/api/activities/a1', async ({ request }) => {
        bodies.push(await request.json())
        return bodies.length === 1 ? apiError(409, 'activity_topic_locked') : HttpResponse.json(act({ topic_ids: ['t1', 't2', 't3'] }))
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await waitFor(() => expect(within(dialog).getAllByRole('listitem')).toHaveLength(2))
    // 教師移除第一題後送出，後端回 409
    await userEvent.click(within(dialog).getByRole('button', { name: '移除「電車難題」' }))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findAllByText(/此活動已有學生開始對話，既有題目不能移除、更換或調整順序/)).not.toHaveLength(0)
    expect(screen.queryByText('activity_topic_locked')).not.toBeInTheDocument()
    // 清單還原為已儲存的題目，既有題目的移除與排序都停用
    const items = within(dialog).getAllByRole('listitem')
    expect(items.map((li) => li.textContent)).toEqual([expect.stringContaining('電車難題'), expect.stringContaining('自由意志')])
    for (const title of ['電車難題', '自由意志']) {
      expect(within(dialog).getByRole('button', { name: `移除「${title}」` })).toBeDisabled()
      expect(within(dialog).getByRole('button', { name: `上移「${title}」` })).toBeDisabled()
      expect(within(dialog).getByRole('button', { name: `下移「${title}」` })).toBeDisabled()
    }
    // 仍可新增，新增的題目可以自由移除
    await addTopic('t3', dialog)
    expect(within(dialog).getByRole('button', { name: '移除「正義是什麼」' })).toBeEnabled()
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('活動已更新。')).toBeInTheDocument()
    expect(bodies[1]).toEqual({ title: '第一週討論', description: '請討論公平', topic_ids: ['t1', 't2', 't3'] })
  })

  it('does not let an added topic move above a locked one', async () => {
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics3)),
      http.get('/api/activities', () => pageOf([act({ topic_ids: ['t1'] })])),
      http.patch('/api/activities/a1', () => apiError(409, 'activity_topic_locked')),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await waitFor(() => expect(within(dialog).getAllByRole('listitem')).toHaveLength(1))
    // 先送出一次（後端回 409）讓清單鎖定，已儲存的題目固定在最前面
    await addTopic('t2', dialog)
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '移除「電車難題」' })).toBeDisabled())
    expect(within(dialog).getAllByRole('listitem')).toHaveLength(1)
    await addTopic('t2', dialog)
    await addTopic('t3', dialog)
    expect(within(dialog).getByRole('button', { name: '上移「自由意志」' })).toBeDisabled()
    expect(within(dialog).getByRole('button', { name: '上移「正義是什麼」' })).toBeEnabled()
    expect(within(dialog).getByRole('button', { name: '下移「自由意志」' })).toBeEnabled()
  })

  it('blocks saving an edit that removes every topic', async () => {
    let patched = false
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics3)),
      http.get('/api/activities', () => pageOf([act()])),
      http.patch('/api/activities/a1', () => {
        patched = true
        return HttpResponse.json(act())
      }),
    )
    setup()
    await screen.findByText('第一週討論')
    await userEvent.click(screen.getByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await userEvent.click(await within(dialog).findByRole('button', { name: '移除「電車難題」' }))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByText('請至少加入一個題目。')).toBeInTheDocument()
    expect(patched).toBe(false)
  })

  it('keeps an inactive topic in the list, labelled as unavailable, and lets it be removed', async () => {
    mockList([act({ topic_ids: ['gone', 't1'] })])
    setup()
    await userEvent.click(await screen.findByRole('button', { name: '編輯' }))
    const dialog = await screen.findByRole('dialog', { name: '編輯活動' })
    await waitFor(() => expect(within(dialog).getAllByRole('listitem')).toHaveLength(2))
    expect(within(dialog).getAllByRole('listitem')[0]).toHaveTextContent('（已停用或無法載入的題目）')
    expect(within(dialog).getByRole('button', { name: '移除「（已停用或無法載入的題目）」' })).toBeEnabled()
  })

  it('only offers active topics even though /api/topics lists every topic', async () => {
    server.use(
      http.get('/api/activities', () => pageOf([act({ topic_ids: ['t3'] })])),
      http.get('/api/topics', () =>
        HttpResponse.json([...topics, { id: 't3', title: '已停用題目', description: '', category: null, is_active: false }]),
      ),
    )
    setup()
    expect(await within(await screen.findByRole('table')).findByText('（已停用或無法載入的題目）')).toBeInTheDocument()
    await openCreate()
    const select = await screen.findByLabelText('新增題目')
    await waitFor(() => expect(within(select).getByRole('option', { name: '電車難題' })).toBeInTheDocument())
    expect(within(select).queryByRole('option', { name: '已停用題目' })).not.toBeInTheDocument()
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
    expect(await within(dialog).findByText('題目不存在、重複，或超過上限。')).toBeInTheDocument()
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
    await confirmAs('確認發布')
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
    await confirmAs('確認發布')
    expect(await screen.findByText('活動已發布。')).toBeInTheDocument()
    await userEvent.click(await screen.findByRole('button', { name: '停止開放' }))
    await confirmAs('確認停止開放')
    expect(await screen.findByText('活動已停止開放。')).toBeInTheDocument()
    await userEvent.click(await screen.findByRole('button', { name: '重新開放' }))
    await confirmAs('確認重新開放')
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
    await userEvent.click(btn)
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.dblClick(within(dialog).getByRole('button', { name: '確認發布' }))
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
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await addTopic('t1')
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
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
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    expect(await screen.findByText('無法載入題目清單，請重新整理頁面後再試。')).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('活動標題'), 'X')
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('請至少加入一個題目。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })
})

describe('activity schedules', () => {
  it('rejects an end at or before the start without posting', async () => {
    mockList([])
    let posted = false
    server.use(http.post('/api/activities', () => { posted = true; return HttpResponse.json(act()) }))
    setup()
    await openCreate()
    await screen.findByLabelText('新增題目')
    await userEvent.type(screen.getByLabelText('活動標題'), 'scheduled')
    await addTopic('t1')
    fireEvent.change(screen.getByLabelText('開始時間（選填）'), { target: { value: '2030-01-01T10:00' } })
    fireEvent.change(screen.getByLabelText('結束時間（選填）'), { target: { value: '2030-01-01T10:00' } })
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('結束時間必須晚於開始時間。')).toBeInTheDocument()
    expect(posted).toBe(false)
  })

  it('rejects newly picked schedule times in the past and sets min to now', async () => {
    mockList([])
    let posted = false
    server.use(http.post('/api/classrooms/:id/activities', () => { posted = true; return HttpResponse.json(act()) }))
    setup()
    await openCreate()
    await screen.findByLabelText('活動標題')
    const opens = screen.getByLabelText('開始時間（選填）') as HTMLInputElement
    const closes = screen.getByLabelText('結束時間（選填）') as HTMLInputElement
    expect(opens.min).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/)
    expect(new Date(opens.min).getTime()).toBeGreaterThan(Date.now() - 2 * 60000)
    expect(new Date(opens.min).getTime()).toBeLessThanOrEqual(Date.now())
    await userEvent.type(screen.getByLabelText('活動標題'), 'past')
    await addTopic('t1')
    fireEvent.change(opens, { target: { value: '2020-01-01T10:00' } })
    fireEvent.change(closes, { target: { value: '2030-01-01T10:00' } })
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
    expect(await screen.findByText('開始與結束時間不可早於現在。')).toBeInTheDocument()
    expect(posted).toBe(false)
    fireEvent.change(opens, { target: { value: '' } })
    fireEvent.change(closes, { target: { value: '2020-01-01T10:00' } })
    await userEvent.click(screen.getByRole('button', { name: '儲存草稿' }))
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

describe('publishing while creating', () => {
  it('creates the activity and then publishes it with 發布活動', async () => {
    const calls: string[] = []
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.get('/api/activities', () => pageOf([])),
      http.post('/api/activities', () => {
        calls.push('create')
        return HttpResponse.json(act({ id: 'new1', title: '新活動' }), { status: 201 })
      }),
      http.post('/api/activities/:id/publish', ({ params }) => {
        calls.push(`publish:${String(params.id)}`)
        return HttpResponse.json(act({ id: 'new1', status: 'published' }))
      }),
    )
    setup()
    await screen.findByText('還沒有任何討論活動，請先建立一個。')
    await openCreate()
    await userEvent.type(screen.getByLabelText('活動標題'), '新活動')
    await addTopic('t1')
    await userEvent.click(screen.getByRole('button', { name: '發布活動' }))
    expect(await screen.findByText('活動已發布。')).toBeInTheDocument()
    expect(calls).toEqual(['create', 'publish:new1'])
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })
})
