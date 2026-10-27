import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Classroom } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const existing: Classroom = {
  id: '00000000-0000-4000-8000-000000000101',
  name: '2026 春季班',
  year_label: '2026',
  description: '春季討論課',
  created_at: '2026-10-01T00:00:00Z',
  is_archived: false,
  updated_at: '2026-10-01T00:00:00Z',
}

function renderTeacher() {
  mockMe(makeMe({ teacher: true }))
  renderApp(null, { route: '/teacher/classrooms' })
}

describe('classroom management', () => {
  it('shows the teacher classrooms, creates another, and refreshes the list', async () => {
    const rows = [existing]
    const createdBodies: unknown[] = []
    server.use(
      http.get('/api/classrooms', () => HttpResponse.json(rows)),
      http.post('/api/classrooms', async ({ request }) => {
        const body = (await request.json()) as { name: string; year_label: string | null; description: string | null }
        createdBodies.push(body)
        rows.push({
          ...existing,
          id: '00000000-0000-4000-8000-000000000102',
          name: body.name,
          year_label: body.year_label,
          description: body.description,
        })
        return HttpResponse.json(rows[1], { status: 201 })
      }),
    )
    renderTeacher()
    expect(await screen.findByText('2026 春季班')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: '+ 建立教室' }))
    const dialog = await screen.findByRole('dialog', { name: '建立新教室' })
    expect(within(dialog).getByRole('button', { name: '建立教室' })).toBeDisabled()
    await userEvent.type(within(dialog).getByLabelText('教室名稱'), '  2026 秋季班  ')
    await userEvent.click(within(dialog).getByRole('button', { name: '建立教室' }))

    expect(await screen.findByText('2026 秋季班')).toBeInTheDocument()
    expect(createdBodies).toEqual([{ name: '2026 秋季班', year_label: null, description: null }])
    expect(screen.getByText('2026 春季班')).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('教室已建立。')
  })

  it('shows a loading state followed by the empty state', async () => {
    let release!: () => void
    const gate = new Promise<void>((resolve) => (release = resolve))
    server.use(
      http.get('/api/classrooms', async () => {
        await gate
        return HttpResponse.json([])
      }),
    )
    renderTeacher()
    expect(screen.getByRole('status')).toBeInTheDocument()
    release()
    expect(await screen.findByText('你還沒有教室，先建立一間吧。')).toBeInTheDocument()
  })

  it('shows a translated error and retry action when the classroom list fails', async () => {
    server.use(http.get('/api/classrooms', () => apiError(500, 'internal')))
    renderTeacher()
    expect(await screen.findByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。')
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('你還沒有教室，先建立一間吧。')).toBeInTheDocument()
  })

  it('shows a clear error when the classroom name is already taken', async () => {
    server.use(
      http.get('/api/classrooms', () => HttpResponse.json([])),
      http.post('/api/classrooms', () => apiError(409, 'classroom_name_taken')),
    )
    renderTeacher()
    await screen.findByText('你還沒有教室，先建立一間吧。')
    await userEvent.click(screen.getByRole('button', { name: '+ 建立教室' }))
    const dialog = await screen.findByRole('dialog', { name: '建立新教室' })
    await userEvent.type(within(dialog).getByLabelText('教室名稱'), '重複教室')
    await userEvent.click(within(dialog).getByRole('button', { name: '建立教室' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('已有同名教室，請使用其他名稱。')
  })

  it('lets a teacher edit their own classroom', async () => {
    let current = existing
    const patchBodies: unknown[] = []
    server.use(
      http.get('/api/classrooms', () => HttpResponse.json([current])),
      http.patch('/api/classrooms/:id', async ({ request }) => {
        const body = (await request.json()) as Partial<typeof existing>
        patchBodies.push(body)
        current = { ...current, ...body }
        return HttpResponse.json(current)
      }),
    )
    renderTeacher()
    const row = await screen.findByRole('row', { name: /2026 春季班/ })
    await userEvent.click(within(row).getByRole('button', { name: '編輯 2026 春季班' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.clear(within(dialog).getByLabelText('教室名稱'))
    await userEvent.type(within(dialog).getByLabelText('教室名稱'), '2026 春季哲學班')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存變更' }))
    await waitFor(() => expect(patchBodies).toEqual([{ name: '2026 春季哲學班', year_label: '2026', description: '春季討論課' }]))
    expect(await screen.findByText('2026 春季哲學班')).toBeInTheDocument()
  })

  it('allows admin-only users to manage classrooms without a create action', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([existing])))
    renderApp(null, { route: '/admin/classrooms' })
    expect(await screen.findByText('2026 春季班')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '建立教室' })).not.toBeInTheDocument()
  })

  it('shows a neutral empty state to an admin who cannot create classrooms', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    renderApp(null, { route: '/admin/classrooms' })
    expect(await screen.findByText('目前沒有任何教室。')).toBeInTheDocument()
    expect(screen.queryByText('你還沒有教室，先建立一間吧。')).not.toBeInTheDocument()
  })
})

it('archives a classroom, moves it to the archived view and restores it', async () => {
  let row = { ...existing, is_archived: false }
  server.use(
    http.get('/api/classrooms', () => HttpResponse.json([row])),
    http.patch('/api/classrooms/:id', async ({ request }) => {
      row = { ...row, ...await request.json() as { is_archived: boolean } }
      return HttpResponse.json(row)
    }),
  )
  renderTeacher()
  await screen.findByText(existing.name)
  await userEvent.click(screen.getByRole('button', { name: '封存教室' }))
  const confirm = await screen.findByRole('alertdialog')
  expect(confirm).toHaveTextContent(`確定要封存「${existing.name}」嗎？`)
  await userEvent.click(within(confirm).getByRole('button', { name: '封存' }))
  // 封存後從預設清單隱藏，改在「已封存」檢視
  await waitFor(() => expect(screen.queryByText(existing.name)).not.toBeInTheDocument())
  await userEvent.click(screen.getByRole('button', { name: '已封存' }))
  expect(await screen.findByText('已封存（唯讀）')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: `編輯 ${existing.name}` })).toBeDisabled()
  await userEvent.click(screen.getByRole('button', { name: '復原教室' }))
  await waitFor(() => expect(screen.queryByText('已封存（唯讀）')).not.toBeInTheDocument())
  expect(row.is_archived).toBe(false)
})

it('deletes a classroom only after its name is typed, even when archived', async () => {
  let rows = [{ ...existing, is_archived: true }]
  let body: unknown
  server.use(
    http.get('/api/classrooms', () => HttpResponse.json(rows)),
    http.delete('/api/classrooms/:id', async ({ request }) => {
      body = await request.json()
      rows = []
      return new HttpResponse(null, { status: 204 })
    }),
  )
  renderTeacher()
  await screen.findByText('你還沒有教室，先建立一間吧。')
  await userEvent.click(screen.getByRole('button', { name: '已封存' }))
  await userEvent.click(await screen.findByRole('button', { name: '刪除教室' }))
  const confirm = screen.getByRole('button', { name: '永久刪除' })
  expect(confirm).toBeDisabled()
  await userEvent.type(screen.getByLabelText(`請輸入「${existing.name}」以確認`), existing.name)
  await userEvent.click(confirm)
  await waitFor(() => expect(body).toEqual({ confirm_name: existing.name }))
  expect(await screen.findByText('教室已刪除。')).toBeInTheDocument()
})
