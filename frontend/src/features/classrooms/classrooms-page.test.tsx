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

    await userEvent.type(screen.getByLabelText('教室名稱'), '  2026 秋季班  ')
    await userEvent.type(screen.getByLabelText('年度標籤'), '2026')
    await userEvent.type(screen.getByLabelText('說明'), '秋季討論課')
    await userEvent.click(screen.getByRole('button', { name: '建立教室' }))

    expect(await screen.findByText('2026 秋季班')).toBeInTheDocument()
    expect(createdBodies).toEqual([{ name: '2026 秋季班', year_label: '2026', description: '秋季討論課' }])
    expect(screen.getByText('2026 春季班')).toBeInTheDocument()
    expect(screen.getByLabelText('教室名稱')).toHaveValue('')
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
    await userEvent.type(screen.getByLabelText('教室名稱'), '重複教室')
    await userEvent.click(screen.getByRole('button', { name: '建立教室' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('這個教室名稱已被使用，請換一個名稱。')
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
    const row = await screen.findByRole('listitem', { name: /2026 春季班/ })
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
