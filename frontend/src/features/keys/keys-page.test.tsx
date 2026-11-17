import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { KeysPage } from '@/features/keys/keys-page'
import type { TeacherKey } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const SECRET = 'sk-synthetic-FRONT-1111-2222-3333-aaaa'
const providers = [
  { id: 'openai', name: 'OpenAI' },
  { id: 'anthropic', name: 'Anthropic' },
]
const key = (over: Partial<TeacherKey> = {}): TeacherKey => ({
  id: 'k-1',
  provider: 'openai',
  mask: '…aaaa',
  status: 'ok',
  status_at: null,
  created_at: '2026-11-18T01:00:00Z',
  updated_at: '2026-11-18T01:00:00Z',
  ...over,
})

type Call = { method: string; url: string; body?: unknown }

/** 有狀態的假後端：記錄寫入類請求。 */
function mockKeys(initial: TeacherKey[]) {
  const keys = [...initial]
  const calls: Call[] = []
  server.use(
    http.get('/api/teacher/ai-providers', () => HttpResponse.json(providers)),
    http.get('/api/teacher/keys', () => HttpResponse.json(keys)),
    http.post('/api/teacher/keys', async ({ request }) => {
      const body = (await request.json()) as { provider: string; key: string }
      calls.push({ method: 'POST', url: '/api/teacher/keys', body })
      keys.push(key({ id: `k-${keys.length + 1}`, provider: body.provider, mask: `…${body.key.slice(-4)}` }))
      return HttpResponse.json(keys[keys.length - 1], { status: 201 })
    }),
    http.put('/api/teacher/keys/:id', async ({ request, params }) => {
      calls.push({ method: 'PUT', url: `/api/teacher/keys/${String(params.id)}`, body: await request.json() })
      return HttpResponse.json(keys[0])
    }),
    http.delete('/api/teacher/keys/:id', ({ params }) => {
      calls.push({ method: 'DELETE', url: `/api/teacher/keys/${String(params.id)}` })
      const i = keys.findIndex((k) => k.id === params.id)
      if (i >= 0) keys.splice(i, 1)
      return new HttpResponse(null, { status: 204 })
    }),
  )
  return { keys, calls }
}

const render = () => renderApp(<KeysPage />)

