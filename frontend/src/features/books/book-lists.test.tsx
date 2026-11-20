import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { ActivityBooksButton, BookListEditor } from './book-list'
import type { Activity } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const teacher = makeMe({ teacher: true })
const book = (id: string, active = true) => { const edition = { external_sources: [], id, work_id: id, title: `Book ${id}`, authors: null, language: null, publication_year: null, isbn: null, source_url: null, description: null, recommendation_note: null, created_by: teacher.id, is_active: active, created_at: '', updated_at: '' }; return { ...edition, editions: [edition] } }

describe('topic reading lists', () => {
  it('keeps selections across language filters and saves additions and removals together', async () => {
    mockMe(teacher)
    let saved: unknown
    server.use(
      http.get('/api/topics/t1/books', () => HttpResponse.json([book('old')])),
      http.get('/api/book-works', () => HttpResponse.json([book('old'), { ...book('english'), editions: [{ ...book('english'), language: 'eng' }] }, { ...book('spanish'), editions: [{ ...book('spanish'), language: 'es' }] }])),
      http.put('/api/topics/t1/books', async ({ request }) => { saved = await request.json(); return HttpResponse.json([{ ...book('english'), editions: [{ ...book('english'), language: 'eng' }] }]) }),
    )
    renderApp(<BookListEditor path="/api/topics/t1/books" />)
    await screen.findByRole('heading', { name: '已選作品（1）' })
    await screen.findByRole('button', { name: '將「Book english」加入書單' })
    await userEvent.selectOptions(screen.getByLabelText('書目語言'), 'en')
    expect(screen.queryByRole('button', { name: '將「Book spanish」加入書單' })).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '將「Book english」加入書單' }))
    await userEvent.selectOptions(screen.getByLabelText('書目語言'), 'es')
    expect(screen.getByRole('heading', { name: '已選作品（2）' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '將「Book english」移出書單' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '將「Book old」移出書單' }))
    expect(screen.getByText('待加入 1 本，待移除 1 本')).toBeInTheDocument()
    expect(saved).toBeUndefined()
    await userEvent.click(screen.getByRole('button', { name: '儲存書單' }))
    await waitFor(() => expect(saved).toEqual({ work_ids: ['english'] }))
    expect(await screen.findByText('待加入 0 本，待移除 0 本')).toBeInTheDocument()
  })
  it('removes list links without deleting books and excludes inactive books from additions', async () => {
    mockMe(teacher)
    let selected = [book('old'), book('inactive', false)]
    const puts: unknown[] = []
    server.use(
      http.get('/api/topics', () => HttpResponse.json([{ id: 't1', title: 'Synthetic topic', description: '', category: null, is_active: true }])),
      http.get('/api/topics/t1/books', () => HttpResponse.json(selected)),
      http.get('/api/book-works', () => HttpResponse.json([book('old'), book('new'), book('inactive', false)])),
      http.put('/api/topics/t1/books', async ({ request }) => { const body = await request.json() as { work_ids: string[] }; puts.push(body); selected = body.work_ids.map((id) => book(id, id !== 'inactive')); return HttpResponse.json(selected) }),
    )
    renderApp(null, { route: '/teacher/topics' })
    await userEvent.click(await screen.findByRole('button', { name: '管理「Synthetic topic」書單' }))
    const dialog = await screen.findByRole('dialog', { name: '「Synthetic topic」書單' })
    await userEvent.click(await within(dialog).findByRole('button', { name: '將「Book old」移出書單' }))
    expect(within(dialog).queryByRole('button', { name: '將「Book inactive」加入書單' })).not.toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: '將「Book new」加入書單' }))
    expect(puts).toEqual([])
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書單' }))
    await waitFor(() => expect(puts).toEqual([{ work_ids: ['inactive', 'new'] }]))
  })
})


const activity: Activity = { id: 'a1', title: 'Synthetic activity', description: '', topic_ids: ['t1', 't2'], opens_at: null, closes_at: null, classroom_id: 'c1', status: 'published', is_archived: false, created_at: '' }

