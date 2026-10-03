import type { ChatModelRunOptions, ChatModelRunResult } from '@assistant-ui/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it, vi } from 'vitest'
import { createChatAdapter } from '@/features/chat/chat-adapter'
import type { SentMessage, StreamDone } from '@/lib/types'
import { apiError } from '@/test/render'
import { server } from '@/test/server'

const enc = new TextEncoder()

function sse(...frames: string[]) {
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      for (const f of frames) c.enqueue(enc.encode(f))
      c.close()
    },
  })
  return new HttpResponse(body, { headers: { 'Content-Type': 'text/event-stream' } })
}

const done: StreamDone = { message_id: 'ai-1', question_type: 'clarify', stage: 2, suggest_end: true }
const doneFrame = `event: done\ndata: ${JSON.stringify(done)}\n\n`
const delta = (text: string) => `event: delta\ndata: ${JSON.stringify({ text })}\n\n`

function userMsg(id: string, text = '合成問題') {
  return { id, role: 'user', content: [{ type: 'text', text }] }
}

/** 只提供 adapter 實際用到的欄位。 */
function runOptions(messages: unknown[], abortSignal = new AbortController().signal): ChatModelRunOptions {
  return { messages, abortSignal } as unknown as ChatModelRunOptions
}

async function drain(gen: ReturnType<ReturnType<typeof createChatAdapter>['run']>): Promise<ChatModelRunResult[]> {
  const out: ChatModelRunResult[] = []
  for await (const r of gen as AsyncGenerator<ChatModelRunResult>) out.push(r)
  return out
}

const sent: SentMessage = {
  id: 's1',
  role: 'student',
  content: '合成問題',
  source: 'text',
  question_type: null,
  created_at: '2026-01-01T00:00:00Z',
  auto_ended: false,
}

function mockSend(calls: unknown[], result: SentMessage = sent) {
  server.use(
    http.post('/api/conversations/c1/messages', async ({ request }) => {
      calls.push(await request.json())
      return HttpResponse.json(result, { status: 201 })
    }),
  )
}

