import type { ReactNode } from 'react'
import { useAuth } from '@/auth/auth-context'
import { screen, within, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { expect, it, vi } from 'vitest'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'
import { WorkActions, MoveEditionButton } from './work-actions'
import type { Book, BookWork } from './types'
function Ready({ children }: { children: ReactNode }) { const auth = useAuth(); return auth.status === 'authed' ? children : null }
const stamps = { created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z' }
const edition = { id: 'edition', work_id: 'source', title: 'Edition', external_sources: [], is_active: true, ...stamps } as unknown as Book
const source = { id: 'source', title: 'Source', is_active: true, editions: [edition], ...stamps } as BookWork
const target = { id: 'target', title: 'Target', is_active: false, editions: [], ...stamps } as unknown as BookWork
it('cancels merge without writing and confirms source title and inactive destination explicitly', async () => {
 mockMe(makeMe({ teacher: true }))
 let payload: unknown
 const changed = vi.fn()
 server.use(http.get('/api/book-works/source/impact', () => HttpResponse.json({ edition_count: 1, topic_count: 2, activity_topic_count: 3 })), http.post('/api/book-works/source/merge', async ({ request }) => { payload = await request.json(); return HttpResponse.json(target) }))
 renderApp(<Ready><WorkActions work={source} works={[source, target]} changed={changed} /></Ready>)
 await userEvent.click(await screen.findByRole('button', { name: '合併作品' }))
 let dialog = await screen.findByRole('alertdialog')
 await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
 expect(payload).toBeUndefined()
 await userEvent.click(await screen.findByRole('button', { name: '合併作品' }))
 dialog = await screen.findByRole('alertdialog')
 await userEvent.selectOptions(within(dialog).getByLabelText('目的作品'), 'target')
 expect(within(dialog).getByText('目的作品: Target · 已停用')).toBeInTheDocument()
 await userEvent.click(within(dialog).getByRole('button', { name: '確認' }))
 await waitFor(() => expect(payload).toEqual({ target_work_id: 'target', confirm_title: 'Source' }))
 expect(changed).toHaveBeenCalledOnce()
})
it('shows empty-source warning before moving the last edition and sends edition title', async () => {
 mockMe(makeMe({ teacher: true }))
 let payload: unknown
 server.use(http.get('/api/books/edition/work-impact', () => HttpResponse.json({ edition_count: 1, topic_count: 2, activity_topic_count: 3 })), http.put('/api/books/edition/work', async ({ request }) => { payload = await request.json(); return HttpResponse.json(edition) }))
 renderApp(<MoveEditionButton book={edition} works={[source, target]} changed={() => {}} />)
 await userEvent.click(screen.getByRole('button', { name: '移動版本' }))
 const dialog = await screen.findByRole('alertdialog')
 expect(await within(dialog).findByText(/移動後來源作品將無版本/)).toBeInTheDocument()
 await userEvent.selectOptions(within(dialog).getByLabelText('目的作品'), 'target')
 await userEvent.click(within(dialog).getByRole('button', { name: '確認' }))
 await waitFor(() => expect(payload).toEqual({ work_id: 'target', confirm_title: 'Edition' }))
})
it('explains that an administrator must reactivate the source work when regrouping fails', async () => {
  mockMe(makeMe({ teacher: true }))
  const changed = vi.fn()
  server.use(
    http.get('/api/book-works/source/impact', () =>
      HttpResponse.json({ edition_count: 1, topic_count: 0, activity_topic_count: 0 })),
    http.post('/api/book-works/source/merge', () =>
      HttpResponse.json({ error: { code: 'book_work_inactive', request_id: 'test' } }, { status: 409 })),
  )
  renderApp(<Ready><WorkActions work={source} works={[source, target]} changed={changed} /></Ready>)

  await userEvent.click(await screen.findByRole('button', { name: '合併作品' }))
  const dialog = await screen.findByRole('alertdialog')
  await userEvent.selectOptions(within(dialog).getByLabelText('目的作品'), 'target')
  const confirm = within(dialog).getByRole('button', { name: '確認' })
  await waitFor(() => expect(confirm).toBeEnabled())
  await userEvent.click(confirm)

  expect(await within(dialog).findByRole('alert')).toHaveTextContent('來源作品已停用，請先由管理者重新啟用。')
  expect(changed).not.toHaveBeenCalled()
})

it('shows the archived-list conflict and keeps the merge dialog open', async () => {
  mockMe(makeMe({ teacher: true }))
  const changed = vi.fn()
  server.use(
    http.get('/api/book-works/source/impact', () => HttpResponse.json({
      edition_count: 1, topic_count: 1, activity_topic_count: 2,
    })),
    http.post('/api/book-works/source/merge', () => HttpResponse.json({
      error: { code: 'book_work_archived_list', request_id: 'archived-list-merge' },
    }, { status: 409 })),
  )
  renderApp(<Ready><WorkActions work={source} works={[source, target]} changed={changed} /></Ready>)
  await userEvent.click(await screen.findByRole('button', { name: '合併作品' }))
  const dialog = await screen.findByRole('alertdialog')
  await userEvent.selectOptions(within(dialog).getByLabelText('目的作品'), 'target')
  const confirm = within(dialog).getByRole('button', { name: '確認' })
  await waitFor(() => expect(confirm).toBeEnabled())
  await userEvent.click(confirm)
  expect(await within(dialog).findByRole('alert')).toHaveTextContent('來源作品已列入封存活動或封存教室的書單，無法合併。')
  expect(dialog).toBeInTheDocument()
  expect(changed).not.toHaveBeenCalled()
})
