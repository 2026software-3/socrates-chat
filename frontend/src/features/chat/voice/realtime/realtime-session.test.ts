import { afterEach, beforeEach, describe, expect, it, vi, type Mock, type Mocked } from 'vitest'
import { MicrophoneError, openMicrophone } from '@/features/chat/voice/audio/microphone'
import { startFrameCapture } from '@/features/chat/voice/audio/frame-capture'
import { decodePcm16, encodePcm16 } from '@/features/chat/voice/realtime/pcm'
import { createRealtimeSession, realtimeUrl, type RealtimeCallbacks } from '@/features/chat/voice/realtime/realtime-session'

vi.mock('@/features/chat/voice/audio/microphone', async (original) => ({
  ...(await original<typeof import('@/features/chat/voice/audio/microphone')>()),
  openMicrophone: vi.fn(),
}))
vi.mock('@/features/chat/voice/audio/frame-capture', async (original) => ({
  ...(await original<typeof import('@/features/chat/voice/audio/frame-capture')>()),
  startFrameCapture: vi.fn(),
}))

class FakeSocket {
  static last: FakeSocket
  static OPEN = 1
  readyState = 0
  sent: Record<string, unknown>[] = []
  onopen: (() => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null
  onmessage: ((e: { data: string }) => void) | null = null
  url: string
  constructor(url: string) {
    this.url = url
    FakeSocket.last = this
  }
  send(data: string) {
    this.sent.push(JSON.parse(data) as Record<string, unknown>)
  }
  close = vi.fn(() => {
    this.readyState = 3
  })
  open() {
    this.readyState = 1
    this.onopen?.()
  }
  event(body: Record<string, unknown>) {
    this.onmessage?.({ data: JSON.stringify(body) })
  }
  drop() {
    this.readyState = 3
    this.onclose?.()
  }
  ofType(type: string) {
    return this.sent.filter((e) => e.type === type)
  }
}

type Source = { onended: (() => void) | null; stop: ReturnType<typeof vi.fn> }

function fakeAudioContext() {
  const sources: Source[] = []
  const instances: { close: ReturnType<typeof vi.fn>; sources: Source[]; sampleRate: number }[] = []
  class Ctx {
    currentTime = 0
    sampleRate = 48_000
    destination = {}
    sources = sources
    resume = vi.fn(() => Promise.resolve())
    close = vi.fn(() => Promise.resolve())
    constructor() {
      instances.push(this)
    }
    createBuffer(_c: number, length: number, rate: number) {
      return { duration: length / rate, copyToChannel: vi.fn() }
    }
    createBufferSource() {
      const source = { buffer: null, onended: null, connect: vi.fn(), disconnect: vi.fn(), start: vi.fn(), stop: vi.fn() }
      sources.push(source)
      return source
    }
  }
  return { Ctx, instances, sources }
}

const audioDelta = (responseId: string, ms = 40) => ({
  type: 'response.output_audio.delta',
  response_id: responseId,
  item_id: responseId.replace('resp', 'item'),
  delta: encodePcm16(new Int16Array((24_000 * ms) / 1000)),
})
const created = (id: string) => ({ type: 'response.created', response: { id, status: 'in_progress' } })
const done = (id: string, status = 'completed') => ({ type: 'response.done', response: { id, status } })

function callbacks(): Mocked<RealtimeCallbacks> {
  return {
    onPhase: vi.fn<RealtimeCallbacks['onPhase']>(),
    onMicrophone: vi.fn<RealtimeCallbacks['onMicrophone']>(),
    onTranscript: vi.fn<RealtimeCallbacks['onTranscript']>(),
    onReplyDelta: vi.fn<RealtimeCallbacks['onReplyDelta']>(),
    onReplyEnd: vi.fn<RealtimeCallbacks['onReplyEnd']>(),
    onReplyFailed: vi.fn<RealtimeCallbacks['onReplyFailed']>(),
    onSettled: vi.fn<RealtimeCallbacks['onSettled']>(),
    onClosed: vi.fn<RealtimeCallbacks['onClosed']>(),
  }
}

describe('realtime session', () => {
  let audio: ReturnType<typeof fakeAudioContext>
  let cb: ReturnType<typeof callbacks>
  let frame: (samples: Float32Array) => void
  let micClose: Mock<() => void>
  let captureStop: Mock<() => void>
  let micEvents: { onEnded?: () => void; onMuted?: () => void }

  const flush = async () => {
    for (let i = 0; i < 5; i += 1) await Promise.resolve()
  }
  async function connected(interruptible = true) {
    const session = createRealtimeSession('conv1', cb, interruptible)
    await flush()
    FakeSocket.last.open()
    await flush()
    FakeSocket.last.event({ type: 'sglang.input_audio.accepted', seq: 0 })
    return session
  }
  const sendFrame = () => frame(new Float32Array(4800).fill(0.5))

  beforeEach(() => {
    vi.useFakeTimers()
    audio = fakeAudioContext()
    cb = callbacks()
    micClose = vi.fn<() => void>()
    captureStop = vi.fn<() => void>()
    micEvents = {}
    vi.stubGlobal('AudioContext', audio.Ctx)
    vi.stubGlobal('WebSocket', FakeSocket)
    vi.mocked(openMicrophone).mockImplementation((opts) => {
      micEvents = opts ?? {}
      return Promise.resolve({
        stream: {} as MediaStream,
        diagnostics: { features: {} } as never,
        close: micClose,
      })
    })
    vi.mocked(startFrameCapture).mockImplementation((_ctx, _stream, opts) => {
      frame = (samples) => opts.onFrame({ samples, sampleRate: 48_000 })
      return Promise.resolve({ stop: captureStop })
    })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  it('builds a same-origin websocket url', () => {
    expect(realtimeUrl('c1', { protocol: 'https:', host: 'a.example' })).toBe('wss://a.example/api/conversations/c1/voice')
    expect(realtimeUrl('c1', { protocol: 'http:', host: 'localhost:5173' })).toBe('ws://localhost:5173/api/conversations/c1/voice')
  })

  it('reports listening only after the server answered', async () => {
    const session = createRealtimeSession('conv1', cb, true)
    await flush()
    expect(cb.onPhase).toHaveBeenLastCalledWith('connecting')
    FakeSocket.last.open()
    await flush()
    expect(cb.onPhase).not.toHaveBeenCalledWith('listening')
    FakeSocket.last.event({ type: 'sglang.input_audio.accepted', seq: 0 })
    expect(cb.onPhase).toHaveBeenLastCalledWith('listening')
    session.close()
  })

  it('sends 16 kHz audio with contiguous seq and an event id', async () => {
    const session = await connected()
    sendFrame()
    sendFrame()
    const appends = FakeSocket.last.ofType('input_audio_buffer.append')
    expect(appends.map((e) => (e.sglang as { seq: number }).seq)).toEqual([0, 1])
    expect(appends[0].event_id).toEqual(expect.any(String))
    expect(appends[0].event_id).not.toBe(appends[1].event_id)
    expect(decodePcm16(appends[0].audio as string).length).toBe(1600)
    session.close()
  })

  it('does not send audio before the socket is open', async () => {
    const session = createRealtimeSession('conv1', cb, true)
    await flush()
    expect(startFrameCapture).not.toHaveBeenCalled()
    session.close()
  })

  it('walks through a whole turn', async () => {
    const session = await connected()
    const ws = FakeSocket.last
    ws.event({ type: 'input_audio_buffer.speech_stopped' })
    expect(cb.onPhase).toHaveBeenLastCalledWith('transcribing')
    ws.event({ type: 'conversation.item.input_audio_transcription.completed', transcript: ' 合成的發言 ' })
    expect(cb.onTranscript).toHaveBeenCalledWith('合成的發言')
    ws.event(created('resp_1'))
    expect(cb.onPhase).toHaveBeenLastCalledWith('waiting')
    ws.event({ type: 'response.output_text.delta', response_id: 'resp_1', delta: '你' })
    expect(cb.onReplyDelta).toHaveBeenCalledWith('你')
    ws.event(audioDelta('resp_1'))
    expect(cb.onPhase).toHaveBeenLastCalledWith('speaking')
    ws.event(done('resp_1'))
    expect(cb.onReplyEnd).toHaveBeenCalledTimes(1)
    // 伺服器比播放早：還在唸就不能回到聆聽
    expect(cb.onPhase).toHaveBeenLastCalledWith('speaking')
    expect(cb.onSettled).not.toHaveBeenCalled()
    audio.sources[0].onended?.()
    expect(cb.onPhase).toHaveBeenLastCalledWith('listening')
    expect(cb.onSettled).toHaveBeenCalledTimes(1)
    session.close()
  })

  it('settles a reply that had no audio', async () => {
    const session = await connected()
    FakeSocket.last.event(created('resp_1'))
    FakeSocket.last.event(done('resp_1'))
    expect(cb.onPhase).toHaveBeenLastCalledWith('listening')
    expect(cb.onSettled).toHaveBeenCalledTimes(1)
    session.close()
  })

  it('on barge-in stops playback at once, reports what was heard and drops late audio', async () => {
    const session = await connected()
    const ws = FakeSocket.last
    ws.event(created('resp_1'))
    ws.event(audioDelta('resp_1'))
    ws.event(audioDelta('resp_1'))
    ws.event({ type: 'input_audio_buffer.speech_started' })
    expect(audio.sources[0].stop).toHaveBeenCalled()
    expect(audio.sources[1].stop).toHaveBeenCalled()
    expect(cb.onReplyEnd).toHaveBeenCalledTimes(1)
    expect(cb.onPhase).toHaveBeenLastCalledWith('listening')
    const [truncate] = ws.ofType('conversation.item.truncate')
    expect(truncate).toMatchObject({ item_id: 'item_1', content_index: 0, audio_end_ms: 0 })
    expect(truncate.event_id).toEqual(expect.any(String))
    // 伺服器取消前已送出的片段與文字都是過期的
    ws.event(audioDelta('resp_1'))
    ws.event({ type: 'response.output_text.delta', response_id: 'resp_1', delta: '遲到' })
    expect(audio.sources).toHaveLength(2)
    expect(cb.onReplyDelta).not.toHaveBeenCalled()
    ws.event(done('resp_1', 'cancelled'))
    expect(cb.onReplyEnd).toHaveBeenCalledTimes(1)
    expect(cb.onSettled).toHaveBeenCalledTimes(1)
    session.close()
  })

  it('reports only the current reply when a later reply is interrupted', async () => {
    const session = await connected()
    const ws = FakeSocket.last
    const ctx = audio.instances[0] as unknown as { currentTime: number }
    for (const id of ['resp_1', 'resp_2']) {
      ws.event(created(id))
      ws.event(audioDelta(id, 1000))
      ws.event(done(id))
      ctx.currentTime += 2
      audio.sources.at(-1)?.onended?.()
    }
    ws.event(created('resp_3'))
    ws.event(audioDelta('resp_3', 1000))
    ctx.currentTime += 0.3
    ws.event({ type: 'input_audio_buffer.speech_started' })
    const [truncate] = ws.ofType('conversation.item.truncate')
    expect(truncate.audio_end_ms).toBeLessThan(500)
    session.close()
  })

  it('ends the session when the server says the turn failed', async () => {
    await connected()
    const ws = FakeSocket.last
    ws.event(created('resp_1'))
    ws.event({ type: 'response.output_text.delta', response_id: 'resp_1', delta: '你' })
    ws.event({ type: 'socrates.turn.failed' })
    expect(cb.onReplyFailed).toHaveBeenCalledTimes(1)
    expect(cb.onClosed).toHaveBeenCalledWith('replyFailed')
    expect(micClose).toHaveBeenCalled()
    expect(cb.onReplyFailed.mock.invocationCallOrder[0]).toBeLessThan(cb.onReplyEnd.mock.invocationCallOrder[0])
  })

  it('settles when interrupted after the server already finished the reply', async () => {
    const session = await connected()
    const ws = FakeSocket.last
    ws.event(created('resp_1'))
    ws.event(audioDelta('resp_1'))
    ws.event(done('resp_1'))
    session.interrupt()
    expect(cb.onSettled).toHaveBeenCalledTimes(1)
    session.close()
  })

  it('ignores speech while no reply is open', async () => {
    const session = await connected()
    FakeSocket.last.event({ type: 'input_audio_buffer.speech_started' })
    expect(FakeSocket.last.ofType('conversation.item.truncate')).toHaveLength(0)
    expect(FakeSocket.last.ofType('response.cancel')).toHaveLength(0)
    session.close()
  })

  it('interrupt cancels the reply with an event the server accepts', async () => {
    const session = await connected()
    const ws = FakeSocket.last
    ws.event(created('resp_1'))
    ws.event(audioDelta('resp_1'))
    session.interrupt()
    expect(audio.sources[0].stop).toHaveBeenCalled()
    const [cancel] = ws.ofType('response.cancel')
    expect(Object.keys(cancel).sort()).toEqual(['event_id', 'type'])
    expect(ws.ofType('conversation.item.truncate')).toHaveLength(1)
    session.close()
  })

  it('interrupt with nothing playing sends nothing', async () => {
    const session = await connected()
    session.interrupt()
    expect(FakeSocket.last.ofType('response.cancel')).toHaveLength(0)
    session.close()
  })

  describe('with barge-in off', () => {
    const levels = (e: Record<string, unknown>) => [...decodePcm16(e.audio as string)]

    it('keeps sending silence during a reply, so seq stays contiguous and the voice detector keeps running', async () => {
      const session = await connected(false)
      const ws = FakeSocket.last
      sendFrame()
      ws.event(created('resp_1'))
      sendFrame()
      ws.event(audioDelta('resp_1'))
      sendFrame()
      const appends = ws.ofType('input_audio_buffer.append')
      expect(appends.map((e) => (e.sglang as { seq: number }).seq)).toEqual([0, 1, 2])
      expect(Math.max(...levels(appends[0]))).toBeGreaterThan(0)
      expect(Math.max(...levels(appends[1]))).toBe(0)
      expect(Math.max(...levels(appends[2]))).toBe(0)
      session.close()
    })

    it('opens the microphone again when the reply was played', async () => {
      const session = await connected(false)
      const ws = FakeSocket.last
      ws.event(created('resp_1'))
      ws.event(audioDelta('resp_1'))
      ws.event(done('resp_1'))
      audio.sources[0].onended?.()
      sendFrame()
      expect(Math.max(...levels(ws.ofType('input_audio_buffer.append')[0]))).toBeGreaterThan(0)
      session.close()
    })

    it('opens the microphone again after a manual interrupt', async () => {
      const session = await connected(false)
      const ws = FakeSocket.last
      ws.event(created('resp_1'))
      ws.event(audioDelta('resp_1'))
      session.interrupt()
      sendFrame()
      expect(Math.max(...levels(ws.ofType('input_audio_buffer.append')[0]))).toBeGreaterThan(0)
      session.close()
    })

    it('can be turned on during a reply', async () => {
      const session = await connected(false)
      const ws = FakeSocket.last
      ws.event(created('resp_1'))
      session.setInterruptible(true)
      sendFrame()
      expect(Math.max(...levels(ws.ofType('input_audio_buffer.append')[0]))).toBeGreaterThan(0)
      session.close()
    })
  })

  it.each([
    ['closes before any event', (ws: FakeSocket) => ws.drop()],
    ['fails to open', (ws: FakeSocket) => ws.onerror?.()],
  ])('reports the service as unreachable when it %s', async (_name, fail) => {
    createRealtimeSession('conv1', cb, true)
    await flush()
    FakeSocket.last.open()
    await flush()
    fail(FakeSocket.last)
    expect(cb.onClosed).toHaveBeenCalledWith('unreachable')
    expect(cb.onClosed).toHaveBeenCalledTimes(1)
    expect(micClose).toHaveBeenCalled()
  })

  it('gives up when the server stays silent', async () => {
    createRealtimeSession('conv1', cb, true)
    await flush()
    FakeSocket.last.open()
    await flush()
    await vi.advanceTimersByTimeAsync(10_000)
    expect(cb.onClosed).toHaveBeenCalledWith('unreachable')
    expect(FakeSocket.last.close).toHaveBeenCalled()
  })

  it('reports a dropped connection, ends the reply and releases everything', async () => {
    await connected()
    const ws = FakeSocket.last
    ws.event(created('resp_1'))
    ws.event(audioDelta('resp_1'))
    ws.drop()
    expect(cb.onClosed).toHaveBeenCalledWith('lost')
    expect(cb.onReplyEnd).toHaveBeenCalledTimes(1)
    expect(audio.sources[0].stop).toHaveBeenCalled()
    expect(captureStop).toHaveBeenCalled()
    expect(micClose).toHaveBeenCalled()
    expect(audio.instances[0].close).toHaveBeenCalled()
  })

  it('treats a fatal error as lost and keeps going on a recoverable one', async () => {
    await connected()
    const ws = FakeSocket.last
    ws.event({ type: 'error', sglang: { fatal: false }, error: { code: 'invalid_request' } })
    expect(cb.onClosed).not.toHaveBeenCalled()
    ws.event({ type: 'error', sglang: { fatal: true }, error: { code: 'idle_timeout' } })
    expect(cb.onClosed).toHaveBeenCalledWith('lost')
  })

  it.each([
    ['permission', 'permission'],
    ['no-device', 'device'],
    ['busy', 'device'],
    ['unsupported', 'unavailable'],
  ] as const)('maps microphone failure %s without opening a socket', async (kind, reason) => {
    vi.mocked(openMicrophone).mockRejectedValueOnce(new MicrophoneError(kind))
    const before = FakeSocket.last
    createRealtimeSession('conv1', cb, true)
    await flush()
    expect(cb.onClosed).toHaveBeenCalledWith(reason)
    expect(FakeSocket.last).toBe(before)
    expect(audio.instances[0].close).toHaveBeenCalled()
  })

  it('treats losing the microphone as a device failure', async () => {
    await connected()
    micEvents.onEnded?.()
    expect(cb.onClosed).toHaveBeenCalledWith('device')
    expect(FakeSocket.last.close).toHaveBeenCalled()
  })

  it('close releases everything once and stays silent', async () => {
    const session = await connected()
    session.close()
    session.close()
    expect(FakeSocket.last.close).toHaveBeenCalledTimes(1)
    expect(captureStop).toHaveBeenCalledTimes(1)
    expect(micClose).toHaveBeenCalledTimes(1)
    expect(audio.instances[0].close).toHaveBeenCalledTimes(1)
    expect(cb.onClosed).not.toHaveBeenCalled()
  })

  it('a microphone opened after close is released and no socket is opened', async () => {
    let finish: (mic: never) => void = () => {}
    vi.mocked(openMicrophone).mockImplementationOnce(
      ({ signal } = {}) =>
        new Promise((resolve, reject) => {
          finish = resolve as never
          signal?.addEventListener('abort', () => reject(new MicrophoneError('aborted')))
        }),
    )
    const before = FakeSocket.last
    const session = createRealtimeSession('conv1', cb, true)
    session.close()
    await flush()
    finish({} as never)
    await flush()
    expect(FakeSocket.last).toBe(before)
    expect(cb.onClosed).not.toHaveBeenCalled()
  })

  it('stops the capture when it finishes starting after close', async () => {
    let finish: (c: { stop: () => void }) => void = () => {}
    vi.mocked(startFrameCapture).mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)))
    const session = createRealtimeSession('conv1', cb, true)
    await flush()
    FakeSocket.last.open()
    await flush()
    session.close()
    finish({ stop: captureStop })
    await flush()
    expect(captureStop).toHaveBeenCalled()
  })
})
