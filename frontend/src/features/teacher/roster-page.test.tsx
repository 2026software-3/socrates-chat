import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { RosterPage } from '@/features/teacher/roster-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const emails = ['alice@example.com', 'bob@example.com', 'carol@example.org']

/** 名單上的學生（已有帳號、使用中）。 */
const rows = (list: string[]) =>
  list.map((email) => ({ email, display_name: null, has_account: true, disabled: false, completed_conversations: 0 }))

function mockRoster(list: string[] = emails) {
  server.use(http.get('/api/students', () => HttpResponse.json(rows(list))))
}

function setup() {
  return renderTeacherPage(<RosterPage />)
}

describe('RosterPage', () => {
  it('shows the count and the emails', async () => {
    mockRoster()
    setup()
    expect(await screen.findByText('alice@example.com')).toBeInTheDocument()
    expect(screen.getByText('學生人數：3')).toBeInTheDocument()
    expect(screen.getByText('carol@example.org')).toBeInTheDocument()
  })

  it('filters as you type and shows a no-match message', async () => {
    mockRoster()
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.type(screen.getByLabelText('搜尋電子郵件'), 'EXAMPLE.org')
    expect(screen.queryByText('alice@example.com')).not.toBeInTheDocument()
    expect(screen.getByText('carol@example.org')).toBeInTheDocument()
    await userEvent.clear(screen.getByLabelText('搜尋電子郵件'))
    await userEvent.type(screen.getByLabelText('搜尋電子郵件'), 'zzz')
    expect(screen.getByText('沒有符合的電子郵件。')).toBeInTheDocument()
  })

  it('shows loading, empty and error states', async () => {
    mockRoster([])
    setup()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('名單是空的，請從下方匯入學生電子郵件。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    let calls = 0
    server.use(
      http.get('/api/students', () => {
        calls += 1
        return calls === 1 ? apiError(500, 'internal') : HttpResponse.json(rows(emails))
      }),
    )
    setup()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('alice@example.com')).toBeInTheDocument()
  })

  it('imports pasted text and shows added, existing and invalid lines', async () => {
    mockRoster()
    let body: unknown
    server.use(
      http.post('/api/roster/import', async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({
          added: 2,
          existing: 1,
          invalid: [{ line: 4, value: 'not-an-email' }],
        })
      }),
    )
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.type(screen.getByLabelText('電子郵件（每行一個）'), 'x@example.com')
    await userEvent.click(screen.getByRole('button', { name: '匯入' }))
    const result = await screen.findByRole('region', { name: '匯入結果' })
    expect(body).toEqual({ text: 'x@example.com' })
    expect(within(result).getByText('新增：2')).toBeInTheDocument()
    expect(within(result).getByText('已在名單中：1')).toBeInTheDocument()
    expect(within(result).getByText('第 4 行：not-an-email')).toBeInTheDocument()
  })

  it('refuses to import empty text', async () => {
    mockRoster()
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.click(screen.getByRole('button', { name: '匯入' }))
    expect(await screen.findByText('請貼上至少一個電子郵件。')).toBeInTheDocument()
  })

  it('does not double submit an import and surfaces errors', async () => {
    mockRoster()
    let count = 0
    server.use(
      http.post('/api/roster/import', async () => {
        count += 1
        await new Promise((r) => setTimeout(r, 60))
        return apiError(403, 'forbidden')
      }),
    )
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.type(screen.getByLabelText('電子郵件（每行一個）'), 'x@example.com')
    await userEvent.dblClick(screen.getByRole('button', { name: '匯入' }))
    expect(await screen.findByText('你沒有權限執行這個操作。')).toBeInTheDocument()
    expect(count).toBe(1)
  })

  it('fills the textarea from a chosen file without uploading the file', async () => {
    mockRoster()
    setup()
    await screen.findByText('alice@example.com')
    const file = new File(['email\ndan@example.com\n'], 'roster.csv', {
      type: 'text/csv',
    })
    await userEvent.upload(screen.getByLabelText('從 .csv 或 .txt 檔載入'), file)
    await waitFor(() => expect(screen.getByLabelText('電子郵件（每行一個）')).toHaveValue('email\ndan@example.com\n'))
  })

  it('removes a student after confirmation and reloads', async () => {
    let list = [...emails]
    let deleted = ''
    server.use(
      http.get('/api/students', () => HttpResponse.json(rows(list))),
      http.delete('/api/roster/:email', ({ request, params }) => {
        deleted = new URL(request.url).pathname
        list = list.filter((e) => e !== String(params.email))
        return new HttpResponse(null, { status: 204 })
      }),
    )
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 alice@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText(/學生已有的對話與總結資料會保留/)).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: '移除' }))
    expect(await screen.findByText('已移出修課名單。')).toBeInTheDocument()
    expect(deleted).toBe('/api/roster/alice%40example.com')
    await waitFor(() => expect(screen.queryByText('alice@example.com')).not.toBeInTheDocument())
  })

  it('cancelling the confirm dialog sends nothing', async () => {
    let deleted = false
    server.use(
      http.delete('/api/roster/:email', () => {
        deleted = true
        return new HttpResponse(null, { status: 204 })
      }),
    )
    mockRoster()
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 bob@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(deleted).toBe(false)
  })

  it('shows a translated message for a non-404 removal failure', async () => {
    server.use(
      http.get('/api/students', () => HttpResponse.json(rows(emails))),
      http.delete('/api/roster/:email', () => apiError(500, 'internal')),
    )
    setup()
    await screen.findByText('bob@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 bob@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '移除' }))
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
  })

  it('shows an error when the file cannot be read', async () => {
    mockRoster()
    setup()
    await screen.findByText('alice@example.com')
    const file = new File(['x'], 'roster.csv', { type: 'text/csv' })
    Object.defineProperty(file, 'text', {
      value: () => Promise.reject(new Error('boom')),
    })
    await userEvent.upload(screen.getByLabelText('從 .csv 或 .txt 檔載入'), file)
    expect(await screen.findByText('無法讀取這個檔案。')).toBeInTheDocument()
  })

  it('clears the previous import result when a new import fails', async () => {
    mockRoster()
    let calls = 0
    server.use(
      http.post('/api/roster/import', () => {
        calls += 1
        return calls === 1 ? HttpResponse.json({ added: 1, existing: 0, invalid: [] }) : apiError(403, 'forbidden')
      }),
    )
    setup()
    await screen.findByText('alice@example.com')
    const box = screen.getByLabelText('電子郵件（每行一個）')
    await userEvent.type(box, 'x@example.com')
    await userEvent.click(screen.getByRole('button', { name: '匯入' }))
    await screen.findByRole('region', { name: '匯入結果' })
    await userEvent.type(box, 'y@example.com')
    await userEvent.click(screen.getByRole('button', { name: '匯入' }))
    expect(await screen.findByText('你沒有權限執行這個操作。')).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: '匯入結果' })).not.toBeInTheDocument()
  })

  it('shows an error with retry when a reload fails while old data is on screen', async () => {
    let calls = 0
    server.use(
      http.get('/api/students', () => {
        calls += 1
        return calls === 2 ? apiError(500, 'internal') : HttpResponse.json(rows(emails))
      }),
      http.post('/api/roster/import', () => HttpResponse.json({ added: 1, existing: 0, invalid: [] })),
    )
    setup()
    await screen.findByText('alice@example.com')
    await userEvent.type(screen.getByLabelText('電子郵件（每行一個）'), 'x@example.com')
    await userEvent.click(screen.getByRole('button', { name: '匯入' }))
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByText('alice@example.com')).toBeInTheDocument()
  })

  it('shows a translated message and refreshes when removal returns 404', async () => {
    let list = [...emails]
    server.use(
      http.get('/api/students', () => HttpResponse.json(rows(list))),
      http.delete('/api/roster/:email', () => {
        list = list.filter((e) => e !== 'bob@example.com')
        return apiError(404, 'not_found')
      }),
    )
    setup()
    await screen.findByText('bob@example.com')
    await userEvent.click(screen.getByRole('button', { name: '移除 bob@example.com' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '移除' }))
    expect(await screen.findByText('找不到資料。')).toBeInTheDocument()
    await waitFor(() => expect(screen.queryByText('bob@example.com')).not.toBeInTheDocument())
  })

  it('shows the temporary passwords of newly created accounts once', async () => {
    mockRoster([])
    server.use(
      http.post('/api/roster/import', () =>
        HttpResponse.json({
          added: 1,
          existing: 0,
          invalid: [],
          credentials: [{ email: 'new@example.com', temporary_password: 'ABCD2345WXYZ' }],
        }),
      ),
    )
    setup()
    await userEvent.type(await screen.findByLabelText('電子郵件（每行一個）'), 'new@example.com')
    await userEvent.click(screen.getByRole('button', { name: '匯入' }))
    expect(await screen.findByRole('heading', { name: '臨時密碼（新帳號）' })).toBeInTheDocument()
    expect(screen.getByText('ABCD2345WXYZ')).toBeInTheDocument()
  })

  it('does not offer password reset to a plain teacher', async () => {
    mockRoster(['alice@example.com'])
    setup()
    await screen.findByText('alice@example.com')
    expect(screen.queryByRole('button', { name: '重設 alice@example.com 的密碼' })).not.toBeInTheDocument()
  })

  it('lets an admin reset a student password', async () => {
    mockRoster(['alice@example.com'])
    server.use(
      http.post('/api/admin/users/reset-password', () =>
        HttpResponse.json({ email: 'alice@example.com', temporary_password: 'ABCD2345WXYZ' }),
      ),
    )
    mockMe(makeMe({ admin: true }))
    renderApp(<RosterPage />)
    await userEvent.click(await screen.findByRole('button', { name: '重設 alice@example.com 的密碼' }))
    await userEvent.click(screen.getByRole('button', { name: '重設密碼' }))
    expect(await screen.findByLabelText('臨時密碼')).toHaveValue('ABCD2345WXYZ')
  })
})
