import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import type { Classroom, ClassroomAiSettings, TeacherKey } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const CID = '11111111-1111-4111-8111-111111111111'
const ROUTE = `/teacher/classrooms/${CID}/ai`
const providers = [
  { id: 'openai', name: 'OpenAI' },
  { id: 'anthropic', name: 'Anthropic' },
  { id: 'deepseek', name: 'DeepSeek' },
]
const classroom = (over: Partial<Classroom> = {}): Classroom => ({
  id: CID,
  name: '合成教室',
  year_label: null,
  description: null,
  is_archived: false,
  created_at: '2026-11-18T01:00:00Z',
  updated_at: '2026-11-18T01:00:00Z',
  ai_model_missing: false,
  ...over,
})
/** 背景工作（GET /api/ai-jobs/{id}）：契約規定的欄位齊全，`over` 可帶 `error_code` 等額外欄位。 */
const aiJob = (id: string, status: 'pending' | 'ready' | 'failed', over: Record<string, unknown> = {}) => ({
  id, kind: 'classroom_ai_settings', status, label_key: 'jobs.classroom_ai_settings', scope: { classroom_id: CID },
  started_at: '2026-11-18T01:00:00Z', finished_at: status === 'pending' ? null : '2026-11-18T01:00:05Z', retryable: false, ...over,
})
const key = (provider: string): TeacherKey => ({
  id: `k-${provider}`,
  provider,
  mask: '…aaaa',
  status: 'ok',
  status_at: null,
  created_at: '2026-11-18T01:00:00Z',
  updated_at: '2026-11-18T01:00:00Z',
})
const emptyUsage = { range: '7d', totals: { requests: 0, input_tokens: 0, output_tokens: 0, unreported_requests: 0 }, items: [] }

function mockSettings(settings: ClassroomAiSettings, opts: { keys?: string[]; archived?: boolean } = {}) {
  const puts: unknown[] = []
  server.use(
    http.get('/api/classrooms', () => HttpResponse.json([classroom({ is_archived: opts.archived ?? false })])),
    http.get('/api/teacher/ai-providers', () => HttpResponse.json(providers)),
    http.get('/api/teacher/keys', () => HttpResponse.json((opts.keys ?? ['openai', 'anthropic', 'deepseek']).map(key))),
    http.get(`/api/classrooms/${CID}/ai-settings`, () => HttpResponse.json(settings)),
    http.get(`/api/classrooms/${CID}/ai-usage`, () => HttpResponse.json(emptyUsage)),
    http.put(`/api/classrooms/${CID}/ai-settings`, async ({ request }) => {
      puts.push(await request.json())
      return HttpResponse.json({ job_id: 'j-default', status: 'pending' }, { status: 202 })
    }),
    http.get('/api/ai-jobs/j-default', () => HttpResponse.json(aiJob('j-default', 'ready'))),
  )
  return puts
}

const transitional: ClassroomAiSettings = { transitional: true, primary: null, fallbacks: [], confirmed_providers: [] }
const configured: ClassroomAiSettings = {
  transitional: false,
  primary: { provider: 'openai', model: 'gpt-main' },
  fallbacks: [{ provider: 'anthropic', model: 'claude-fb' }],
  confirmed_providers: ['openai', 'anthropic'],
}

const open = async () => {
  mockMe(makeMe({ teacher: true }))
  renderApp(null, { route: ROUTE })
  await screen.findByRole('heading', { name: '教室 AI 設定' })
}

