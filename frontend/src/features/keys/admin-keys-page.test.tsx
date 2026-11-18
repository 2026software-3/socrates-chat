import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { AdminTeacherKey } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const item = (over: Partial<AdminTeacherKey> = {}): AdminTeacherKey => ({
  id: 'k-1',
  owner_email: 'teacher-a@example.com',
  provider: 'openai',
  provider_name: 'OpenAI',
  mask: '…aaaa',
  status: 'ok',
  status_at: null,
  created_at: '2026-11-18T01:00:00Z',
  ...over,
})

function mockAdminKeys(initial: AdminTeacherKey[]) {
  const keys = [...initial]
  const calls: string[] = []
  server.use(
    http.get('/api/admin/teacher-keys', () => HttpResponse.json(keys)),
    http.delete('/api/admin/teacher-keys/:id', ({ params }) => {
      calls.push(String(params.id))
      const i = keys.findIndex((k) => k.id === params.id)
      if (i >= 0) keys.splice(i, 1)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return calls
}

const open = async () => {
  mockMe(makeMe({ admin: true }))
  renderApp(null, { route: '/admin/teachers/keys' })
  await screen.findByRole('heading', { name: '教師 API key' })
}

describe('AdminKeysPage', () => {
  it('lists each teacher key with status but no usage and no full key', async () => {
    mockAdminKeys([item(), item({ id: 'k-2', owner_email: 'teacher-b@example.com', provider: 'anthropic', provider_name: 'Anthropic', mask: '…bbbb', status: 'invalid', status_at: '2026-11-18T02:00:00Z' })])
    await open()
    const list = await screen.findByRole('list', { name: '教師 key 清單' })
    const rows = within(list).getAllByRole('listitem')
    expect(rows).toHaveLength(2)
    expect(rows[0]).toHaveTextContent('teacher-a@example.com')
    expect(rows[0]).toHaveTextContent('OpenAI')
    expect(rows[0]).toHaveTextContent('…aaaa')
    expect(rows[0]).toHaveTextContent('可使用')
    expect(rows[1]).toHaveTextContent('失效或已被撤銷')
    expect(document.body.textContent).not.toContain('sk-')
    expect(screen.queryByText(/請求次數|token/i)).not.toBeInTheDocument()
    expect(screen.queryByRole('table')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /新增|更換|測試/ })).not.toBeInTheDocument()
    expect(screen.getByText(/看不到用量與完整 key，也不能新增或更換/)).toBeInTheDocument()
  })

  it('shows empty and error states', async () => {
    mockAdminKeys([])
    await open()
    expect(await screen.findByText('還沒有任何教師新增 key。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    server.use(http.get('/api/admin/teacher-keys', () => apiError(500, 'internal', 'req-a')))
    await open()
    expect(await screen.findByText('錯誤代碼：req-a')).toBeInTheDocument()
    mockAdminKeys([item()])
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('teacher-a@example.com')).toBeInTheDocument()
  })

  it('confirms before deleting, then deletes and says the teacher was notified', async () => {
    const calls = mockAdminKeys([item()])
    await open()
    const trigger = await screen.findByRole('button', { name: '刪除 teacher-a@example.com 的 OpenAI …aaaa key' })
    await userEvent.click(trigger)
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog).toHaveTextContent(/用量紀錄會被刪除/)
    expect(dialog).toHaveTextContent(/通知這位教師並寫入稽核紀錄/)
    expect(calls).toEqual([])
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    expect(calls).toEqual([])
    await userEvent.click(trigger)
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '確認刪除' }))
    await waitFor(() => expect(calls).toEqual(['k-1']))
    expect(await screen.findByText('已刪除 teacher-a@example.com 的 key，並通知該教師。')).toBeInTheDocument()
    expect(await screen.findByText('還沒有任何教師新增 key。')).toBeInTheDocument()
  })

  it('keeps the dialog open with the error when deleting fails', async () => {
    mockAdminKeys([item()])
    server.use(http.delete('/api/admin/teacher-keys/:id', () => apiError(500, 'internal')))
    await open()
    await userEvent.click(await screen.findByRole('button', { name: /刪除 teacher-a@example.com/ }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '確認刪除' }))
    expect(await within(await screen.findByRole('alertdialog')).findByRole('alert')).toHaveTextContent('系統發生錯誤')
  })

  it('refreshes the list when the key is already gone (404)', async () => {
    mockAdminKeys([item()])
    server.use(http.delete('/api/admin/teacher-keys/:id', () => apiError(404, 'not_found')))
    await open()
    await userEvent.click(await screen.findByRole('button', { name: /刪除 teacher-a@example.com/ }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '確認刪除' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })

  it('is offered to administrators only', async () => {
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: '/admin/teachers/keys' })
    expect(await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: '教師 API key' })).not.toBeInTheDocument()
  })
})
