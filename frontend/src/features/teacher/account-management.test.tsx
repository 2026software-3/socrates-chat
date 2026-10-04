import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { RosterPage } from '@/features/teacher/roster-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { StudentRow } from '@/lib/types'
import { apiError } from '@/test/render'
import { server } from '@/test/server'

const student = (email: string, over: Partial<StudentRow> = {}): StudentRow => ({
  email,
  display_name: null,
  has_account: true,
  disabled: false,
  completed_conversations: 0,
  ...over,
})

const roster = [
  student('active@example.com', { display_name: 'Ada', completed_conversations: 3 }),
  student('later@example.com', { has_account: false }),
  student('off@example.com', { disabled: true }),
]

function mockStudents(rows: StudentRow[] = roster) {
  server.use(http.get('/api/students', () => HttpResponse.json(rows)))
}

describe('student account management', () => {
  it('shows account status and the number of completed conversations', async () => {
    mockStudents()
    renderTeacherPage(<RosterPage />)
    const row = (email: string) => screen.getByText(email).closest('tr') as HTMLElement
    await screen.findByText('active@example.com')
    expect(within(row('active@example.com')).getByText('使用中')).toBeInTheDocument()
    expect(within(row('active@example.com')).getByText('Ada')).toBeInTheDocument()
    expect(within(row('active@example.com')).getByText('3')).toBeInTheDocument()
    expect(within(row('later@example.com')).getByText('尚未登入')).toBeInTheDocument()
    expect(within(row('off@example.com')).getByText('已停用')).toBeInTheDocument()
  })

  it('teachers cannot disable, restore or reset passwords — only admins can', async () => {
    mockStudents()
    renderTeacherPage(<RosterPage />)
    await screen.findByText('active@example.com')
    expect(screen.queryByRole('button', { name: /停用 /u })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /復原 /u })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /重設/u })).not.toBeInTheDocument()
    // 教師可以刪除學生帳號與移出名單
    expect(screen.getByRole('button', { name: '刪除 active@example.com 的帳號' })).toBeInTheDocument()
  })

  it('an admin disables an account with an optional reason', async () => {
    mockStudents()
    let call: { url: string; body: unknown } | undefined
    server.use(
      http.post('/api/admin/users/:email/disable', async ({ request }) => {
        call = { url: new URL(request.url).pathname, body: await request.json() }
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderTeacherPage(<RosterPage />, undefined, { admin: true })
    await screen.findByText('active@example.com')
    // 沒有帳號的學生沒有停用按鈕
    expect(screen.queryByRole('button', { name: '停用 later@example.com' })).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '停用 active@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.type(within(dialog).getByLabelText(/原因/u), '測試')
    await userEvent.click(within(dialog).getByRole('button', { name: '停用' }))
    expect(await screen.findByText('帳號已停用。')).toBeInTheDocument()
    expect(call).toEqual({ url: '/api/admin/users/active%40example.com/disable', body: { reason: '測試' } })
  })

  it('an admin restores a disabled account', async () => {
    mockStudents()
    let url = ''
    server.use(
      http.post('/api/admin/users/:email/enable', ({ request }) => {
        url = new URL(request.url).pathname
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderTeacherPage(<RosterPage />, undefined, { admin: true })
    await screen.findByText('off@example.com')
    await userEvent.click(screen.getByRole('button', { name: '復原 off@example.com' }))
    expect(await screen.findByText('帳號已復原。')).toBeInTheDocument()
    expect(url).toBe('/api/admin/users/off%40example.com/enable')
  })

  it('shows a translated message when the last admin cannot be disabled', async () => {
    mockStudents()
    server.use(http.post('/api/admin/users/:email/disable', () => apiError(409, 'last_admin')))
    renderTeacherPage(<RosterPage />, undefined, { admin: true })
    await screen.findByText('active@example.com')
    await userEvent.click(screen.getByRole('button', { name: '停用 active@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '停用' }))
    expect(await within(dialog).findByText('至少需保留一位啟用中的管理者。')).toBeInTheDocument()
  })

  it('deleting an account requires typing the email again', async () => {
    mockStudents()
    let call: { url: string; body: unknown } | undefined
    server.use(
      http.post('/api/students/:email/delete', async ({ request }) => {
        call = { url: new URL(request.url).pathname, body: await request.json() }
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderTeacherPage(<RosterPage />)
    await screen.findByText('active@example.com')
    await userEvent.click(screen.getByRole('button', { name: '刪除 active@example.com 的帳號' }))
    const dialog = await screen.findByRole('alertdialog')
    const confirm = within(dialog).getByRole('button', { name: '永久刪除' })
    expect(confirm).toBeDisabled()
    await userEvent.type(within(dialog).getByLabelText(/請輸入/u), 'wrong@example.com')
    expect(confirm).toBeDisabled()
    await userEvent.clear(within(dialog).getByLabelText(/請輸入/u))
    await userEvent.type(within(dialog).getByLabelText(/請輸入/u), 'Active@example.com')
    expect(confirm).toBeEnabled()
    await userEvent.click(confirm)
    expect(await screen.findByText('學生帳號已刪除。')).toBeInTheDocument()
    expect(call).toEqual({
      url: '/api/students/active%40example.com/delete',
      body: { confirm_email: 'Active@example.com' },
    })
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })

  it('cancelling the delete dialog sends nothing', async () => {
    mockStudents()
    let called = false
    server.use(
      http.post('/api/students/:email/delete', () => {
        called = true
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderTeacherPage(<RosterPage />)
    await screen.findByText('active@example.com')
    await userEvent.click(screen.getByRole('button', { name: '刪除 active@example.com 的帳號' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '取消' }))
    expect(called).toBe(false)
  })
})
