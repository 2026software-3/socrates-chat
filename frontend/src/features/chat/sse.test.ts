import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { ApiError } from '@/lib/api'
import type { StreamDone } from '@/lib/types'
import { SseParser, readSseFrames, sendMessage, streamReply, type ReplyEvent } from '@/features/chat/sse'
import { server } from '@/test/server'

const enc = new TextEncoder()

/** 把位元組切成指定的區塊，模擬網路分段。 */
function streamOf(chunks: Uint8Array[]): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(c) {
      for (const ch of chunks) c.enqueue(ch)
      c.close()
    },
  })
}

async function collect<T>(it: AsyncIterable<T>): Promise<T[]> {
  const out: T[] = []
  for await (const x of it) out.push(x)
  return out
}

describe('SseParser', () => {
  it('parses a complete frame', () => {
    const p = new SseParser()
    expect(p.feed('event: delta\ndata: {"text":"hi"}\n\n')).toEqual([{ event: 'delta', data: '{"text":"hi"}' }])
  })

  it('handles frames split across chunks', () => {
    const p = new SseParser()
    expect(p.feed('event: del')).toEqual([])
    expect(p.feed('ta\ndata: {"te')).toEqual([])
    expect(p.feed('xt":"a"}\n')).toEqual([])
    expect(p.feed('\nevent: done\ndata: {}\n\n')).toEqual([
      { event: 'delta', data: '{"text":"a"}' },
      { event: 'done', data: '{}' },
    ])
  })

  it('handles CRLF, lone CR and a CRLF split between chunks', () => {
    const p = new SseParser()
    expect(p.feed('event: a\r\ndata: 1\r\n\r\n')).toEqual([{ event: 'a', data: '1' }])
    expect(p.feed('event: b\rdata: 2\r\r')).toEqual([{ event: 'b', data: '2' }])
    expect(p.feed('event: c\r')).toEqual([])
    expect(p.feed('\ndata: 3\r')).toEqual([])
    expect(p.feed('\n\r')).toEqual([{ event: 'c', data: '3' }])
    expect(p.feed('\n')).toEqual([])
  })

  it('joins multiple data lines, ignores comments and unknown fields', () => {
    const p = new SseParser()
    expect(p.feed(': keep-alive\nid: 7\nretry: 1\nevent: x\ndata: a\ndata: b\n\n')).toEqual([{ event: 'x', data: 'a\nb' }])
  })

  it('defaults the event name to "message" and strips one leading space only', () => {
    const p = new SseParser()
    expect(p.feed('data:  two\n\n')).toEqual([{ event: 'message', data: ' two' }])
  })

  it('does not emit frames without data', () => {
    const p = new SseParser()
    expect(p.feed('event: x\n\n')).toEqual([])
  })

  it('flushes a last frame without trailing blank line on end()', () => {
    const p = new SseParser()
    expect(p.feed('event: done\ndata: {}')).toEqual([])
    expect(p.end()).toEqual([{ event: 'done', data: '{}' }])
  })
})

describe('readSseFrames', () => {
  it('decodes a multibyte character split across chunks', async () => {
    const bytes = enc.encode('event: delta\ndata: {"text":"蘇格拉底"}\n\n')
    // 在「蘇」的三個位元組中間切開
    const cut = bytes.indexOf(0xe8) + 1
    const frames = await collect(readSseFrames(streamOf([bytes.slice(0, cut), bytes.slice(cut)])))
    expect(frames).toEqual([{ event: 'delta', data: '{"text":"蘇格拉底"}' }])
  })

  it('yields frames one by one across many tiny chunks', async () => {
    const bytes = enc.encode('event: a\ndata: 1\n\nevent: b\ndata: 2\n\n')
    const chunks = Array.from(bytes, (b) => Uint8Array.of(b))
    const frames = await collect(readSseFrames(streamOf(chunks)))
    expect(frames.map((f) => f.event)).toEqual(['a', 'b'])
  })
})

const sse = (...frames: string[]) =>
  new HttpResponse(streamOf(frames.map((f) => enc.encode(f))), { headers: { 'Content-Type': 'text/event-stream' } })

const done: StreamDone = { message_id: 'm1', question_type: 'clarify', stage: 2, suggest_end: false }

