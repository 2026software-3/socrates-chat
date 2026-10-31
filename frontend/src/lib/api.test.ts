import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { api, ApiError } from '@/lib/api'
import { AI_USAGE_STALE_EVENT } from '@/lib/ai-usage-events'
import { server } from '@/test/server'

/** 記錄 `fn` 執行期間（含之後的微任務）收到幾次「AI 用量可能變了」。 */
async function staleEvents(fn: () => Promise<unknown>): Promise<number> {
  let n = 0
  const on = () => (n += 1)
  window.addEventListener(AI_USAGE_STALE_EVENT, on)
  try {
    await fn().catch(() => undefined)
  } finally {
    window.removeEventListener(AI_USAGE_STALE_EVENT, on)
  }
  return n
}

describe('api client: AI usage refresh', () => {
  it('announces that the AI usage may have changed after a write, even a failed one', async () => {
    server.use(
      http.post('/api/ok', () => HttpResponse.json({})),
      http.post('/api/bad', () => HttpResponse.json({ error: { code: 'ai_usage_exceeded', request_id: 'r' } }, { status: 429 })),
    )
    expect(await staleEvents(() => api.post('/api/ok'))).toBe(1)
    expect(await staleEvents(() => api.post('/api/bad'))).toBe(1)
  })

  it('does not announce anything for reads', async () => {
    server.use(http.get('/api/x', () => HttpResponse.json({})))
    expect(await staleEvents(() => api.get('/api/x'))).toBe(0)
  })
})

describe('api client', () => {
  it('parses JSON and sends same-origin credentials', async () => {
    server.use(http.get('/api/x', () => HttpResponse.json({ ok: 1 })))
    await expect(api.get('/api/x')).resolves.toEqual({ ok: 1 })
  })

  it('returns undefined for 204', async () => {
    server.use(http.post('/api/logout', () => new HttpResponse(null, { status: 204 })))
    await expect(api.post('/api/logout')).resolves.toBeUndefined()
  })

  it('sends a JSON body with a content type', async () => {
    let seen: { type: string | null; body: unknown } | undefined
    server.use(
      http.post('/api/y', async ({ request }) => {
        seen = { type: request.headers.get('content-type'), body: await request.json() }
        return HttpResponse.json({})
      }),
    )
    await api.post('/api/y', { a: 1 })
    expect(seen).toEqual({ type: 'application/json', body: { a: 1 } })
  })

  it('turns backend errors into ApiError with code and request id', async () => {
    server.use(
      http.get('/api/z', () => HttpResponse.json({ error: { code: 'not_enrolled', request_id: 'rid-1' } }, { status: 403 })),
    )
    const err = await api.get('/api/z').catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err).toMatchObject({ status: 403, code: 'not_enrolled', requestId: 'rid-1' })
  })

  it('falls back to the x-request-id header and an internal code for non-JSON errors', async () => {
    server.use(http.get('/api/bad', () => new HttpResponse('<html>bad gateway</html>', { status: 502, headers: { 'x-request-id': 'hdr-9' } })))
    const err = await api.get('/api/bad').catch((e: unknown) => e)
    expect(err).toMatchObject({ status: 502, code: 'internal', requestId: 'hdr-9' })
  })

  it('reports network failures as code "network"', async () => {
    server.use(http.get('/api/down', () => HttpResponse.error()))
    const err = await api.get('/api/down').catch((e: unknown) => e)
    expect(err).toMatchObject({ status: 0, code: 'network' })
  })
})
