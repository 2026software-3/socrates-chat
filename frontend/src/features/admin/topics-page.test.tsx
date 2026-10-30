import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { TopicsPage } from '@/features/admin/topics-page'
import type { Topic } from '@/lib/types'
import { apiError, renderApp } from '@/test/render'
import { server } from '@/test/server'

const topic = (id: string, over: Partial<Topic> = {}): Topic => ({
  id,
  title: `Topic ${id}`,
  description: `Description ${id}`,
  category: 'Ethics',
  is_active: true,
  ...over,
})

/** 有狀態的假後端：記錄 POST／PATCH 的 body。 */
function mockTopics(initial: Topic[]) {
  const topics = [...initial]
  const calls: { method: string; url: string; body: unknown }[] = []
  server.use(
    http.get('/api/topics', () => HttpResponse.json(topics)),
    http.post('/api/topics', async ({ request }) => {
      const body = (await request.json()) as Partial<Topic>
      calls.push({ method: 'POST', url: '/api/topics', body })
      const created = topic(`n${topics.length + 1}`, { title: body.title, description: body.description ?? '', category: body.category ?? null })
      topics.push(created)
      return HttpResponse.json(created, { status: 201 })
    }),
    http.patch('/api/topics/:id', async ({ request, params }) => {
      const body = (await request.json()) as Partial<Topic>
      calls.push({ method: 'PATCH', url: `/api/topics/${String(params.id)}`, body })
      const i = topics.findIndex((x) => x.id === params.id)
      topics[i] = { ...topics[i], ...body }
      return HttpResponse.json(topics[i])
    }),
  )
  return { calls }
}

const render = () => renderApp(<TopicsPage />)

/** 開啟「新增題目」對話框並回傳它。 */
async function openCreate() {
  await userEvent.click(screen.getByRole('button', { name: '+ 新增題目' }))
  return screen.findByRole('dialog', { name: '新增題目' })
}