describe('streamReply', () => {
  it('yields delta then done events and ignores unknown events', async () => {
    server.use(
      http.get('/api/conversations/c1/stream', () =>
        sse(
          'event: delta\ndata: {"text":"你"}\n\n',
          'event: ping\ndata: {}\n\n',
          'event: delta\ndata: {"text":"好"}\n\n',
          `event: done\ndata: ${JSON.stringify(done)}\n\n`,
        ),
      ),
    )
    const events = await collect(streamReply('c1'))
    expect(events).toEqual<ReplyEvent[]>([
      { type: 'delta', text: '你' },
      { type: 'delta', text: '好' },
      { type: 'done', done },
    ])
  })

  it('throws an ApiError with the code of an error event', async () => {
    server.use(
      http.get('/api/conversations/c1/stream', () =>
        sse('event: delta\ndata: {"text":"x"}\n\n', 'event: error\ndata: {"code":"ai_unavailable"}\n\n'),
      ),
    )
    const err = await collect(streamReply('c1')).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err).toMatchObject({ code: 'ai_unavailable' })
  })

  it('throws the HTTP error code when the stream cannot be opened', async () => {
    server.use(
      http.get('/api/conversations/c1/stream', () =>
        HttpResponse.json({ error: { code: 'reply_in_progress', request_id: 'r1' } }, { status: 409 }),
      ),
    )
    await expect(collect(streamReply('c1'))).rejects.toMatchObject({ code: 'reply_in_progress', status: 409 })
  })

  it('throws a network error when the stream ends without a done event', async () => {
    server.use(http.get('/api/conversations/c1/stream', () => sse('event: delta\ndata: {"text":"x"}\n\n')))
    await expect(collect(streamReply('c1'))).rejects.toMatchObject({ code: 'network' })
  })

  it('throws a network error when the request fails', async () => {
    server.use(http.get('/api/conversations/c1/stream', () => HttpResponse.error()))
    await expect(collect(streamReply('c1'))).rejects.toMatchObject({ code: 'network' })
  })

  it('treats a malformed done frame as a finished stream without a done event', async () => {
    for (const bad of ['event: done\ndata: not-json\n\n', 'event: done\ndata: {"stage":9}\n\n']) {
      server.use(http.get('/api/conversations/c1/stream', () => sse('event: delta\ndata: {"text":"x"}\n\n' + bad)))
      const events = await collect(streamReply('c1'))
      expect(events).toEqual([{ type: 'delta', text: 'x' }])
    }
  })

  it('rethrows AbortError when aborted', async () => {
    server.use(http.get('/api/conversations/c1/stream', () => sse('event: delta\ndata: {"text":"x"}\n\n')))
    const ctrl = new AbortController()
    ctrl.abort()
    await expect(collect(streamReply('c1', ctrl.signal))).rejects.toMatchObject({ name: 'AbortError' })
  })
})

describe('sendMessage', () => {
  const saved = {
    id: 's1',
    role: 'student',
    content: '想法',
    source: 'text',
    question_type: null,
    created_at: '2026-01-01T00:00:00Z',
    auto_ended: false,
  }
  const message = (m = saved) => `event: message\ndata: ${JSON.stringify(m)}\n\n`
  const body = { content: '想法', source: 'text' }

  it('POSTs the JSON body and yields the saved message, then the reply events', async () => {
    let posted: unknown
    server.use(
      http.post('/api/conversations/c1/messages', async ({ request }) => {
        posted = await request.json()
        return sse(message(), 'event: delta\ndata: {"text":"嗨"}\n\n', `event: done\ndata: ${JSON.stringify(done)}\n\n`)
      }),
    )
    const events = await collect(sendMessage('c1', body))
    expect(posted).toEqual(body)
    expect(events).toEqual([
      { type: 'sent', sent: saved },
      { type: 'delta', text: '嗨' },
      { type: 'done', done },
    ])
  })

  it('ends normally after an auto_ended message (no AI reply follows)', async () => {
    const last = { ...saved, auto_ended: true }
    server.use(http.post('/api/conversations/c1/messages', () => sse(message(last))))
    expect(await collect(sendMessage('c1', body))).toEqual([{ type: 'sent', sent: last }])
  })

  it('throws the API error code for HTTP errors before any event', async () => {
    server.use(
      http.post('/api/conversations/c1/messages', () =>
        HttpResponse.json({ error: { code: 'reply_in_progress', request_id: 'r1' } }, { status: 409 }),
      ),
    )
    await expect(collect(sendMessage('c1', body))).rejects.toMatchObject({ code: 'reply_in_progress', status: 409 })
  })

  it('throws after the saved message when the stream ends without done', async () => {
    server.use(http.post('/api/conversations/c1/messages', () => sse(message())))
    const seen: string[] = []
    await expect(
      (async () => {
        for await (const ev of sendMessage('c1', body)) seen.push(ev.type)
      })(),
    ).rejects.toMatchObject({ code: 'network' })
    expect(seen).toEqual(['sent'])
  })
})
