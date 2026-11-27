import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const teacher = makeMe({ teacher: true })
const edition = { id: 'one', work_id: 'one', title: '合成作品', authors: null, language: 'en', publication_year: null, isbn: null, source_url: null, description: null, recommendation_note: null, external_sources: [], created_by: teacher.id, is_active: true, created_at: '', updated_at: '' }
const work = { ...edition, editions: [edition] }
const candidate = { work_id: '/works/OL1W', work_title: '合成著作', book: { source: 'Open Library', source_id: '/books/OL2M', title: '合成中文版本', authors: null, language: 'chi', publication_year: '2020', isbn: null, source_url: 'https://openlibrary.org/books/OL2M', description: null } }

describe('other edition suggestions', () => {
  beforeEach(() => mockMe(teacher))
  it('automatically finds candidates after creation and only imports after confirmation', async () => {
    let saved: unknown
    let lookups = 0
    server.use(http.get('/api/book-works', () => HttpResponse.json([work])), http.post('/api/books', () => HttpResponse.json(edition, { status: 201 })), http.get('/api/books/one/editions', () => { lookups++; return HttpResponse.json([candidate]) }), http.post('/api/book-works/one/editions', async ({ request }) => { saved = await request.json(); return HttpResponse.json(work) }))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const form = await screen.findByRole('dialog')
    await userEvent.type(within(form).getByLabelText('書名（必填）'), '合成作品')
    await userEvent.click(within(form).getByRole('button', { name: '儲存書目' }))
    const check = await screen.findByRole('checkbox', { name: /合成中文版本/ })
    expect(lookups).toBe(1)
    expect(saved).toBeUndefined()
    await userEvent.click(check)
    await userEvent.click(screen.getByRole('button', { name: '確認加入此作品' }))
    await waitFor(() => expect(saved).toMatchObject({ confirm_title: '合成作品', books: [{ title: '合成中文版本', language: 'zh', external_sources: [{ source: 'Open Library', source_id: '/books/OL2M' }] }] }))
  })
  it('lets teachers close failed lookups and retry explicitly without deleting the saved book', async () => {
    let attempts = 0
    server.use(http.get('/api/book-works', () => HttpResponse.json([work])), http.get('/api/books/one/editions', () => { attempts++; return HttpResponse.json({ error: { code: 'book_source_unavailable', request_id: 'synthetic' } }, { status: 503 }) }))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '搜尋其他版本' }))
    await screen.findByRole('alert')
    expect(attempts).toBe(1)
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: '關閉' }))
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '合成作品', level: 2 })).toBeInTheDocument()
  })
})
