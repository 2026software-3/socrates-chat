import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { SystemAiOverview } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const providers = [
  { id: 'openai', name: 'OpenAI' },
  { id: 'anthropic', name: 'Anthropic' },
]
const overview = (over: Partial<SystemAiOverview> = {}): SystemAiOverview => ({
  source: 'host_file',
  routing_enabled: true,
  encryption_configured: true,
  setting: null,
  host_model: 'gpt-4o-mini',
  keys: [],
  providers,
  ...over,
})
const key = {
  id: 'k-1',
  provider: 'openai',
  provider_name: 'OpenAI',
  mask: '…aaaa',
  status: 'ok' as const,
  status_at: null,
  created_at: '2026-11-18T01:00:00Z',
  requests_30d: 1200,
  input_tokens_30d: 3000,
  output_tokens_30d: 400,
  unreported_requests_30d: 0,
}

const noUsage = { range: '7d', totals: { requests: 0, input_tokens: 0, output_tokens: 0, unreported_requests: 0 }, items: [] }

async function open(data: SystemAiOverview, usage: (range: string) => Record<string, unknown> = () => noUsage) {
  server.use(
    http.get('/api/admin/system-ai', () => HttpResponse.json(data)),
    http.get('/api/admin/system-ai/usage', ({ request }) => HttpResponse.json(usage(new URL(request.url).searchParams.get('range') ?? ''))),
  )
  mockMe(makeMe({ admin: true }))
  renderApp(null, { route: '/admin/system-ai' })
  await screen.findByRole('heading', { name: '系統 AI' })
}

const goTo = async (name: string) => userEvent.setup().click(await screen.findByRole('tab', { name }))