describe('createChatAdapter', () => {
  it('POSTs the new student message first, then streams the accumulated reply', async () => {
    const order: string[] = []
    server.use(
      http.post('/api/conversations/c1/messages', async ({ request }) => {
        order.push('post')
        expect(await request.json()).toEqual({ content: '合成問題', source: 'text' })
        return HttpResponse.json(sent, { status: 201 })
      }),
      http.get('/api/conversations/c1/stream', () => {
        order.push('stream')
        return sse(delta('你'), delta('好'), doneFrame)
      }),
    )
    const onDone = vi.fn()
    const onSent = vi.fn()
    const adapter = createChatAdapter({ conversationId: 'c1', onDone, onSent })
    const out = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(order).toEqual(['post', 'stream'])
    expect(out.map((r) => r.content)).toEqual([
      [{ type: 'text', text: '你' }],
      [{ type: 'text', text: '你好' }],
    ])
    expect(onSent).toHaveBeenCalledWith(sent)
    expect(onDone).toHaveBeenCalledWith(done)
  })

  it('does not stream when the POST says the conversation auto-ended', async () => {
    const calls: unknown[] = []
    mockSend(calls, { ...sent, auto_ended: true })
    const onSent = vi.fn()
    const adapter = createChatAdapter({ conversationId: 'c1', onSent })
    const out = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(out).toEqual([])
    expect(onSent).toHaveBeenCalledWith(expect.objectContaining({ auto_ended: true }))
  })

  it('retry re-opens the stream only: an ai_unavailable error does not cause a second POST', async () => {
    const posts: unknown[] = []
    let streams = 0
    mockSend(posts)
    server.use(
      http.get('/api/conversations/c1/stream', () => {
        streams += 1
        return streams === 1 ? sse('event: error\ndata: {"code":"ai_unavailable"}\n\n') : sse(delta('ok'), doneFrame)
      }),
    )
    const adapter = createChatAdapter({ conversationId: 'c1' })
    const first = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(first).toEqual([{ status: { type: 'incomplete', reason: 'error', error: 'ai_unavailable' } }])

    const second = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(second.at(-1)?.content).toEqual([{ type: 'text', text: 'ok' }])
    expect(posts).toHaveLength(1)
    expect(streams).toBe(2)
  })

  it('POSTs again after a failed POST (the message was not saved)', async () => {
    let posts = 0
    server.use(
      http.post('/api/conversations/c1/messages', () => {
        posts += 1
        return posts === 1 ? apiError(400, 'invalid_message') : HttpResponse.json(sent, { status: 201 })
      }),
      http.get('/api/conversations/c1/stream', () => sse(delta('ok'), doneFrame)),
    )
    const adapter = createChatAdapter({ conversationId: 'c1' })
    const first = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(first).toEqual([{ status: { type: 'incomplete', reason: 'error', error: 'invalid_message' } }])
    await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(posts).toBe(2)
  })

  it('never POSTs messages that were already persisted (loaded history)', async () => {
    server.use(http.get('/api/conversations/c1/stream', () => sse(delta('ok'), doneFrame)))
    const adapter = createChatAdapter({ conversationId: 'c1', persistedIds: ['u0'] })
    const out = await drain(adapter.run(runOptions([userMsg('u0')])))
    expect(out.at(-1)?.content).toEqual([{ type: 'text', text: 'ok' }])
  })

  it('reports a network failure as the "network" code', async () => {
    server.use(http.post('/api/conversations/c1/messages', () => HttpResponse.error()))
    const adapter = createChatAdapter({ conversationId: 'c1' })
    const out = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(out).toEqual([{ status: { type: 'incomplete', reason: 'error', error: 'network' } }])
  })

  it('stops quietly when aborted', async () => {
    server.use(http.get('/api/conversations/c1/stream', () => sse(delta('x'), doneFrame)))
    const ctrl = new AbortController()
    ctrl.abort()
    const adapter = createChatAdapter({ conversationId: 'c1', persistedIds: ['u1'] })
    const out = await drain(adapter.run(runOptions([userMsg('u1')], ctrl.signal)))
    expect(out).toEqual([])
  })

  it('reports conversation_ended from the POST via onEnded without an error result', async () => {
    server.use(http.post('/api/conversations/c1/messages', () => apiError(409, 'conversation_ended')))
    const onEnded = vi.fn()
    const adapter = createChatAdapter({ conversationId: 'c1', onEnded })
    const out = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(onEnded).toHaveBeenCalledTimes(1)
    expect(out).toEqual([])
  })

  it('reports conversation_ended from the stream via onEnded', async () => {
    mockSend([])
    server.use(
      http.get('/api/conversations/c1/stream', () =>
        sse(`event: error\ndata: ${JSON.stringify({ code: 'conversation_ended' })}\n\n`),
      ),
    )
    const onEnded = vi.fn()
    const adapter = createChatAdapter({ conversationId: 'c1', onEnded })
    await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(onEnded).toHaveBeenCalledTimes(1)
  })

  it('does not fail a completed reply when the onDone callback throws', async () => {
    mockSend([])
    server.use(http.get('/api/conversations/c1/stream', () => sse(delta('完整'), doneFrame)))
    const adapter = createChatAdapter({
      conversationId: 'c1',
      onDone: () => {
        throw new Error('boom')
      },
    })
    const out = await drain(adapter.run(runOptions([userMsg('u1')])))
    expect(out.some((r) => r.status?.type === 'incomplete')).toBe(false)
  })

  it('ignores a thread whose last message is not from the student', async () => {
    const adapter = createChatAdapter({ conversationId: 'c1' })
    const out = await drain(adapter.run(runOptions([{ id: 'a1', role: 'assistant', content: [] }])))
    expect(out).toEqual([])
  })
})
