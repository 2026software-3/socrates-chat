import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { RosterPage } from './roster-page'
import { mockClassrooms, renderTeacherPage, ROOM_A, student } from './test-helpers'
import { apiError, pageOf } from '@/test/render'
import { server } from '@/test/server'

const id = '22222222-2222-4222-8222-222222222222'
const endpoint = `/api/students/${id}/student-number`

function setup(admin = false) {
  mockClassrooms()
  let number = '11234567A'
  server.use(http.get(`/api/classrooms/${ROOM_A}/students`, () => pageOf([
    student(null, { id, student_number: number, display_name: '測試學生' }),
  ], null, { total: 1 })))
  renderTeacherPage(<RosterPage />, undefined, admin ? { admin: true } : undefined)
  return (value: string) => { number = value }
}

async function open() {
  await userEvent.click(await screen.findByRole('button', { name: '更正 11234567A 的學號' }))
  return screen.findByRole('alertdialog')
}

describe('student number correction', () => {
  for (const admin of [false, true]) {
    it(`corrects a student's number and refreshes the list (${admin ? 'admin' : 'teacher'})`, async () => {
      const update = setup(admin)
      let body: unknown
      server.use(http.put(endpoint, async ({ request }) => {
        body = await request.json()
        update('87654321B')
        return new HttpResponse(null, { status: 204 })
      }))
      const dialog = await open()
      expect(within(dialog).getByText(/所有教室/)).toBeInTheDocument()
      expect(within(dialog).queryByRole('textbox', { name: /電子郵件/ })).not.toBeInTheDocument()
      const input = within(dialog).getByRole('textbox', { name: '新學號' })
      await userEvent.clear(input)
      await userEvent.type(input, '87654321b')
      await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
      expect(await screen.findByText('學號已更正，已通知學生。')).toBeInTheDocument()
      await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
      expect(await screen.findByText('87654321B')).toBeInTheDocument()
      expect(body).toEqual({ student_number: '87654321b' })
    })
  }

  it('validates locally and allows retry after a duplicate error', async () => {
    setup()
    let calls = 0
    server.use(http.put(endpoint, () => {
      calls++
      return calls === 1 ? apiError(409, 'student_number_in_use') : new HttpResponse(null, { status: 204 })
    }))
    const dialog = await open()
    const input = within(dialog).getByRole('textbox', { name: '新學號' })
    await userEvent.clear(input)
    await userEvent.type(input, 'bad')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent('學號格式不正確。')
    expect(calls).toBe(0)
    await userEvent.clear(input)
    await userEvent.type(input, '87654321B')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('這個學號已被其他帳號使用。')
    expect(input).toHaveValue('87654321B')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(calls).toBe(2)
  })

  it('cancels without sending a request', async () => {
    setup()
    let calls = 0
    server.use(http.put(endpoint, () => { calls++; return new HttpResponse(null, { status: 204 }) }))
    const dialog = await open()
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    expect(calls).toBe(0)
  })

  it('prevents duplicate submissions and shows a translated failure', async () => {
    setup()
    let calls = 0
    server.use(http.put(endpoint, async () => {
      calls++
      await delay(100)
      return apiError(500, 'internal')
    }))
    const dialog = await open()
    const input = within(dialog).getByRole('textbox', { name: '新學號' })
    await userEvent.clear(input)
    await userEvent.type(input, '87654321B')
    await userEvent.dblClick(within(dialog).getByRole('button', { name: '儲存' }))
    await waitFor(() => expect(within(dialog).getByRole('alert')).toHaveTextContent('系統發生錯誤，請稍後再試。'))
    expect(calls).toBe(1)
    expect(input).toHaveValue('87654321B')
  })
})
