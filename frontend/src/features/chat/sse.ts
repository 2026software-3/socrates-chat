import { ApiError, apiErrorFrom, isAbortError } from '@/lib/api'
import type { SentMessage, StreamDone } from '@/lib/types'

/** 一個完整的 SSE 事件框。 */
export type SseFrame = { event: string; data: string }

/**
 * 逐段餵入文字的 SSE 解析器（text/event-stream）。
 * 事件框與行結尾（LF、CRLF、CR）可以被任意切在區塊邊界；未知欄位與註解會被忽略。
 */
export class SseParser {
  private line = ''
  private skipLF = false
  private event = ''
  private data: string[] = []

  feed(text: string): SseFrame[] {
    const frames: SseFrame[] = []
    for (const ch of text) {
      if (this.skipLF) {
        this.skipLF = false
        if (ch === '\n') continue
      }
      if (ch === '\r' || ch === '\n') {
        this.skipLF = ch === '\r'
        const frame = this.handleLine(this.line)
        this.line = ''
        if (frame) frames.push(frame)
      } else {
        this.line += ch
      }
    }
    return frames
  }

  /** 串流結束：把最後一個沒有空行收尾的事件框也送出。 */
  end(): SseFrame[] {
    const frames: SseFrame[] = []
    if (this.line !== '') {
      const f = this.handleLine(this.line)
      this.line = ''
      if (f) frames.push(f)
    }
    const last = this.dispatch()
    if (last) frames.push(last)
    return frames
  }

  private dispatch(): SseFrame | null {
    const frame = this.data.length > 0 ? { event: this.event || 'message', data: this.data.join('\n') } : null
    this.event = ''
    this.data = []
    return frame
  }

  private handleLine(line: string): SseFrame | null {
    if (line === '') return this.dispatch()
    if (line.startsWith(':')) return null
    const i = line.indexOf(':')
    const field = i === -1 ? line : line.slice(0, i)
    let value = i === -1 ? '' : line.slice(i + 1)
    if (value.startsWith(' ')) value = value.slice(1)
    if (field === 'event') this.event = value
    else if (field === 'data') this.data.push(value)
    return null
  }
}

/** 讀取 SSE 位元組串流。TextDecoder 用 stream 模式，被切開的多位元組字元不會亂碼。 */
export async function* readSseFrames(body: ReadableStream<Uint8Array>): AsyncGenerator<SseFrame> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  const parser = new SseParser()
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      yield* parser.feed(decoder.decode(value, { stream: true }))
    }
    yield* parser.feed(decoder.decode())
    yield* parser.end()
  } finally {
    // 提早離開（done 事件、中止）時釋放連線
    reader.cancel().catch(() => undefined)
  }
}

export type ReplyEvent = { type: 'delta'; text: string } | { type: 'done'; done: StreamDone }
/** `POST …/messages` 的串流事件：先是保存好的學生訊息，之後與重試串流相同。 */
export type SendEvent = { type: 'sent'; sent: SentMessage } | ReplyEvent

function isAbort(e: unknown, signal?: AbortSignal): boolean {
  return signal?.aborted === true || isAbortError(e)
}

function parseJson(data: string): unknown {
  try {
    return JSON.parse(data)
  } catch {
    return undefined
  }
}

function isStreamDone(p: unknown): p is StreamDone {
  if (typeof p !== 'object' || p === null) return false
  const d = p as Record<string, unknown>
  return (d.stage === 1 || d.stage === 2 || d.stage === 3) && typeof d.suggest_end === 'boolean'
}

function isSentMessage(p: unknown): p is SentMessage {
  if (typeof p !== 'object' || p === null) return false
  const m = p as Record<string, unknown>
  return typeof m.id === 'string' && typeof m.auto_ended === 'boolean'
}

/**
 * 開啟一條回覆串流（POST 訊息或 GET 重試）並依序產出事件。
 * `error` 事件、HTTP 錯誤、連線中斷或沒有 `done` 就結束，都會丟出帶 `code` 的 ApiError；
 * 中止（abort）則原樣丟出 AbortError。達回合上限時只有 `message` 事件（`auto_ended`），
 * 沒有 AI 回覆，這時串流正常結束。
 */
async function* openStream(url: string, init: RequestInit, signal?: AbortSignal): AsyncGenerator<SendEvent> {
  let res: Response
  try {
    res = await fetch(url, {
      credentials: 'same-origin',
      ...init,
      headers: { Accept: 'text/event-stream', ...init.headers },
      signal,
    })
  } catch (e) {
    if (isAbort(e, signal)) throw e
    throw new ApiError(0, 'network', '')
  }
  if (!res.ok) throw await apiErrorFrom(res)
  if (!res.body) throw new ApiError(0, 'network', '')

  try {
    for await (const frame of readSseFrames(res.body)) {
      const payload = parseJson(frame.data) as Record<string, unknown> | undefined
      if (frame.event === 'message') {
        if (!isSentMessage(payload)) throw new ApiError(0, 'internal', '')
        yield { type: 'sent', sent: payload }
        // 達回合上限：對話已自動結束，沒有 AI 回覆可等
        if (payload.auto_ended) return
      } else if (frame.event === 'delta') {
        if (typeof payload?.text === 'string') yield { type: 'delta', text: payload.text }
      } else if (frame.event === 'done') {
        // 回覆已完整串完；done 內容格式不對時只略過階段更新，不把整則回覆當成失敗
        if (isStreamDone(payload)) yield { type: 'done', done: payload }
        return
      } else if (frame.event === 'error') {
        throw new ApiError(res.status, typeof payload?.code === 'string' ? payload.code : 'internal', '')
      }
      // 其他事件一律忽略
    }
  } catch (e) {
    if (e instanceof ApiError || isAbort(e, signal)) throw e
    throw new ApiError(0, 'network', '')
  }
  throw new ApiError(0, 'network', '')
}

/** 送出學生訊息並串流 AI 回覆：`POST /api/conversations/{id}/messages`（S-08.4）。 */
export function sendMessage(
  conversationId: string,
  body: { content: string; source: string },
  signal?: AbortSignal,
): AsyncGenerator<SendEvent> {
  return openStream(
    `/api/conversations/${conversationId}/messages`,
    { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) },
    signal,
  )
}

/** 重試：`GET /api/conversations/{id}/stream` 只重新產生還沒有回覆的 AI 回覆。 */
export async function* streamReply(conversationId: string, signal?: AbortSignal): AsyncGenerator<ReplyEvent> {
  for await (const ev of openStream(`/api/conversations/${conversationId}/stream`, {}, signal)) {
    if (ev.type !== 'sent') yield ev
  }
}