describe('SystemAiPage', () => {
  it('says the host key list is in use and warns when the feature is not enabled', async () => {
    await open(overview({ encryption_configured: false }))
    expect(await screen.findByText(/尚未設定系統 AI（預設模型 gpt-4o-mini）/)).toBeInTheDocument()
    expect(screen.getByText(/後端尚未啟用教師 key 功能/)).toBeInTheDocument()
  })

  it('splits the page into model, keys and usage tabs and shows one at a time', async () => {
    await open(overview({ keys: [key] }))
    const tabs = await screen.findAllByRole('tab')
    expect(tabs.map((t) => t.textContent)).toEqual(['供應商與模型', '系統 key', '用量'])
    expect(tabs[0]).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('button', { name: '驗證並儲存' })).toBeInTheDocument()
    expect(screen.queryByRole('list', { name: '系統 key' })).not.toBeInTheDocument()
    await goTo('系統 key')
    expect(await screen.findByRole('list', { name: '系統 key' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '新增' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '驗證並儲存' })).not.toBeInTheDocument()
    await goTo('用量')
    expect(await screen.findByLabelText('期間')).toBeInTheDocument()
    expect(screen.queryByRole('list', { name: '系統 key' })).not.toBeInTheDocument()
  })

  it('lists keys with their usage but never a full key', async () => {
    await open(overview({ source: 'database', setting: { provider: 'openai', provider_name: 'OpenAI', model: 'gpt-x' }, keys: [key] }))
    await goTo('系統 key')
    const row = (await screen.findByRole('list', { name: '系統 key' })).querySelector('li')!
    expect(row).toHaveTextContent('…aaaa')
    expect(row).toHaveTextContent('近 30 天 1,200 次')
    expect(screen.getByText(/目前使用資料庫設定：OpenAI／gpt-x/)).toBeInTheDocument()
  })

  it('adds a key and shows the server error when it already exists', async () => {
    const bodies: unknown[] = []
    server.use(
      http.post('/api/admin/system-ai/keys', async ({ request }) => {
        bodies.push(await request.json())
        return bodies.length === 1 ? HttpResponse.json(key, { status: 201 }) : apiError(409, 'key_already_exists', '這把 key 已經存在')
      }),
    )
    await open(overview())
    await goTo('系統 key')
    const user = userEvent.setup()
    await user.selectOptions(await screen.findByLabelText('供應商', { selector: '#sys-add-provider' }), 'openai')
    await user.type(screen.getByLabelText('API key', { selector: '#sys-add-key' }), ' sk-synthetic-1 ')
    await user.click(screen.getByRole('button', { name: '新增' }))
    expect(await screen.findByText('已新增系統 key。')).toBeInTheDocument()
    expect(bodies[0]).toEqual({ provider: 'openai', key: 'sk-synthetic-1' })
    await user.type(screen.getByLabelText('API key', { selector: '#sys-add-key' }), 'sk-synthetic-1')
    await user.click(screen.getByRole('button', { name: '新增' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('已經存在')
  })

  it('verifies and saves the model, and clears it', async () => {
    const puts: unknown[] = []
    let deleted = false
    let current = overview({ keys: [key] })
    server.use(
      http.put('/api/admin/system-ai/model', async ({ request }) => {
        puts.push(await request.json())
        current = overview({ source: 'database', keys: [key], setting: { provider: 'openai', provider_name: 'OpenAI', model: 'gpt-x' } })
        return HttpResponse.json(current)
      }),
      http.delete('/api/admin/system-ai/model', () => {
        deleted = true
        return HttpResponse.json(overview({ keys: [key] }))
      }),
    )
    await open(current)
    server.use(http.get('/api/admin/system-ai', () => HttpResponse.json(current)))
    const user = userEvent.setup()
    await user.selectOptions(await screen.findByLabelText('供應商', { selector: '#sys-provider' }), 'openai')
    await user.type(screen.getByLabelText('模型 ID', { selector: '#sys-model' }), 'gpt-x')
    await user.click(screen.getByRole('button', { name: '驗證並儲存' }))
    await waitFor(() => expect(puts).toEqual([{ provider: 'openai', model: 'gpt-x' }]))
    expect(await screen.findByText('已儲存系統模型。')).toBeInTheDocument()
    await user.click(await screen.findByRole('button', { name: /清除設定/ }))
    await waitFor(() => expect(deleted).toBe(true))
  })

  it('shows the system usage by provider and model with a range selector', async () => {
    const ranges: string[] = []
    await open(overview({ keys: [key] }), (range) => {
      ranges.push(range)
      return {
        range,
        totals: { requests: 3, input_tokens: 11, output_tokens: 6, unreported_requests: 0 },
        items: [{ provider: 'openai', model: 'gpt-a', requests: 3, input_tokens: 11, output_tokens: 6, unreported_requests: 0 }],
      }
    })
    await goTo('用量')
    expect(await screen.findByText('OpenAI / gpt-a')).toBeInTheDocument()
    expect(ranges[0]).toBe('7d')
    await userEvent.setup().selectOptions(screen.getByLabelText('期間'), '30d')
    await waitFor(() => expect(ranges).toContain('30d'))
  })

  it('suggests models from the system keys and still accepts any typed model', async () => {
    const asked: string[] = []
    server.use(
      http.get('/api/admin/system-ai/models', ({ request }) => {
        asked.push(new URL(request.url).searchParams.get('provider') ?? '')
        return HttpResponse.json({ provider: 'openai', status: 'ok', models: ['gpt-a', 'gpt-b'] })
      }),
    )
    await open(overview({ keys: [key] }))
    const user = userEvent.setup()
    await user.selectOptions(await screen.findByLabelText('供應商', { selector: '#sys-provider' }), 'openai')
    await waitFor(() => expect(asked).toEqual(['openai']))
    const input = screen.getByLabelText('模型 ID', { selector: '#sys-model' })
    await waitFor(() => expect(input).toHaveAttribute('list'))
    expect(document.querySelectorAll(`datalist#${input.getAttribute('list')} option`)).toHaveLength(2)
    await user.type(input, 'my-own-model')
    expect(input).toHaveValue('my-own-model')
  })

  it('tests, replaces and deletes a key', async () => {
    const seen: string[] = []
    server.use(
      http.post('/api/admin/system-ai/keys/k-1/test', () => HttpResponse.json({ result: 'usable' })),
      http.put('/api/admin/system-ai/keys/k-1', async ({ request }) => {
        seen.push(JSON.stringify(await request.json()))
        return HttpResponse.json(key)
      }),
      http.delete('/api/admin/system-ai/keys/k-1', () => {
        seen.push('delete')
        return new HttpResponse(null, { status: 204 })
      }),
    )
    await open(overview({ keys: [key] }))
    await goTo('系統 key')
    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: '測試 OpenAI …aaaa 的 key' }))
    expect(await screen.findByText('可用')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '更換 OpenAI …aaaa 的 key' }))
    await user.type(await screen.findByLabelText('API key', { selector: '#sys-replace-key' }), 'sk-synthetic-2')
    await user.click(screen.getByRole('button', { name: '更換' }))
    await waitFor(() => expect(seen).toEqual(['{"key":"sk-synthetic-2"}']))
    await user.click(await screen.findByRole('button', { name: '刪除 OpenAI …aaaa 的 key' }))
    await user.click(await screen.findByRole('button', { name: '確認刪除' }))
    await waitFor(() => expect(seen.at(-1)).toBe('delete'))
  })
})