describe('ClassroomAiSettingsPage', () => {
  it('explains that a transitional classroom still uses the system key', async () => {
    mockSettings(transitional)
    await open()
    expect(await screen.findByText(/目前沿用系統 key，請設定教師 key 與模型/)).toBeInTheDocument()
    expect(screen.queryByText('這間教室還沒有設定模型')).not.toBeInTheDocument()
  })

  it('warns that students cannot use AI when a non-transitional classroom has no model', async () => {
    mockSettings({ transitional: false, primary: null, fallbacks: [], confirmed_providers: [] })
    await open()
    expect(await screen.findByRole('alert')).toHaveTextContent('這間教室還沒有設定模型')
  })

  it('shows the current primary and fallbacks, and the model disclaimer', async () => {
    mockSettings(configured)
    await open()
    expect(await screen.findByDisplayValue('gpt-main')).toBeInTheDocument()
    expect(screen.getByDisplayValue('claude-fb')).toBeInTheDocument()
    expect(screen.getByText(/系統不保證自由輸入的模型的表現/)).toBeInTheDocument()
    expect(screen.queryByText(/目前沿用系統 key/)).not.toBeInTheDocument()
    expect(screen.queryByText('這間教室還沒有設定模型')).not.toBeInTheDocument()
  })

  it('suggests the provider’s models in the model field but still accepts any typed ID', async () => {
    const puts = mockSettings(transitional)
    const asked: string[] = []
    server.use(
      http.get('/api/teacher/keys/models', ({ request }) => {
        const provider = new URL(request.url).searchParams.get('provider') ?? ''
        asked.push(provider)
        return HttpResponse.json({ provider, status: 'ok', models: provider === 'openai' ? ['gpt-4o', 'gpt-4o-mini'] : ['claude-x'] })
      }),
    )
    await open()
    // 還沒選供應商：不取清單、沒有建議
    const model = await screen.findByLabelText('模型 ID')
    expect(asked).toEqual([])
    expect(model).not.toHaveAttribute('list')

    await userEvent.selectOptions(screen.getByLabelText('供應商'), 'openai')
    expect(await screen.findByText(/可從供應商目前列出的模型中選擇/)).toBeInTheDocument()
    expect(asked).toEqual(['openai'])
    const listId = model.getAttribute('list') ?? ''
    const options = Array.from(document.querySelectorAll(`datalist#${listId} option`)).map((o) => o.getAttribute('value'))
    expect(options).toEqual(['gpt-4o', 'gpt-4o-mini'])

    // 清單以外的模型 ID 照樣能輸入並儲存；儲存內容與沒有清單時相同
    await userEvent.type(model, 'my-new-model')
    await userEvent.click(screen.getByRole('checkbox', { name: /我已確認符合我的授課與隱私需求/ }))
    await userEvent.click(screen.getByRole('button', { name: '驗證並儲存' }))
    expect(await screen.findByText('已儲存。')).toBeInTheDocument()
    expect(puts).toEqual([{ provider: 'openai', model: 'my-new-model', fallbacks: [], data_flow_confirmed: true }])
  })

  it('falls back to a plain text field when the list is missing or cannot be fetched', async () => {
    mockSettings(transitional)
    server.use(
      http.get('/api/teacher/keys/models', ({ request }) => {
        const provider = new URL(request.url).searchParams.get('provider') ?? ''
        return provider === 'anthropic'
          ? HttpResponse.json({ provider, status: 'unavailable', models: [] })
          : provider === 'deepseek'
            ? apiError(500, 'internal', 'req-m')
            : HttpResponse.json({ provider, status: 'no_key', models: [] })
      }),
    )
    await open()
    const model = await screen.findByLabelText('模型 ID')
    // 取不到清單：說明並可直接輸入
    await userEvent.selectOptions(screen.getByLabelText('供應商'), 'anthropic')
    expect(await screen.findByText(/暫時無法取得這個供應商的模型清單，請直接輸入模型 ID/)).toBeInTheDocument()
    expect(model).not.toHaveAttribute('list')
    await userEvent.type(model, 'claude-typed')
    expect(model).toHaveValue('claude-typed')
    // 沒有 key：不顯示額外說明，仍可輸入
    await userEvent.selectOptions(screen.getByLabelText('供應商'), 'openai')
    await waitFor(() => expect(screen.queryByText(/暫時無法取得這個供應商的模型清單/)).not.toBeInTheDocument())
    expect(model).not.toHaveAttribute('list')
    // 請求失敗：整頁不受影響，仍可輸入
    await userEvent.selectOptions(screen.getByLabelText('供應商'), 'deepseek')
    expect(screen.getByRole('heading', { name: '教室 AI 設定' })).toBeInTheDocument()
    expect(model).not.toHaveAttribute('list')
    expect(model).toBeEnabled()
  })

  it('requires the data-flow confirmation the first time a provider is chosen, then saves', async () => {
    const puts = mockSettings(transitional)
    await open()
    await userEvent.selectOptions(await screen.findByLabelText('供應商'), 'openai')
    await userEvent.type(screen.getByLabelText('模型 ID'), 'gpt-main')
    const save = screen.getByRole('button', { name: '驗證並儲存' })
    expect(save).toBeDisabled()
    const confirm = screen.getByRole('checkbox', { name: /學生的對話內容會送往 OpenAI，我已確認符合我的授課與隱私需求/ })
    expect(screen.getByText(/可能產生少量費用/)).toBeInTheDocument()
    await userEvent.click(confirm)
    expect(save).toBeEnabled()
    await userEvent.click(save)
    expect(await screen.findByText('已儲存。')).toBeInTheDocument()
    expect(puts).toEqual([{ provider: 'openai', model: 'gpt-main', fallbacks: [], data_flow_confirmed: true }])
  })

  it('does not ask again for a provider that was already confirmed', async () => {
    const puts = mockSettings(configured)
    await open()
    await screen.findByDisplayValue('gpt-main')
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
    await userEvent.clear(screen.getByDisplayValue('gpt-main'))
    await userEvent.type(screen.getByLabelText('模型 ID', { selector: '#primary-model' }), 'gpt-other')
    await userEvent.click(screen.getByRole('button', { name: '驗證並儲存' }))
    await screen.findByText('已儲存。')
    expect(puts).toEqual([
      { provider: 'openai', model: 'gpt-other', fallbacks: [{ provider: 'anthropic', model: 'claude-fb' }], data_flow_confirmed: true },
    ])
  })

  it('asks for confirmation again when a new provider joins as a fallback', async () => {
    mockSettings(configured)
    await open()
    await screen.findByDisplayValue('gpt-main')
    await userEvent.click(screen.getByRole('button', { name: '新增備援' }))
    await userEvent.selectOptions(screen.getByLabelText('供應商', { selector: '#fallback-2-provider' }), 'deepseek')
    await userEvent.type(screen.getByLabelText('模型 ID', { selector: '#fallback-2-model' }), 'ds-chat')
    expect(screen.getByRole('button', { name: '驗證並儲存' })).toBeDisabled()
    expect(screen.getByRole('checkbox', { name: /送往 DeepSeek/ })).toBeInTheDocument()
  })

  it('adds, orders and removes at most three fallbacks', async () => {
    const puts = mockSettings({ ...configured, fallbacks: [], confirmed_providers: ['openai', 'anthropic', 'deepseek'] })
    await open()
    await screen.findByDisplayValue('gpt-main')
    for (const [i, [provider, model]] of [
      ['anthropic', 'fb-one'],
      ['deepseek', 'fb-two'],
      ['openai', 'fb-three'],
    ].entries()) {
      await userEvent.click(screen.getByRole('button', { name: '新增備援' }))
      await userEvent.selectOptions(screen.getByLabelText('供應商', { selector: `#fallback-${i + 1}-provider` }), provider)
      await userEvent.type(screen.getByLabelText('模型 ID', { selector: `#fallback-${i + 1}-model` }), model)
    }
    expect(screen.queryByRole('button', { name: '新增備援' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '上移備援 1' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '下移備援 3' })).toBeDisabled()
    await userEvent.click(screen.getByRole('button', { name: '上移備援 2' }))
    await userEvent.click(screen.getByRole('button', { name: '移除備援 3' }))
    await userEvent.click(screen.getByRole('button', { name: '驗證並儲存' }))
    await screen.findByText('已儲存。')
    expect(puts).toEqual([
      {
        provider: 'openai',
        model: 'gpt-main',
        fallbacks: [
          { provider: 'deepseek', model: 'fb-two' },
          { provider: 'anthropic', model: 'fb-one' },
        ],
        data_flow_confirmed: true,
      },
    ])
  })

  it('warns about a missing key and links to key management instead of saving', async () => {
    mockSettings(transitional, { keys: [] })
    await open()
    await userEvent.selectOptions(await screen.findByLabelText('供應商'), 'openai')
    await userEvent.type(screen.getByLabelText('模型 ID'), 'gpt-main')
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('你還沒有 OpenAI 的 key。')
    expect(within(alert).getByRole('link', { name: '前往 key 管理' })).toHaveAttribute('href', '/teacher/keys')
    expect(screen.getByRole('button', { name: '驗證並儲存' })).toBeDisabled()
  })

  it('verifies in the background: waits for the job, then reloads and shows it saved (S-20)', async () => {
    let polls = 0
    let reloads = 0
    mockSettings(configured)
    server.use(
      http.get(`/api/classrooms/${CID}/ai-settings`, () => {
        reloads += 1
        return HttpResponse.json(configured)
      }),
      http.put(`/api/classrooms/${CID}/ai-settings`, () => HttpResponse.json({ job_id: 'jv', status: 'pending' }, { status: 202 })),
      http.get('/api/ai-jobs/jv', () => HttpResponse.json(aiJob('jv', polls++ < 1 ? 'pending' : 'ready'))),
    )
    await open()
    await screen.findByDisplayValue('gpt-main')
    const before = reloads
    await userEvent.click(screen.getByRole('button', { name: '驗證並儲存' }))
    expect(await screen.findByRole('button', { name: '驗證中…' })).toBeDisabled()
    expect(await screen.findByText(/可離開此頁/)).toBeInTheDocument()
    expect(await screen.findByText('已儲存。', {}, { timeout: 8000 })).toBeInTheDocument()
    expect(reloads).toBeGreaterThan(before)
  }, 15_000)

  it('translates verification failures and points at the failing model', async () => {
    mockSettings(configured)
    server.use(
      http.put(`/api/classrooms/${CID}/ai-settings`, () => HttpResponse.json({ job_id: 'jf', status: 'pending' }, { status: 202 })),
      http.get('/api/ai-jobs/jf', () =>
        HttpResponse.json(aiJob('jf', 'failed', { error_code: 'model_structured_output_unsupported', error_target: 'fallback_1' })),
      ),
    )
    await open()
    await screen.findByDisplayValue('gpt-main')
    await userEvent.click(screen.getByRole('button', { name: '驗證並儲存' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('備援 1：這個模型不支援結構化輸出。')
    expect(screen.queryByText('已儲存。')).not.toBeInTheDocument()
  })

  it.each([
    ['model_not_found', '供應商找不到這個模型，請確認模型 ID。'],
    ['model_auth_failed', '這個模型無法用你的 key 驗證，請確認 key 與權限。'],
    ['key_test_rate_limited', '測試太頻繁，請稍後再試。'],
  ])('translates the %s error', async (code, text) => {
    mockSettings(configured)
    server.use(http.put(`/api/classrooms/${CID}/ai-settings`, () => apiError(code === 'key_test_rate_limited' ? 429 : 400, code)))
    await open()
    await screen.findByDisplayValue('gpt-main')
    await userEvent.click(screen.getByRole('button', { name: '驗證並儲存' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(text)
  })

  it('does not let an archived classroom be changed', async () => {
    mockSettings(configured, { archived: true })
    await open()
    expect(await screen.findByText('封存的教室不能修改 AI 設定。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '驗證並儲存' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '新增備援' })).toBeDisabled()
  })

  it('shows the classroom usage and no money', async () => {
    mockSettings(configured)
    server.use(
      http.get(`/api/classrooms/${CID}/ai-usage`, () =>
        HttpResponse.json({
          range: '7d',
          totals: { requests: 4, input_tokens: 40, output_tokens: 20, unreported_requests: 0 },
          items: [{ provider: 'openai', model: 'gpt-main', requests: 4, input_tokens: 40, output_tokens: 20, unreported_requests: 0 }],
        }),
      ),
    )
    await open()
    expect(await screen.findByRole('row', { name: /OpenAI \/ gpt-main/ })).toHaveTextContent('40')
    expect(screen.getByText(/不顯示金額/)).toBeInTheDocument()
  })

  it('shows a load error with retry', async () => {
    mockSettings(configured)
    server.use(http.get(`/api/classrooms/${CID}/ai-settings`, () => apiError(500, 'internal', 'req-s')))
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: ROUTE })
    expect(await screen.findByText('錯誤代碼：req-s')).toBeInTheDocument()
    mockSettings(configured)
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    await waitFor(() => expect(screen.getByDisplayValue('gpt-main')).toBeInTheDocument())
  })

  it('shows another teacher’s classroom (404) as an error, and keeps students out', async () => {
    mockSettings(configured)
    server.use(http.get(`/api/classrooms/${CID}/ai-settings`, () => apiError(404, 'not_found')))
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: ROUTE })
    expect(await screen.findByText('找不到資料。')).toBeInTheDocument()
  })

  it('shows the forbidden page to a student who types the URL', async () => {
    mockMe(makeMe({ student: true }))
    renderApp(null, { route: ROUTE })
    expect(await screen.findByRole('heading', { name: '你沒有權限執行這個操作。' })).toBeInTheDocument()
  })
})
