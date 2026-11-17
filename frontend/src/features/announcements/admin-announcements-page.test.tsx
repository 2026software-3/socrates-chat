import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { AdminAnnouncementsPage } from '@/features/announcements/admin-announcements-page'
import { apiError, renderApp } from '@/test/render'
import { server } from '@/test/server'

type Row = {
  id: string
  kind: string
  text_zh_tw: string
  text_en: string | null
  text_es: string | null
  pending_languages: string[]
  starts_at: string
  ends_at: string | null
  enabled: boolean
}

const row = (over: Partial<Row> = {}): Row => ({
  id: 'a1',
  kind: 'general',
  text_zh_tw: '週五停機更新',
  text_en: 'Maintenance on Friday',
  text_es: 'Mantenimiento el viernes',
  pending_languages: [],
  starts_at: '2026-11-01T00:00:00Z',
  ends_at: null,
  enabled: true,
  ...over,
})

/** 有狀態的假後端：記錄寫入請求。 */
function mockBackend(initial: Row[], maintenance = false) {
  const rows = [...initial]
  const state = { maintenance }
  const calls: { method: string; url: string; body?: unknown }[] = []
  server.use(
    http.get('/api/admin/announcements', () => HttpResponse.json(rows)),
    http.post('/api/admin/announcements', async ({ request }) => {
      const body = (await request.json()) as Partial<Row>
      calls.push({ method: 'POST', url: '/api/admin/announcements', body })
      const created = row({ id: 'new', text_zh_tw: body.text_zh_tw ?? '', kind: body.kind ?? 'general' })
      rows.unshift(created)
      return HttpResponse.json(created, { status: 201 })
    }),
    http.patch('/api/admin/announcements/:id', async ({ request, params }) => {
      const body = (await request.json()) as Partial<Row>
      calls.push({ method: 'PATCH', url: `/api/admin/announcements/${String(params.id)}`, body })
      const r = rows.find((x) => x.id === params.id)
      if (!r) return apiError(404, 'not_found')
      Object.assign(r, body)
      return HttpResponse.json(r)
    }),
    http.delete('/api/admin/announcements/:id', ({ params }) => {
      calls.push({ method: 'DELETE', url: `/api/admin/announcements/${String(params.id)}` })
      const i = rows.findIndex((x) => x.id === params.id)
      if (i >= 0) rows.splice(i, 1)
      return new HttpResponse(null, { status: 204 })
    }),
    http.get('/api/admin/maintenance', () => HttpResponse.json({ enabled: state.maintenance })),
    http.put('/api/admin/maintenance', async ({ request }) => {
      const body = (await request.json()) as { enabled: boolean }
      calls.push({ method: 'PUT', url: '/api/admin/maintenance', body })
      state.maintenance = body.enabled
      return HttpResponse.json({ enabled: state.maintenance })
    }),
  )
  return { rows, calls }
}

const render = () => renderApp(<AdminAnnouncementsPage />)

