import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { makeConversation } from '@/features/chat/test-data'
import type { Conversation } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

function setup(list: Conversation[] | 'error') {
  mockMe(makeMe({ student: true }))
  server.use(
    http.get('/api/conversations', () => (list === 'error' ? apiError(500, 'internal') : HttpResponse.json(list))),
  )
}

describe('ConversationListPage', () => {
  it('lists conversations with title, status, turns, date and a link', async () => {
    setup([
      makeConversation(),
      makeConversation({ id: 'c2', title: '合成題目：功利主義', status: 'ended', turn_count: 5 }),
    ])
    renderApp(null, { route: '/conversations' })
    const link = await screen.findByRole('link', { name: '開啟對話：合成題目：電車難題' })
    expect(link).toHaveAttribute('href', '/conversations/c1')
    const first = link.closest('li') as HTMLElement
    expect(within(first).getByText('進行中')).toBeInTheDocument()
    expect(within(first).getByText('3 回合')).toBeInTheDocument()
    expect(within(first).getByText(/建立於 2026/)).toBeInTheDocument()
    const second = screen.getByRole('link', { name: '開啟對話：合成題目：功利主義' }).closest('li') as HTMLElement
    expect(within(second).getByText('已結束')).toBeInTheDocument()
  })

  it('formats the date with the active language', async () => {
    setup([makeConversation()])
    renderApp(null, { route: '/conversations', lang: 'en' })
    expect(await screen.findByText(/Created Mar 5, 2026/)).toBeInTheDocument()
  })

  it('shows an empty state that links to the topic chooser', async () => {
    setup([])
    renderApp(null, { route: '/conversations' })
    expect(await screen.findByText('你還沒有任何對話。')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '選擇題目開始討論' })).toHaveAttribute('href', '/available')
  })

  it('shows a translated error with retry', async () => {
    setup('error')
    renderApp(null, { route: '/conversations' })
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    server.use(http.get('/api/conversations', () => HttpResponse.json([makeConversation()])))
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByRole('link', { name: /開啟對話/ })).toBeInTheDocument()
  })

  it('deletes only after confirmation and removes the row', async () => {
    setup([makeConversation(), makeConversation({ id: 'c2', title: '合成題目：功利主義' })])
    const deleted: string[] = []
    server.use(
      http.delete('/api/conversations/:id', ({ params }) => {
        deleted.push(String(params.id))
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderApp(null, { route: '/conversations' })
    await userEvent.click(await screen.findByRole('button', { name: '刪除對話：合成題目：電車難題' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(deleted).toEqual([])
    await userEvent.click(within(dialog).getByRole('button', { name: '刪除' }))
    await waitFor(() => expect(screen.queryByRole('link', { name: '開啟對話：合成題目：電車難題' })).not.toBeInTheDocument())
    expect(deleted).toEqual(['c1'])
    expect(screen.getByRole('link', { name: '開啟對話：合成題目：功利主義' })).toBeInTheDocument()
  })

  it('cancelling the confirmation deletes nothing', async () => {
    setup([makeConversation()])
    renderApp(null, { route: '/conversations' })
    await userEvent.click(await screen.findByRole('button', { name: /刪除對話/ }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(screen.getByRole('link', { name: /開啟對話/ })).toBeInTheDocument()
  })

  it('shows the translated error inside the dialog when deleting fails and keeps the row', async () => {
    setup([makeConversation()])
    server.use(http.delete('/api/conversations/c1', () => apiError(500, 'internal')))
    renderApp(null, { route: '/conversations' })
    await userEvent.click(await screen.findByRole('button', { name: /刪除對話/ }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '刪除' }))
    expect(await within(dialog).findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    // 對話框開啟時背景是 aria-hidden
    expect(screen.getByRole('link', { name: /開啟對話/, hidden: true })).toBeInTheDocument()
  })

  it('treats a 404 on delete as success (already deleted elsewhere): removes the row and closes the dialog', async () => {
    setup([makeConversation()])
    server.use(http.delete('/api/conversations/c1', () => apiError(404, 'not_found')))
    renderApp(null, { route: '/conversations' })
    await userEvent.click(await screen.findByRole('button', { name: /刪除對話/ }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '刪除' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(screen.queryByRole('link', { name: /開啟對話/ })).not.toBeInTheDocument()
  })

  it('omits the created date instead of rendering a blank value when the date is invalid', async () => {
    setup([makeConversation({ created_at: 'not-a-date' })])
    renderApp(null, { route: '/conversations' })
    await screen.findByRole('link', { name: /開啟對話/ })
    expect(screen.queryByText(/建立於/)).not.toBeInTheDocument()
  })
})