describe('KeysPage', () => {
  it('lists each key with provider, masked identifier and status, never a full key', async () => {
    mockKeys([key(), key({ id: 'k-2', provider: 'anthropic', mask: '…bbbb', status: 'quota_exhausted', status_at: '2026-11-18T02:00:00Z' })])
    render()
    const list = await screen.findByRole('list', { name: 'API key 清單' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(2)
    expect(items[0]).toHaveTextContent('OpenAI')
    expect(items[0]).toHaveTextContent('…aaaa')
    expect(items[0]).toHaveTextContent('可使用')
    expect(items[1]).toHaveTextContent('Anthropic')
    expect(items[1]).toHaveTextContent('額度用盡')
    expect(items[1]).toHaveTextContent(/自 /)
    expect(document.body.textContent).not.toContain('sk-')
  })

  it('shows the reason category of every unusable status', async () => {
    mockKeys([
      key({ id: 'a', status: 'invalid', status_at: '2026-11-18T02:00:00Z' }),
      key({ id: 'b', status: 'rate_limited', status_at: '2026-11-18T02:00:00Z' }),
    ])
    render()
    expect(await screen.findByText('失效或已被撤銷')).toBeInTheDocument()
    expect(screen.getByText('受頻率限制')).toBeInTheDocument()
  })

  it('shows loading, empty and error states', async () => {
    mockKeys([])
    render()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(await screen.findByText('還沒有 key。請先在下方新增一把。')).toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    server.use(
      http.get('/api/teacher/ai-providers', () => HttpResponse.json(providers)),
      http.get('/api/teacher/keys', () => apiError(500, 'internal', 'req-5')),
    )
    render()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByText('錯誤代碼：req-5')).toBeInTheDocument()
    mockKeys([key()])
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('…aaaa')).toBeInTheDocument()
  })

  it('adds a key: provider from the API first, the field is cleared and the key is not shown again', async () => {
    const { calls } = mockKeys([])
    render()
    const select = await screen.findByLabelText('供應商')
    await screen.findByRole('option', { name: 'Anthropic' })
    const field = screen.getByLabelText('API key')
    expect(field).toHaveAttribute('type', 'password')
    const submit = screen.getByRole('button', { name: '新增' })
    expect(submit).toBeDisabled()
    await userEvent.selectOptions(select, 'anthropic')
    await userEvent.type(field, SECRET)
    await userEvent.click(submit)
    expect(await screen.findByText('已新增 Anthropic 的 key。')).toBeInTheDocument()
    expect(calls).toEqual([{ method: 'POST', url: '/api/teacher/keys', body: { provider: 'anthropic', key: SECRET } }])
    expect(field).toHaveValue('')
    expect(document.body.textContent).not.toContain(SECRET)
    expect(await screen.findByText('…aaaa')).toBeInTheDocument()
  })

  it('says "this key already exists" for a duplicate', async () => {
    mockKeys([])
    server.use(http.post('/api/teacher/keys', () => apiError(409, 'key_already_exists')))
    render()
    await screen.findByRole('option', { name: 'OpenAI' })
    await userEvent.selectOptions(screen.getByLabelText('供應商'), 'openai')
    await userEvent.type(screen.getByLabelText('API key'), SECRET)
    await userEvent.click(screen.getByRole('button', { name: '新增' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('這把 key 已經存在。')
  })

  it('replaces a key with the provider fixed', async () => {
    const { calls } = mockKeys([key()])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '更換 OpenAI …aaaa 的 key' }))
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByLabelText('供應商')).toHaveAttribute('readonly')
    expect(within(dialog).getByLabelText('供應商')).toHaveValue('OpenAI')
    expect(within(dialog).getByText(/供應商不能更換/)).toBeInTheDocument()
    await userEvent.type(within(dialog).getByLabelText('API key'), SECRET)
    await userEvent.click(within(dialog).getByRole('button', { name: '更換' }))
    expect(await screen.findByText('已更換 OpenAI 的 key。')).toBeInTheDocument()
    expect(calls).toEqual([{ method: 'PUT', url: '/api/teacher/keys/k-1', body: { key: SECRET } }])
    expect(document.body.textContent).not.toContain(SECRET)
  })

  it('confirms before deleting and explains that usage is deleted too', async () => {
    const { calls } = mockKeys([key()])
    render()
    await userEvent.click(await screen.findByRole('button', { name: '刪除 OpenAI …aaaa 的 key' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText(/用量紀錄都會被刪除/)).toBeInTheDocument()
    expect(within(dialog).getByText(/統計會扣除/)).toBeInTheDocument()
    expect(calls).toEqual([])
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    expect(calls).toEqual([])
    await userEvent.click(screen.getByRole('button', { name: '刪除 OpenAI …aaaa 的 key' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '確認刪除' }))
    await waitFor(() => expect(calls).toEqual([{ method: 'DELETE', url: '/api/teacher/keys/k-1' }]))
    expect(await screen.findByText('還沒有 key。請先在下方新增一把。')).toBeInTheDocument()
  })

  it('tests a key with the no-cost explanation and shows the result', async () => {
    mockKeys([key()])
    server.use(http.post('/api/teacher/keys/:id/test', () => HttpResponse.json({ result: 'usable' })))
    render()
    expect(await screen.findByText('此測試只驗證 key，不送出對話內容，通常不產生費用。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '測試 OpenAI …aaaa 的 key' }))
    expect(await screen.findByText('可用')).toBeInTheDocument()
    server.use(http.post('/api/teacher/keys/:id/test', () => HttpResponse.json({ result: 'connection_failed' })))
    await userEvent.click(screen.getByRole('button', { name: '測試 OpenAI …aaaa 的 key' }))
    expect(await screen.findByText('連線失敗')).toBeInTheDocument()
  })

  it('shows the rate limit message when testing too often', async () => {
    mockKeys([key()])
    server.use(http.post('/api/teacher/keys/:id/test', () => apiError(429, 'key_test_rate_limited')))
    render()
    await userEvent.click(await screen.findByRole('button', { name: '測試 OpenAI …aaaa 的 key' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('測試太頻繁，請稍後再試。')
  })

  it('shows the usage of a key without money and with "not reported" for missing token counts', async () => {
    mockKeys([key()])
    const ranges: string[] = []
    server.use(
      http.get('/api/teacher/keys/:id/usage', ({ request }) => {
        const range = new URL(request.url).searchParams.get('range') ?? ''
        ranges.push(range)
        return HttpResponse.json({
          range,
          totals: { requests: 5, input_tokens: 1200, output_tokens: 300, unreported_requests: 2 },
          items: [
            { provider: 'openai', model: 'm-a', requests: 3, input_tokens: 1200, output_tokens: 300, unreported_requests: 0 },
            { provider: 'openai', model: 'm-b', requests: 2, input_tokens: 0, output_tokens: 0, unreported_requests: 2 },
          ],
        })
      }),
    )
    render()
    await userEvent.click(await screen.findByRole('button', { name: '查看 OpenAI …aaaa 的用量' }))
    const table = await screen.findByRole('table')
    expect(within(table).getByRole('row', { name: /OpenAI \/ m-a/ })).toHaveTextContent('1,200')
    expect(within(table).getByRole('row', { name: /OpenAI \/ m-b/ })).toHaveTextContent('供應商未回報')
    expect(within(table).getByRole('row', { name: /合計/ })).toHaveTextContent('5')
    expect(screen.getByText(/不顯示金額/)).toBeInTheDocument()
    expect(document.body.textContent).not.toMatch(/\$|NT\$/)
    expect(ranges).toEqual(['7d'])
    await userEvent.selectOptions(screen.getByLabelText('期間'), 'today')
    await waitFor(() => expect(ranges).toEqual(['7d', 'today']))
  })

  it('shows an empty state when a key has no usage', async () => {
    mockKeys([key()])
    server.use(
      http.get('/api/teacher/keys/:id/usage', () =>
        HttpResponse.json({ range: '7d', totals: { requests: 0, input_tokens: 0, output_tokens: 0, unreported_requests: 0 }, items: [] }),
      ),
    )
    render()
    await userEvent.click(await screen.findByRole('button', { name: '查看 OpenAI …aaaa 的用量' }))
    expect(await screen.findByText('這段期間沒有用量。')).toBeInTheDocument()
  })

  it('shows a usage error with retry', async () => {
    mockKeys([key()])
    server.use(http.get('/api/teacher/keys/:id/usage', () => apiError(500, 'internal', 'req-u')))
    render()
    await userEvent.click(await screen.findByRole('button', { name: '查看 OpenAI …aaaa 的用量' }))
    expect(await screen.findByText('錯誤代碼：req-u')).toBeInTheDocument()
  })
})

describe('KeysPage access', () => {
  it('has a nav entry and a page for teachers', async () => {
    mockMe(makeMe({ teacher: true }))
    mockKeys([])
    renderApp(null, { route: '/teacher/keys' })
    expect(await screen.findByRole('heading', { name: 'API key 管理' })).toBeInTheDocument()
    expect(screen.getAllByRole('link', { name: 'API key' }).length).toBeGreaterThan(0)
  })

  it('shows students neither the entry nor the page', async () => {
    mockMe(makeMe({ student: true }))
    server.use(http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })))
    renderApp(null, { route: '/' })
    await screen.findByRole('heading', { name: '選擇題目' })
    expect(screen.queryByRole('link', { name: 'API key' })).not.toBeInTheDocument()
  })

  it('shows the forbidden page to a student who types the URL', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: '/teacher/keys' })
    expect(await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'API key 管理' })).not.toBeInTheDocument()
  })
})