describe('TopicsPage', () => {
  it('lists all topics including inactive ones, marking inactive ones', async () => {
    mockTopics([topic('1'), topic('2', { is_active: false })])
    render()
    const table = await screen.findByRole('table', { name: '題目清單' })
    const rows = within(table).getAllByRole('row').slice(1)
    expect(rows).toHaveLength(2)
    expect(within(rows[0]).getByText('啟用')).toBeInTheDocument()
    expect(within(rows[0]).getByRole('button', { name: '停用「Topic 1」' })).toBeInTheDocument()
    expect(within(rows[1]).getByText('已停用')).toBeInTheDocument()
    expect(within(rows[1]).getByRole('button', { name: '啟用「Topic 2」' })).toBeInTheDocument()
    expect(screen.getByText('共 2 個題目')).toBeInTheDocument()
    expect(screen.getByText(/已經開始的對話不受影響/)).toBeInTheDocument()
  })

  it('shows loading and empty states', async () => {
    mockTopics([])
    render()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('題目庫還沒有題目。請按右上角新增。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    server.use(http.get('/api/topics', () => apiError(500, 'internal')))
    render()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    mockTopics([topic('1')])
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('Topic 1')).toBeInTheDocument()
  })

  it('creates a topic with exactly the entered fields', async () => {
    const { calls } = mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), ' New topic ')
    await userEvent.type(within(dialog).getByLabelText('題目內容'), 'About it')
    await userEvent.type(within(dialog).getByLabelText('分類'), 'Logic')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('New topic')).toBeInTheDocument()
    expect(calls).toEqual([
      { method: 'POST', url: '/api/topics', body: { title: 'New topic', description: 'About it', category: 'Logic' } },
    ])
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.getByText('已新增題目「New topic」。')).toBeInTheDocument()
  })

  it('omits empty optional fields and requires a title', async () => {
    const { calls } = mockTopics([])
    render()
    await screen.findByText('題目庫還沒有題目。請按右上角新增。')
    const dialog = await openCreate()
    expect(within(dialog).getByRole('button', { name: '新增題目' })).toBeDisabled()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'Only title')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    await screen.findByText('Only title')
    expect(calls[0].body).toEqual({ title: 'Only title' })
  })

  it('shows invalid_title inline and keeps the input', async () => {
    mockTopics([])
    server.use(http.post('/api/topics', () => apiError(400, 'invalid_title')))
    render()
    await screen.findByText('題目庫還沒有題目。請按右上角新增。')
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'x')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('標題不可為空。')
    expect(within(dialog).getByLabelText('題目名稱')).toHaveValue('x')
  })

  it('blocks double submit while creating', async () => {
    mockTopics([])
    let posts = 0
    let release!: () => void
    const gate = new Promise<void>((r) => (release = r))
    server.use(
      http.post('/api/topics', async () => {
        posts += 1
        await gate
        return HttpResponse.json(topic('n1'), { status: 201 })
      }),
    )
    render()
    await screen.findByText('題目庫還沒有題目。請按右上角新增。')
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'x{Enter}')
    expect(await screen.findByRole('button', { name: '新增中…' })).toBeDisabled()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), '{Enter}')
    release()
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(posts).toBe(1)
  })

  it('edits a topic in a dialog and sends only the changed fields', async () => {
    const { calls } = mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    const title = within(dialog).getByLabelText('題目名稱')
    expect(title).toHaveValue('Topic 1')
    await userEvent.clear(title)
    await userEvent.type(title, 'Renamed')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([{ method: 'PATCH', url: '/api/topics/1', body: { title: 'Renamed' } }])
    expect(screen.getByText('Renamed')).toBeInTheDocument()
  })

  it('shows invalid_title in the edit dialog and keeps it open', async () => {
    mockTopics([topic('1')])
    server.use(http.patch('/api/topics/:id', () => apiError(400, 'invalid_title')))
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), '!')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('標題不可為空。')
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('toggles a topic inactive with a PATCH of is_active only', async () => {
    const { calls } = mockTopics([topic('1')])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '停用「Topic 1」' }))
    const confirm = await screen.findByRole('alertdialog')
    expect(confirm).toHaveTextContent('確認停用題目？')
    expect(confirm).toHaveTextContent('不會影響已使用此題目的活動')
    expect(calls).toEqual([])
    await userEvent.click(within(confirm).getByRole('button', { name: '停用題目' }))
    await screen.findByRole('button', { name: '啟用「Topic 1」' })
    expect(calls).toEqual([{ method: 'PATCH', url: '/api/topics/1', body: { is_active: false } }])
    expect(screen.getByText('已停用')).toBeInTheDocument()
  })

  it('does nothing when the deactivate confirmation is cancelled, and activates after confirming', async () => {
    const { calls } = mockTopics([topic('1', { is_active: false })])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '啟用「Topic 1」' }))
    let confirm = await screen.findByRole('alertdialog')
    expect(confirm).toHaveTextContent('確認啟用題目？')
    await userEvent.click(within(confirm).getByRole('button', { name: '取消' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(calls).toEqual([])
    await userEvent.click(screen.getByRole('button', { name: '啟用「Topic 1」' }))
    confirm = await screen.findByRole('alertdialog')
    await userEvent.click(within(confirm).getByRole('button', { name: '啟用題目' }))
    await screen.findByRole('button', { name: '停用「Topic 1」' })
    expect(calls).toEqual([{ method: 'PATCH', url: '/api/topics/1', body: { is_active: true } }])
  })

  it('reverts the status and shows the translated error when the toggle fails', async () => {
    mockTopics([topic('1')])
    server.use(http.patch('/api/topics/:id', () => apiError(500, 'internal')))
    render()
    await userEvent.click(await screen.findByRole('button', { name: '停用「Topic 1」' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '停用題目' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(screen.getByRole('button', { name: '停用「Topic 1」' })).toBeInTheDocument()
    expect(screen.queryByText('已停用')).not.toBeInTheDocument()
  })

  it('filters by text and category', async () => {
    mockTopics([
      topic('1', { title: 'Justice', category: 'Ethics' }),
      topic('2', { title: 'Truth', category: 'Logic' }),
      topic('3', { title: 'Old one', category: 'Logic', is_active: false }),
    ])
    render()
    await screen.findByText('Justice')
    expect(screen.getByText('Old one')).toBeInTheDocument()

    await userEvent.selectOptions(screen.getByLabelText('依分類篩選'), 'Logic')
    expect(screen.queryByText('Justice')).not.toBeInTheDocument()
    expect(screen.getByText('Truth')).toBeInTheDocument()
    await userEvent.selectOptions(screen.getByLabelText('依分類篩選'), '')

    await userEvent.type(screen.getByLabelText('搜尋題目'), 'justice')
    expect(screen.queryByText('Truth')).not.toBeInTheDocument()
    expect(screen.getByText('Justice')).toBeInTheDocument()

    await userEvent.clear(screen.getByLabelText('搜尋題目'))
    await userEvent.type(screen.getByLabelText('搜尋題目'), 'zzz')
    expect(screen.getByText('沒有符合條件的題目。')).toBeInTheDocument()
  })

  it('does not send a PATCH when the edit changes nothing', async () => {
    const { calls } = mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([])
  })

  it('sends category null when the category is emptied, and drops the shown category', async () => {
    const { calls } = mockTopics([topic('1', { category: 'Ethics' })])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.clear(within(dialog).getByLabelText('分類'))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([{ method: 'PATCH', url: '/api/topics/1', body: { category: null } }])
    expect(within(screen.getByRole('table', { name: '題目清單' })).queryByText('Ethics')).not.toBeInTheDocument()
  })

  it('does not send the category when it is left empty on a topic without one', async () => {
    const { calls } = mockTopics([topic('1', { category: null })])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([])
  })

  it('trims the description when creating and editing', async () => {
    const { calls } = mockTopics([topic('1', { description: 'Desc' })])
    render()
    await screen.findByText('Topic 1')
    const createDialog = await openCreate()
    await userEvent.type(within(createDialog).getByLabelText('題目名稱'), 'T')
    await userEvent.type(within(createDialog).getByLabelText('題目內容'), '  hi  ')
    await userEvent.click(within(createDialog).getByRole('button', { name: '新增題目' }))
    await screen.findByText('已新增題目「T」。')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('題目內容'), '   ')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([{ method: 'POST', url: '/api/topics', body: { title: 'T', description: 'hi' } }])
  })

  it('clears the create notice on the next submit', async () => {
    mockTopics([])
    render()
    await screen.findByText('題目庫還沒有題目。請按右上角新增。')
    let dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'A')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('已新增題目「A」。')).toBeInTheDocument()
    server.use(http.post('/api/topics', () => apiError(500, 'internal')))
    dialog = await openCreate()
    expect(screen.queryByText('已新增題目「A」。')).not.toBeInTheDocument()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'B')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    await within(dialog).findByText('系統發生錯誤，請稍後再試。')
    expect(screen.queryByText('已新增題目「A」。')).not.toBeInTheDocument()
  })

  it('shows a 500 on create inline and keeps the input', async () => {
    mockTopics([])
    server.use(http.post('/api/topics', () => apiError(500, 'internal')))
    render()
    await screen.findByText('題目庫還沒有題目。請按右上角新增。')
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'x')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(within(dialog).getByLabelText('題目名稱')).toHaveValue('x')
  })

  it('lets fresh server data win over earlier local edits after a reload', async () => {
    mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '停用「Topic 1」' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '停用題目' }))
    await screen.findByText('已停用')
    // 別的管理者把它改回啟用、改了標題，然後這裡新增題目觸發重新載入
    server.use(http.get('/api/topics', () => HttpResponse.json([topic('1', { title: 'Server title' }), topic('2')])))
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'N')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('Server title')).toBeInTheDocument()
    expect(screen.queryByText('已停用')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '停用「Server title」' })).toBeInTheDocument()
  })

  it('falls back to all categories when the selected category disappears', async () => {
    mockTopics([topic('1', { category: 'Ethics' }), topic('2', { category: 'Logic' })])
    render()
    await screen.findByText('Topic 1')
    await userEvent.selectOptions(screen.getByLabelText('依分類篩選'), 'Logic')
    expect(screen.queryByText('Topic 1')).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 2」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.clear(within(dialog).getByLabelText('分類'))
    await userEvent.type(within(dialog).getByLabelText('分類'), 'Ethics')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(screen.getByLabelText('依分類篩選')).toHaveValue('')
    expect(screen.getByText('Topic 1')).toBeInTheDocument()
    expect(screen.getByText('Topic 2')).toBeInTheDocument()
  })

  it('shows a retryable error next to the stale list when a reload fails', async () => {
    mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    server.use(http.get('/api/topics', () => apiError(500, 'internal')))
    const dialog = await openCreate()
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), 'N')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByText('Topic 1')).toBeInTheDocument()
  })

  it('localizes the edit dialog: cancel button, no English close label', async () => {
    mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).queryByText('Close')).not.toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('renders in Spanish', async () => {
    mockTopics([topic('1')])
    renderApp(<TopicsPage />, { lang: 'es' })
    expect(await screen.findByText('Temas: 1')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '+ Nuevo tema' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: /Editar/ }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByRole('button', { name: 'Cancelar' })).toBeInTheDocument()
  })
})