function activityMocks() {
  server.use(http.get('/api/topics', () => HttpResponse.json([{ id: 't1', title: 'Topic one' }, { id: 't2', title: 'Topic two' }])), http.get('/api/book-works', () => HttpResponse.json([book('one'), book('two'), book('new')])), http.get('/api/activities/a1/topics/:topic/books', ({ params }) => HttpResponse.json([book(params.topic === 't1' ? 'one' : 'two')])))
}

describe('activity reading lists', () => {
  it('loads and saves each topic independently without exposing stale books when switching', async () => {
    mockMe(teacher)
    activityMocks()
    const writes: { topic: string; body: unknown }[] = []
    server.use(http.put('/api/activities/a1/topics/:topic/books', async ({ request, params }) => { writes.push({ topic: String(params.topic), body: await request.json() }); return HttpResponse.json([book('two'), book('new')]) }))
    renderApp(<ActivityBooksButton activity={activity} classroomArchived={false} />)
    await userEvent.click(screen.getByRole('button', { name: '本題書單' }))
    const dialog = await screen.findByRole('dialog')
    expect(await within(dialog).findByRole('button', { name: '將「Book one」移出書單' })).toBeInTheDocument()
    await userEvent.selectOptions(within(dialog).getByLabelText('本題書單'), 't2')
    expect(within(dialog).queryByRole('button', { name: '將「Book one」移出書單' })).not.toBeInTheDocument()
    await within(dialog).findByRole('button', { name: '將「Book two」移出書單' })
    await userEvent.click(within(dialog).getByRole('button', { name: '將「Book new」加入書單' }))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書單' }))
    await waitFor(() => expect(writes).toEqual([{ topic: 't2', body: { work_ids: ['two', 'new'] } }]))
  })

  it.each([{ ...activity, is_archived: true }, activity])('is read-only when the activity or classroom is archived', async (value) => {
    mockMe(teacher)
    activityMocks()
    renderApp(<ActivityBooksButton activity={value} classroomArchived={!value.is_archived} />)
    await userEvent.click(screen.getByRole('button', { name: '本題書單' }))
    const dialog = await screen.findByRole('dialog')
    expect(await within(dialog).findByText('Book one')).toBeInTheDocument()
    expect(within(dialog).getByText('已封存，書單僅供查看。')).toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: '儲存書單' })).not.toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: /加入書單|移出書單/ })).not.toBeInTheDocument()
  })
})

it('retains selected empty works but excludes empty and all-inactive works from new selections', async () => {
 mockMe(teacher)
 const empty = { ...book('empty'), editions: [] }
 const unavailable = { ...book('unavailable'), editions: [{ ...book('version'), is_active: false }] }
 server.use(http.get('/api/topics/t1/books', () => HttpResponse.json([empty])), http.get('/api/book-works', () => HttpResponse.json([empty, unavailable, book('available')])))
 renderApp(<BookListEditor path="/api/topics/t1/books" />)
 expect(await screen.findByRole('button', { name: '將「Book empty」移出書單' })).toBeInTheDocument()
 expect(await screen.findByRole('button', { name: '將「Book available」加入書單' })).toBeInTheDocument()
 expect(screen.queryByRole('button', { name: '將「Book unavailable」加入書單' })).not.toBeInTheDocument()
})

it('keeps a successfully created work selected when the library refresh fails', async () => {
 mockMe(teacher)
 let reads = 0
 let writes = 0
 server.use(http.get('/api/topics/t1/books', () => HttpResponse.json([])), http.get('/api/book-works', () => ++reads === 1 ? HttpResponse.json([]) : HttpResponse.json({ error: { code: 'internal', request_id: 'synthetic' } }, { status: 500 })), http.post('/api/books', () => { writes++; return HttpResponse.json({ ...book('created'), title: 'New edition', work_id: 'created-work' }, { status: 201 }) }))
 renderApp(<BookListEditor path="/api/topics/t1/books" />)
 await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
 await userEvent.type(screen.getByLabelText('書名（必填）'), 'New edition')
 await userEvent.click(screen.getByRole('button', { name: '儲存書目' }))
 expect(await screen.findByRole('button', { name: '將「New edition」移出書單' })).toBeInTheDocument()
 await waitFor(() => expect(reads).toBe(2))
 expect(screen.queryByRole('button', { name: '儲存書目' })).not.toBeInTheDocument()
 expect(writes).toBe(1)
})
