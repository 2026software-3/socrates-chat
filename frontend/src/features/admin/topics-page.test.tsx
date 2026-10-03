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
    http.get('/api/admin/topics', () => HttpResponse.json(topics)),
    http.post('/api/admin/topics', async ({ request }) => {
      const body = (await request.json()) as Partial<Topic>
      calls.push({ method: 'POST', url: '/api/admin/topics', body })
      const created = topic(`n${topics.length + 1}`, { title: body.title, description: body.description ?? '', category: body.category ?? null })
      topics.push(created)
      return HttpResponse.json(created, { status: 201 })
    }),
    http.patch('/api/admin/topics/:id', async ({ request, params }) => {
      const body = (await request.json()) as Partial<Topic>
      calls.push({ method: 'PATCH', url: `/api/admin/topics/${String(params.id)}`, body })
      const i = topics.findIndex((x) => x.id === params.id)
      topics[i] = { ...topics[i], ...body }
      return HttpResponse.json(topics[i])
    }),
  )
  return { calls }
}

const render = () => renderApp(<TopicsPage />)

describe('TopicsPage', () => {
  it('lists all topics including inactive ones, marking inactive ones', async () => {
    mockTopics([topic('1'), topic('2', { is_active: false })])
    render()
    const list = await screen.findByRole('list', { name: '題目清單' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(2)
    expect(within(items[0]).queryByText('已停用')).not.toBeInTheDocument()
    expect(within(items[1]).getByText('已停用')).toBeInTheDocument()
    expect(screen.getByText('共 2 個題目')).toBeInTheDocument()
    expect(screen.getByText(/已經開始的對話不受影響/)).toBeInTheDocument()
    expect(screen.getByRole('switch', { name: '啟用「Topic 1」' })).toBeChecked()
    expect(screen.getByRole('switch', { name: '啟用「Topic 2」' })).not.toBeChecked()
  })

  it('shows loading and empty states', async () => {
    mockTopics([])
    render()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('題目庫還沒有題目。請在上方新增。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    server.use(http.get('/api/admin/topics', () => apiError(500, 'internal')))
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
    await userEvent.type(screen.getByLabelText('標題'), ' New topic ')
    await userEvent.type(screen.getByLabelText('說明'), 'About it')
    await userEvent.type(screen.getByLabelText('分類'), 'Logic')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('New topic')).toBeInTheDocument()
    expect(calls).toEqual([
      { method: 'POST', url: '/api/admin/topics', body: { title: 'New topic', description: 'About it', category: 'Logic' } },
    ])
    expect(screen.getByLabelText('標題')).toHaveValue('')
    expect(screen.getByText('已新增題目「New topic」。')).toBeInTheDocument()
  })

  it('omits empty optional fields and requires a title', async () => {
    const { calls } = mockTopics([])
    render()
    await screen.findByText('題目庫還沒有題目。請在上方新增。')
    expect(screen.getByRole('button', { name: '新增題目' })).toBeDisabled()
    await userEvent.type(screen.getByLabelText('標題'), 'Only title')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    await screen.findByText('Only title')
    expect(calls[0].body).toEqual({ title: 'Only title' })
  })

  it('shows invalid_title inline and keeps the input', async () => {
    mockTopics([])
    server.use(http.post('/api/admin/topics', () => apiError(400, 'invalid_title')))
    render()
    await screen.findByText('題目庫還沒有題目。請在上方新增。')
    await userEvent.type(screen.getByLabelText('標題'), 'x')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('標題不可為空。')
    expect(screen.getByLabelText('標題')).toHaveValue('x')
  })

  it('blocks double submit while creating', async () => {
    mockTopics([])
    let posts = 0
    let release!: () => void
    const gate = new Promise<void>((r) => (release = r))
    server.use(
      http.post('/api/admin/topics', async () => {
        posts += 1
        await gate
        return HttpResponse.json(topic('n1'), { status: 201 })
      }),
    )
    render()
    await screen.findByText('題目庫還沒有題目。請在上方新增。')
    await userEvent.type(screen.getByLabelText('標題'), 'x{Enter}')
    expect(await screen.findByRole('button', { name: '新增中…' })).toBeDisabled()
    await userEvent.type(screen.getByLabelText('標題'), '{Enter}')
    release()
    await waitFor(() => expect(screen.getByRole('button', { name: '新增題目' })).toBeInTheDocument())
    expect(posts).toBe(1)
  })

  it('edits a topic in a dialog and sends only the changed fields', async () => {
    const { calls } = mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    const title = within(dialog).getByLabelText('標題')
    expect(title).toHaveValue('Topic 1')
    await userEvent.clear(title)
    await userEvent.type(title, 'Renamed')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([{ method: 'PATCH', url: '/api/admin/topics/1', body: { title: 'Renamed' } }])
    expect(screen.getByText('Renamed')).toBeInTheDocument()
  })

  it('shows invalid_title in the edit dialog and keeps it open', async () => {
    mockTopics([topic('1')])
    server.use(http.patch('/api/admin/topics/:id', () => apiError(400, 'invalid_title')))
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('標題'), '!')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('標題不可為空。')
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('toggles a topic inactive with a PATCH of is_active only', async () => {
    const { calls } = mockTopics([topic('1')])
    render()
    const sw = await screen.findByRole('switch', { name: '啟用「Topic 1」' })
    await userEvent.click(sw)
    await waitFor(() => expect(screen.getByRole('switch', { name: '啟用「Topic 1」' })).not.toBeChecked())
    expect(calls).toEqual([{ method: 'PATCH', url: '/api/admin/topics/1', body: { is_active: false } }])
    expect(screen.getByText('已停用')).toBeInTheDocument()
  })

  it('reverts the switch and shows the translated error when the toggle fails', async () => {
    mockTopics([topic('1')])
    server.use(http.patch('/api/admin/topics/:id', () => apiError(500, 'internal')))
    render()
    const sw = await screen.findByRole('switch', { name: '啟用「Topic 1」' })
    await userEvent.click(sw)
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(screen.getByRole('switch', { name: '啟用「Topic 1」' })).toBeChecked()
    expect(screen.queryByText('已停用')).not.toBeInTheDocument()
  })

  it('filters by text, category and the include-inactive toggle', async () => {
    mockTopics([
      topic('1', { title: 'Justice', category: 'Ethics' }),
      topic('2', { title: 'Truth', category: 'Logic' }),
      topic('3', { title: 'Old one', category: 'Logic', is_active: false }),
    ])
    render()
    await screen.findByText('Justice')
    const includeInactive = screen.getByRole('switch', { name: '顯示已停用題目' })
    expect(includeInactive).toBeChecked()

    await userEvent.click(includeInactive)
    expect(screen.queryByText('Old one')).not.toBeInTheDocument()
    expect(screen.getByText('顯示 2 / 3 個題目')).toBeInTheDocument()
    await userEvent.click(includeInactive)
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

  it('does not send an empty category (the backend cannot clear it) and keeps the shown category', async () => {
    const { calls } = mockTopics([topic('1', { category: 'Ethics' })])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.clear(within(dialog).getByLabelText('分類'))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([])
    expect(within(screen.getByRole('list', { name: '題目清單' })).getByText('Ethics')).toBeInTheDocument()
  })

  it('trims the description when creating and editing', async () => {
    const { calls } = mockTopics([topic('1', { description: 'Desc' })])
    render()
    await screen.findByText('Topic 1')
    await userEvent.type(screen.getByLabelText('標題'), 'T')
    await userEvent.type(screen.getByLabelText('說明'), '  hi  ')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    await screen.findByText('已新增題目「T」。')
    await userEvent.click(screen.getByRole('button', { name: '編輯「Topic 1」' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('說明'), '   ')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(calls).toEqual([{ method: 'POST', url: '/api/admin/topics', body: { title: 'T', description: 'hi' } }])
  })

  it('clears the create notice on the next submit', async () => {
    mockTopics([])
    render()
    await screen.findByText('題目庫還沒有題目。請在上方新增。')
    await userEvent.type(screen.getByLabelText('標題'), 'A')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('已新增題目「A」。')).toBeInTheDocument()
    server.use(http.post('/api/admin/topics', () => apiError(500, 'internal')))
    await userEvent.type(screen.getByLabelText('標題'), 'B')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    await screen.findByText('系統發生錯誤，請稍後再試。')
    expect(screen.queryByText('已新增題目「A」。')).not.toBeInTheDocument()
  })

  it('shows a 500 on create inline and keeps the input', async () => {
    mockTopics([])
    server.use(http.post('/api/admin/topics', () => apiError(500, 'internal')))
    render()
    await screen.findByText('題目庫還沒有題目。請在上方新增。')
    await userEvent.type(screen.getByLabelText('標題'), 'x')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    expect(screen.getByLabelText('標題')).toHaveValue('x')
  })

  it('lets fresh server data win over earlier local edits after a reload', async () => {
    mockTopics([topic('1')])
    render()
    await screen.findByText('Topic 1')
    await userEvent.click(screen.getByRole('switch', { name: '啟用「Topic 1」' }))
    await screen.findByText('已停用')
    // 別的管理者把它改回啟用、改了標題，然後這裡新增題目觸發重新載入
    server.use(http.get('/api/admin/topics', () => HttpResponse.json([topic('1', { title: 'Server title' }), topic('2')])))
    await userEvent.type(screen.getByLabelText('標題'), 'N')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('Server title')).toBeInTheDocument()
    expect(screen.queryByText('已停用')).not.toBeInTheDocument()
    expect(screen.getByRole('switch', { name: '啟用「Server title」' })).toBeChecked()
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
    server.use(http.get('/api/admin/topics', () => apiError(500, 'internal')))
    await userEvent.type(screen.getByLabelText('標題'), 'N')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
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
    expect(screen.getByRole('button', { name: 'Añadir tema' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: /Editar/ }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByRole('button', { name: 'Cancelar' })).toBeInTheDocument()
  })
})
