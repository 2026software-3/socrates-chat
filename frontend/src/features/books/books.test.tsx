import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const teacher = makeMe({ teacher: true })
const edition = (id: string, overrides = {}) => ({ external_sources: [], id, work_id: id, title: `Book ${id}`, authors: null, language: null, publication_year: null, isbn: null, source_url: null, description: null, recommendation_note: null, created_by: teacher.id, is_active: true, created_at: '', updated_at: '', ...overrides })
const book = (id: string, overrides = {}) => ({ ...edition(id, overrides), editions: [edition(id, overrides)] })

describe('shared books', () => {
  it('automatically supplements one edition without saving or replacing entered fields', async () => {
    mockMe(teacher)
    let saved: unknown
    server.use(
      http.get('/api/book-works', () => HttpResponse.json([])),
      http.get('/api/books/search', () => HttpResponse.json([{ source: 'Google Books', source_id: 'single', title: 'Original', authors: 'Author', language: 'en', publication_year: '2025', isbn: null, source_url: null, description: 'External description' }])),
      http.post('/api/books', async ({ request }) => { saved = await request.json(); return HttpResponse.json(book('new'), { status: 201 }) }),
    )
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Original')
    await userEvent.type(within(dialog).getByLabelText('簡介（選填）'), 'My description')
    await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    await waitFor(() => expect(within(dialog).getByLabelText('作者（選填；多人以分號分隔）')).toHaveValue('Author'))
    expect(within(dialog).getByLabelText('簡介（選填）')).toHaveValue('My description')
    expect(within(dialog).queryByRole('button', { name: '選擇此版本' })).not.toBeInTheDocument()
    expect(saved).toBeUndefined()
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(saved).toMatchObject({ authors: 'Author', description: 'My description', external_sources: [{ source: 'Google Books', source_id: 'single' }] }))
  })
  it.each(['title', 'description'] as const)('preserves %s edits made while supplementation is pending', async (field) => {
    mockMe(teacher)
    let release!: () => void
    const gate = new Promise<void>((resolve) => { release = resolve })
    server.use(
      http.get('/api/book-works', () => HttpResponse.json([])),
      http.get('/api/books/search', async () => { await gate; return HttpResponse.json([{ source: 'Google Books', source_id: 'late', title: 'Original', authors: 'External author', language: null, publication_year: null, isbn: null, source_url: null, description: 'External description' }]) }),
    )
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Original')
    await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    const input = within(dialog).getByLabelText(field === 'title' ? '書名（必填）' : '簡介（選填）')
    fireEvent.change(input, { target: { value: 'My new input' } })
    release()
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '依書名搜尋補資料' })).toBeEnabled())
    expect(input).toHaveValue('My new input')
    expect(within(dialog).getByLabelText('作者（選填；多人以分號分隔）')).toHaveValue(field === 'title' ? '' : 'External author')
  })
  it('filters library by language and status while keeping global counts and newest order', async () => {
    mockMe(teacher)
    server.use(http.get('/api/book-works', () => HttpResponse.json([
      book('old', { language: 'en', created_at: '2025-01-01T00:00:00Z' }),
      book('new', { language: 'eng', created_at: '2026-01-01T00:00:00Z' }),
      book('inactive', { language: 'zh-TW', is_active: false, created_at: '2026-02-01T00:00:00Z' }),
    ])))
    renderApp(null, { route: '/teacher/books' })
    await screen.findByRole('heading', { name: 'Book new', level: 2 })
    expect(screen.getByRole('button', { name: '全部作品（3）' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '啟用（2）' })).toBeInTheDocument()
    await userEvent.selectOptions(screen.getByLabelText('書目語言'), 'en')
    expect(screen.queryByRole('heading', { name: 'Book inactive', level: 2 })).not.toBeInTheDocument()
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(['Book new', 'Book old'])
    await userEvent.selectOptions(screen.getByLabelText('排序方式'), 'oldest')
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(['Book old', 'Book new'])
    await userEvent.click(screen.getByRole('button', { name: '已停用（1）' }))
    expect(screen.getByText('沒有符合條件的書目。')).toBeInTheDocument()
    await userEvent.selectOptions(screen.getByLabelText('書目語言'), 'all')
    expect(screen.getByRole('heading', { name: 'Book inactive', level: 2 })).toBeInTheDocument()
  })
  it('lets a teacher create with only a title and edit only their own books', async () => {
    mockMe(teacher)
    const books = [book('own'), book('other', { created_by: 'other-teacher' })]
    const requests: unknown[] = []
    server.use(
      http.get('/api/book-works', () => HttpResponse.json(books)),
      http.post('/api/books', async ({ request }) => {
        const data = await request.json() as object
        requests.push(data)
        const created = book('new', data)
        books.push(created)
        return HttpResponse.json(created, { status: 201 })
      }),
    )
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByRole('button', { name: '儲存書目' })).toBeDisabled()
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), '  Synthetic title  ')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    await screen.findByRole('heading', { name: 'Synthetic title', level: 2 })
    expect(requests).toEqual([{ external_sources: [], title: 'Synthetic title', authors: null, language: null, publication_year: null, isbn: null, source_url: null, description: null, recommendation_note: null }])
    expect(screen.getByRole('button', { name: '編輯「Book own」' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '編輯「Book other」' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /永久刪除/ })).not.toBeInTheDocument()
  })
  it('shows deletion impact to admins and only deletes after confirmation', async () => {
    mockMe(makeMe({ admin: true }))
    const target = book('shared', { created_by: null })
    let deleted: unknown
    server.use(
      http.get('/api/book-works', () => HttpResponse.json(deleted ? [] : [target])),
      http.get('/api/books/shared/deletion-impact', () => HttpResponse.json({ topic_count: 2, activity_topic_count: 3 })),
      http.delete('/api/books/shared', async ({ request }) => { deleted = await request.json(); return new HttpResponse(null, { status: 204 }) }),
    )
    renderApp(null, { route: '/admin/books' })
    await userEvent.click(await screen.findByRole('button', { name: '永久刪除「Book shared」' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(await within(dialog).findByText('影響 2 份題目書單、3 份活動題目書單。')).toBeInTheDocument()
    expect(deleted).toBeUndefined()
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    expect(deleted).toBeUndefined()
    await userEvent.click(screen.getByRole('button', { name: '永久刪除「Book shared」' }))
    const again = await screen.findByRole('alertdialog')
    await within(again).findByText('影響 2 份題目書單、3 份活動題目書單。')
    await userEvent.click(within(again).getByRole('button', { name: '確認永久刪除' }))
    await waitFor(() => expect(deleted).toEqual({ confirm_title: 'Book shared' }))
    expect(await screen.findByText('共用書庫尚無書目。')).toBeInTheDocument()
  })

  it('searches external books without writing and saves only a selected, confirmed edition', async () => {
    mockMe(teacher)
    let writes = 0
    const books: ReturnType<typeof book>[] = []
    server.use(
      http.get('/api/book-works', () => HttpResponse.json(books)),
      http.get('/api/books/search', () => HttpResponse.json([{ source: 'Google Books', source_id: 'volume-1', title: 'Synthetic edition', authors: 'Sample Author', language: 'en', publication_year: '2025', isbn: null, source_url: 'https://books.google.com/books?id=volume-1', description: null }])),
      http.post('/api/books', async ({ request }) => { writes++; const created = book('new', await request.json() as object); books.push(created); return HttpResponse.json(created, { status: 201 }) }),
    )
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('外部書目搜尋（書名、作者或 ISBN）'), 'Synthetic')
    await userEvent.click(within(dialog).getByRole('button', { name: '搜尋外部書目' }))
    await userEvent.click(await within(dialog).findByRole('button', { name: '選擇此版本' }))
    expect(writes).toBe(0)
    expect(within(dialog).getByLabelText('書名（必填）')).toHaveValue('Synthetic edition')
    expect(within(dialog).getByLabelText('作者（選填；多人以分號分隔）')).toHaveValue('Sample Author')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(writes).toBe(1))
  })

  it('validates Unicode character limits and unsafe URLs without losing input', async () => {
    mockMe(teacher)
    let writes = 0
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.post('/api/books', () => { writes++; return HttpResponse.json(book('new')) }))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    const title = within(dialog).getByLabelText('書名（必填）')
    fireEvent.change(title, { target: { value: '😀'.repeat(256) } })
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('255')
    expect(writes).toBe(0)
    fireEvent.change(title, { target: { value: '😀'.repeat(255) } })
    fireEvent.change(within(dialog).getByLabelText('來源連結（選填）'), { target: { value: 'javascript:alert(1)' } })
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('HTTP')
    expect(title).toHaveValue('😀'.repeat(255))
    expect(writes).toBe(0)
    fireEvent.change(within(dialog).getByLabelText('來源連結（選填）'), { target: { value: '' } })
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(writes).toBe(1))
  })

  it('shows the full details of another teacher’s book without edit permissions', async () => {
    mockMe(teacher)
    server.use(http.get('/api/book-works', () => HttpResponse.json([book('other', { created_by: 'other-teacher', description: 'Synthetic description', recommendation_note: 'Synthetic note' })])))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '查看「Book other」' }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('Synthetic description')).toBeInTheDocument()
    expect(within(dialog).getByText('Synthetic note')).toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: '儲存書目' })).not.toBeInTheDocument()
  })

  it('preserves entered values when enrichment fails and still permits manual saving', async () => {
    mockMe(teacher)
    let saved: unknown
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.get('/api/books/search', () => HttpResponse.json({ error: { code: 'book_source_unavailable', request_id: 'synthetic' } }, { status: 503 })), http.post('/api/books', async ({ request }) => { saved = await request.json(); return HttpResponse.json(book('new'), { status: 201 }) }))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Manual book')
    await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('原填內容已保留')
    expect(within(dialog).getByLabelText('書名（必填）')).toHaveValue('Manual book')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(saved).toMatchObject({ title: 'Manual book', authors: null }))
  })

  it('fills missing values only and warns instead of truncating oversized external fields', async () => {
    mockMe(teacher)
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.get('/api/books/search', () => HttpResponse.json([{ source: 'Google Books', source_id: 'v1', title: 'Different title', authors: 'Original author', language: 'en', publication_year: '2025', isbn: null, source_url: 'javascript:bad', description: 'x'.repeat(5001) }])))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Original title')
    await userEvent.type(within(dialog).getByLabelText('作者（選填；多人以分號分隔）'), 'Original author')
    await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '依書名搜尋補資料' })).toBeEnabled())
    expect(within(dialog).getByLabelText('書名（必填）')).toHaveValue('Original title')
    expect(within(dialog).getByLabelText('作者（選填；多人以分號分隔）')).toHaveValue('Original author')
    expect(within(dialog).getByLabelText('語言（選填）')).toHaveValue('en')
    expect(within(dialog).getByLabelText('簡介（選填）')).toHaveValue('')
    expect(within(dialog).getByLabelText('來源連結（選填）')).toHaveValue('')
    expect(within(dialog).getByRole('status')).toHaveTextContent('部分外部欄位超過上限')
  })

  it('keeps entered data after a failed save and supports retry', async () => {
    mockMe(teacher)
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.post('/api/books', () => HttpResponse.json({ error: { code: 'internal', request_id: 'synthetic' } }, { status: 500 })))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Keep this')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('系統發生錯誤')
    expect(within(dialog).getByLabelText('書名（必填）')).toHaveValue('Keep this')
    server.use(http.post('/api/books', () => HttpResponse.json(book('new'), { status: 201 })))
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('allows only admins to deactivate and keeps the displayed status on failure', async () => {
    mockMe(makeMe({ admin: true }))
    let patches = 0
    server.use(http.get('/api/book-works', () => HttpResponse.json([book('one')])), http.patch('/api/books/one/status', () => { patches++; return HttpResponse.json({ error: { code: 'internal', request_id: 'synthetic' } }, { status: 500 }) }))
    renderApp(null, { route: '/admin/books' })
    await userEvent.click(await screen.findByRole('button', { name: '停用' }))
    expect(patches).toBe(0)
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '停用' }))
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('系統發生錯誤')
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    expect(screen.getAllByText('啟用')).toHaveLength(2)
    expect(patches).toBe(1)
  })

  it.each([null, makeMe(), makeMe({ student: true })])('does not request staff book data for unauthorized users', async (me) => {
    mockMe(me)
    let requests = 0
    server.use(http.get('/api/book-works', () => { requests++; return HttpResponse.json([]) }))
    renderApp(null, { route: '/teacher/books' })
    if (me === null) await screen.findByRole('button', { name: '登入' })
    else if (!me.roles.student) await screen.findByText(/尚未開通/)
    else await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })
    expect(requests).toBe(0)
  })

  it.each([['en', 'Shared books'], ['es', 'Biblioteca compartida']] as const)('renders the book page in %s', async (language, heading) => {
    mockMe(makeMe({ teacher: true }, { language }))
    server.use(http.get('/api/book-works', () => HttpResponse.json([])))
    renderApp(null, { route: '/teacher/books', lang: language })
    expect(await screen.findByRole('heading', { name: heading })).toBeInTheDocument()
  })

  it('clears absent edition metadata when selecting a different external edition', async () => {
    mockMe(teacher)
    let search = 0
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.get('/api/books/search', () => { search++; return HttpResponse.json([{ source: 'Google Books', source_id: `v${search}`, title: `Edition ${search}`, authors: search === 1 ? 'First author' : null, language: null, publication_year: search === 1 ? '2025' : null, isbn: search === 1 ? '9780140449334' : null, source_url: null, description: search === 1 ? 'Old description' : null }]) }))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('外部書目搜尋（書名、作者或 ISBN）'), 'Edition')
    await userEvent.click(within(dialog).getByRole('button', { name: '搜尋外部書目' }))
    await userEvent.click(await within(dialog).findByRole('button', { name: '選擇此版本' }))
    expect(within(dialog).getByLabelText('ISBN（選填）')).toHaveValue('9780140449334')
    await userEvent.click(within(dialog).getByRole('button', { name: '搜尋外部書目' }))
    await userEvent.click(await within(dialog).findByRole('button', { name: '選擇此版本' }))
    expect(within(dialog).getByLabelText('書名（必填）')).toHaveValue('Edition 2')
    expect(within(dialog).getByLabelText('ISBN（選填）')).toHaveValue('')
    expect(within(dialog).getByLabelText('作者（選填；多人以分號分隔）')).toHaveValue('')
    expect(within(dialog).getByLabelText('出版年（選填）')).toHaveValue('')
    expect(within(dialog).getByLabelText('簡介（選填）')).toHaveValue('')
  })

  it('does not mix supplemental metadata with a conflicting known author', async () => {
    mockMe(teacher)
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.get('/api/books/search', () => HttpResponse.json([{ source: 'Google Books', source_id: 'conflict', title: 'Republic', authors: 'Different author', language: 'en', publication_year: '2025', isbn: '9780140449334', source_url: null, description: 'Wrong edition' }])))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Republic')
    await userEvent.type(within(dialog).getByLabelText('作者（選填；多人以分號分隔）'), 'Plato')
    await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    await waitFor(() => expect(within(dialog).getByRole('button', { name: '依書名搜尋補資料' })).toBeEnabled())
    expect(within(dialog).getByLabelText('ISBN（選填）')).toHaveValue('')
    expect(within(dialog).getByLabelText('作者（選填；多人以分號分隔）')).toHaveValue('Plato')
    expect(within(dialog).getByRole('status')).toBeInTheDocument()
  })

  it('hides supplemental search results when the form title changes', async () => {
    mockMe(teacher)
    server.use(http.get('/api/book-works', () => HttpResponse.json([])), http.get('/api/books/search', () => HttpResponse.json([1, 2].map((n) => ({ source: 'Google Books', source_id: `old${n}`, title: 'Original', authors: null, language: null, publication_year: null, isbn: null, source_url: null, description: null })))))
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '新增書目' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), 'Original')
    await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    expect(await within(dialog).findAllByRole('button', { name: '選擇此版本' })).toHaveLength(2)
    await userEvent.type(within(dialog).getByLabelText('書名（必填）'), ' changed')
    expect(within(dialog).queryByRole('button', { name: '選擇此版本' })).not.toBeInTheDocument()
  })

  it.each(['replace', 'fill', 'conflict', 'unchanged'] as const)('saves external provenance for %s while preserving manual edits', async (mode) => {
    mockMe(teacher)
    const original = [{ source: 'Google Books', source_id: 'original' }]
    let saved: unknown
    server.use(
      http.get('/api/book-works', () => HttpResponse.json([book('own', { authors: mode === 'conflict' || mode === 'unchanged' ? 'Plato' : null, external_sources: original })])),
      http.get('/api/books/search', () => HttpResponse.json([{ source: 'Open Library', source_id: '/books/OL123M', title: 'Book own', authors: mode === 'conflict' ? 'Other' : 'Plato', language: null, publication_year: null, isbn: null, source_url: null, description: null }])),
      http.put('/api/books/own', async ({ request }) => { saved = await request.json(); return HttpResponse.json(book('own')) }),
    )
    renderApp(null, { route: '/teacher/books' })
    await userEvent.click(await screen.findByRole('button', { name: '編輯「Book own」' }))
    const dialog = await screen.findByRole('dialog')
    if (mode === 'replace') {
      await userEvent.type(within(dialog).getByLabelText('外部書目搜尋（書名、作者或 ISBN）'), 'Book own')
      await userEvent.click(within(dialog).getByRole('button', { name: '搜尋外部書目' }))
    } else await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
    if (mode === 'replace') await userEvent.click(await within(dialog).findByRole('button', { name: '選擇此版本' }))
    else await waitFor(() => expect(within(dialog).getByRole('button', { name: '依書名搜尋補資料' })).toBeEnabled())
    if (mode === 'fill') {
      await userEvent.click(within(dialog).getByRole('button', { name: '依書名搜尋補資料' }))
      await waitFor(() => expect(within(dialog).getByRole('button', { name: '依書名搜尋補資料' })).toBeEnabled())
    }
    await userEvent.type(within(dialog).getByLabelText('教師推薦說明（選填）'), 'Manual note')
    await userEvent.click(within(dialog).getByRole('button', { name: '儲存書目' }))
    const selected = { source: 'Open Library', source_id: '/books/OL123M' }
    await waitFor(() => expect(saved).toMatchObject({ recommendation_note: 'Manual note', external_sources: mode === 'replace' ? [selected] : mode === 'fill' ? [...original, selected] : original }))
  })

})