describe('AdminAnnouncementsPage', () => {
  it('lists announcements with their state and the pending translations', async () => {
    mockBackend([
      row(),
      row({ id: 'a2', text_zh_tw: '已停用的公告', enabled: false }),
      row({ id: 'a3', text_zh_tw: '翻譯中的公告', text_en: null, pending_languages: ['en'] }),
    ])
    render()
    const list = await screen.findByRole('list', { name: '公告清單' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(3)
    expect(items[1]).toHaveTextContent('已停用')
    expect(items[2]).toHaveTextContent('英文翻譯尚未完成')
  })

  it('creates an announcement with the chosen kind', async () => {
    const { calls } = mockBackend([])
    render()
    await userEvent.type(await screen.findByLabelText('公告內容（繁體中文）'), '明晚停機更新')
    await userEvent.selectOptions(screen.getByLabelText('類型'), 'downtime')
    await userEvent.click(screen.getByRole('button', { name: '發佈公告' }))
    await waitFor(() => expect(calls).toHaveLength(1))
    expect(calls[0].body).toMatchObject({ kind: 'downtime', text_zh_tw: '明晚停機更新' })
    // 沒填起訖時不送，由後端預設為立即開始、不結束
    expect(calls[0].body).not.toHaveProperty('ends_at')
    expect(calls[0].body).not.toHaveProperty('starts_at')
    expect(await screen.findByText('明晚停機更新')).toBeInTheDocument()
  })

  it('sends the chosen period as ISO times', async () => {
    const { calls } = mockBackend([])
    render()
    await userEvent.type(await screen.findByLabelText('公告內容（繁體中文）'), '排程公告')
    await userEvent.type(screen.getByLabelText('開始時間'), '2026-11-20T10:00')
    await userEvent.type(screen.getByLabelText('結束時間'), '2026-11-21T10:00')
    await userEvent.click(screen.getByRole('button', { name: '發佈公告' }))
    await waitFor(() => expect(calls).toHaveLength(1))
    const body = calls[0].body as { starts_at: string; ends_at: string }
    expect(body.starts_at).toBe(new Date('2026-11-20T10:00').toISOString())
    expect(body.ends_at).toBe(new Date('2026-11-21T10:00').toISOString())
  })

  it('shows the translated error when the announcement is refused', async () => {
    mockBackend([])
    server.use(http.post('/api/admin/announcements', () => apiError(400, 'invalid_text')))
    render()
    await userEvent.type(await screen.findByLabelText('公告內容（繁體中文）'), 'x')
    await userEvent.click(screen.getByRole('button', { name: '發佈公告' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
  })

  it('disables and re-enables an announcement', async () => {
    const { calls } = mockBackend([row()])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '停用「週五停機更新」' }))
    await waitFor(() => expect(calls).toContainEqual({ method: 'PATCH', url: '/api/admin/announcements/a1', body: { enabled: false } }))
    await userEvent.click(await screen.findByRole('button', { name: '啟用「週五停機更新」' }))
    await waitFor(() => expect(calls.at(-1)?.body).toEqual({ enabled: true }))
  })

  it('edits the zh-TW text and says the translations are redone', async () => {
    const { calls } = mockBackend([row()])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '編輯「週五停機更新」' }))
    const box = screen.getByLabelText('修改公告內容（繁體中文）')
    await userEvent.clear(box)
    await userEvent.type(box, '改到週六')
    expect(screen.getByText(/英文與西班牙文會重新翻譯/)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(calls.at(-1)).toEqual({ method: 'PATCH', url: '/api/admin/announcements/a1', body: { text_zh_tw: '改到週六' } }))
  })

  it('asks for confirmation before deleting', async () => {
    const { calls } = mockBackend([row()])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '刪除「週五停機更新」' }))
    expect(calls).toHaveLength(0)
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '刪除' }))
    await waitFor(() => expect(calls).toContainEqual({ method: 'DELETE', url: '/api/admin/announcements/a1' }))
    await waitFor(() => expect(screen.queryByText('週五停機更新')).not.toBeInTheDocument())
  })

  it('turns maintenance mode on only after confirming, and off directly', async () => {
    const { calls } = mockBackend([])
    render()
    const toggle = await screen.findByRole('switch', { name: '維護模式' })
    expect(toggle).not.toBeChecked()
    await userEvent.click(toggle)
    expect(calls).toHaveLength(0)
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog).toHaveTextContent('一般使用者')
    await userEvent.click(within(dialog).getByRole('button', { name: '開啟維護模式' }))
    await waitFor(() => expect(calls).toEqual([{ method: 'PUT', url: '/api/admin/maintenance', body: { enabled: true } }]))
    await waitFor(() => expect(screen.getByRole('switch', { name: '維護模式' })).toBeChecked())

    await userEvent.click(screen.getByRole('switch', { name: '維護模式' }))
    await waitFor(() => expect(calls.at(-1)?.body).toEqual({ enabled: false }))
    await waitFor(() => expect(screen.getByRole('switch', { name: '維護模式' })).not.toBeChecked())
  })
})
