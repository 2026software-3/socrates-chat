import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ConversationPage } from '@/features/chat/conversation-page'
import { makeDetail } from '@/features/chat/test-data'
import {
  createBrowserRecognizer,
  pickRecognitionSource,
  type RecognizerEvents,
} from '@/features/chat/voice/recognizer'
import { createSpeaker } from '@/features/chat/voice/speaker'
import { AI_USAGE_STALE_EVENT } from '@/lib/ai-usage-events'
import { FakeAudioContext, FakeWorkletNode, feed, installAudioGraph, installMediaDevices, noiseSource, removeMediaDevices, voicedSource } from '@/test/fake-audio'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

// 把靜音與倒數縮短，讓自動送出的流程在測試裡很快跑完
vi.mock('@/features/chat/voice/config', async (orig) => ({
  ...(await orig<typeof import('@/features/chat/voice/config')>()),
  SILENCE_MS: 30,
  COUNTDOWN_MS: 30,
}))

// ---- 假的瀏覽器語音 API ----

type ResultEvent = { results: (ArrayLike<{ transcript: string }> & { isFinal: boolean })[] }

class FakeRecognition {
  static instances: FakeRecognition[] = []
  lang = ''
  continuous = false
  interimResults = false
  onresult: ((e: ResultEvent) => void) | null = null
  onerror: ((e: { error: string }) => void) | null = null
  onend: (() => void) | null = null
  started = 0
  constructor() {
    FakeRecognition.instances.push(this)
  }
  start() {
    this.started += 1
  }
  stop() {
    queueMicrotask(() => this.onend?.())
  }
  abort() {
    queueMicrotask(() => this.onend?.())
  }
  say(text: string, isFinal = true) {
    this.onresult?.({ results: [Object.assign([{ transcript: text }], { isFinal })] })
  }
  fail(error: string) {
    this.onerror?.({ error })
  }
  static get last() {
    return FakeRecognition.instances[FakeRecognition.instances.length - 1]
  }
}

function installRecognition() {
  FakeRecognition.instances = []
  Object.assign(window, { SpeechRecognition: FakeRecognition })
}

function removeRecognition() {
  delete (window as unknown as Record<string, unknown>).SpeechRecognition
  delete (window as unknown as Record<string, unknown>).webkitSpeechRecognition
}

class FakeUtterance {
  voice: unknown
  lang = ''
  text: string
  onstart: (() => void) | null = null
  onend: (() => void) | null = null
  onerror: ((e: { error: string }) => void) | null = null
  constructor(text: string) {
    this.text = text
  }
}

type FakeVoice = { lang: string; name: string }

function installSynthesis(voices: FakeVoice[], opts: { autoEnd?: boolean } = { autoEnd: true }) {
  const spoken: { text: string; lang: string }[] = []
  let current: FakeUtterance | undefined
  const synth = {
    getVoices: () => voices,
    addEventListener: () => {},
    removeEventListener: () => {},
    speak: (u: FakeUtterance) => {
      current = u
      spoken.push({ text: u.text, lang: u.lang })
      u.onstart?.()
      if (opts.autoEnd) setTimeout(() => u.onend?.(), 5)
    },
    cancel: vi.fn(() => {
      current?.onerror?.({ error: 'canceled' })
    }),
  }
  Object.assign(window, { speechSynthesis: synth })
  Object.assign(globalThis, { SpeechSynthesisUtterance: FakeUtterance })
  return { spoken, synth }
}

function removeSynthesis() {
  delete (window as unknown as Record<string, unknown>).speechSynthesis
  delete (globalThis as unknown as Record<string, unknown>).SpeechSynthesisUtterance
}

beforeEach(() => {
  removeRecognition()
  removeSynthesis()
  sessionStorage.clear()
})
afterEach(() => {
  removeRecognition()
  removeSynthesis()
  sessionStorage.clear()
})

// ---- 辨識器 ----

function events(): RecognizerEvents & { texts: [string, boolean][]; errors: string[]; ends: number } {
  const e = {
    texts: [] as [string, boolean][],
    errors: [] as string[],
    ends: 0,
    onText: (t: string, f: boolean) => e.texts.push([t, f]),
    onError: (k: string) => e.errors.push(k),
    onEnd: () => (e.ends += 1),
  }
  return e
}

describe('speech recognition engine choice (S-05.3)', () => {
  it('uses the browser when it has SpeechRecognition and OpenAI when it does not', () => {
    expect(pickRecognitionSource()).toBe('speech-openai')
    installRecognition()
    expect(pickRecognitionSource()).toBe('speech-browser')
  })

  it.each(['network', 'service-not-allowed', 'language-not-supported'])(
    'switches to OpenAI for the rest of the login after a %s error',
    (code) => {
      installRecognition()
      const ev = events()
      createBrowserRecognizer('zh-TW', ev).start()
      FakeRecognition.last.fail(code)
      expect(ev.errors).toEqual(['fallback'])
      expect(pickRecognitionSource()).toBe('speech-openai')
    },
  )

  it('does not switch to OpenAI when the microphone permission is denied', () => {
    installRecognition()
    const ev = events()
    createBrowserRecognizer('zh-TW', ev).start()
    FakeRecognition.last.fail('not-allowed')
    expect(ev.errors).toEqual(['permission'])
    expect(pickRecognitionSource()).toBe('speech-browser')
  })

  it('ignores silence and aborts, and restarts by itself when the browser ends recognition', () => {
    installRecognition()
    const ev = events()
    createBrowserRecognizer('es-ES', ev).start()
    const first = FakeRecognition.last
    expect(first.lang).toBe('es-ES')
    first.fail('no-speech')
    first.fail('aborted')
    expect(ev.errors).toEqual([])
    first.onend?.() // 瀏覽器在靜音後自行結束
    expect(FakeRecognition.instances).toHaveLength(2)
    expect(FakeRecognition.last.started).toBe(1)
    expect(ev.ends).toBe(0)
  })

  it('reports interim and final text and does not restart after stop', async () => {
    installRecognition()
    const ev = events()
    const rec = createBrowserRecognizer('zh-TW', ev)
    rec.start()
    FakeRecognition.last.say('我覺得', false)
    FakeRecognition.last.say('我覺得應該', true)
    expect(ev.texts).toEqual([
      ['我覺得', false],
      ['我覺得應該', true],
    ])
    rec.stop()
    await waitFor(() => expect(ev.ends).toBe(1))
    expect(FakeRecognition.instances).toHaveLength(1)
  })
})

// ---- 語音合成 ----

describe('speech synthesis (S-05.3)', () => {
  it('prefers an exact-region voice, then any voice of the same language', async () => {
    const { spoken } = installSynthesis([
      { lang: 'zh-CN', name: 'cn' },
      { lang: 'zh-TW', name: 'tw' },
    ])
    await createSpeaker('zh-TW', 'zh-TW').speak('你好')
    expect(spoken).toEqual([{ text: '你好', lang: 'zh-TW' }])

    removeSynthesis()
    const second = installSynthesis([{ lang: 'es-MX', name: 'mx' }])
    await createSpeaker('es-ES', 'es').speak('hola')
    expect(second.spoken).toEqual([{ text: 'hola', lang: 'es-MX' }])
  })

  it('stops playback on request', async () => {
    installSynthesis([{ lang: 'en-US', name: 'us' }], { autoEnd: false })
    const speaker = createSpeaker('en-US', 'en')
    const done = speaker.speak('hello')
    await Promise.resolve()
    await new Promise((r) => setTimeout(r, 0))
    speaker.stop()
    expect(await done).toBe(false)
  })

  it('falls back to the backend when the browser has no voice for the language', async () => {
    installSynthesis([{ lang: 'ja-JP', name: 'jp' }])
    let body: unknown
    server.use(
      http.post('/api/voice/speech', async ({ request }) => {
        body = await request.json()
        return new HttpResponse(new Uint8Array([1, 2, 3]), { headers: { 'Content-Type': 'audio/mpeg' } })
      }),
    )
    const played: string[] = []
    class FakeAudio {
      onended: (() => void) | null = null
      onerror: (() => void) | null = null
      constructor(src: string) {
        played.push(src)
      }
      play() {
        setTimeout(() => this.onended?.(), 0)
        return Promise.resolve()
      }
      pause() {}
    }
    vi.stubGlobal('Audio', FakeAudio)
    const create = vi.fn(() => 'blob:fake')
    URL.createObjectURL = create
    URL.revokeObjectURL = vi.fn()
    expect(await createSpeaker('zh-TW', 'zh-TW').speak('你好')).toBe(true)
    expect(body).toEqual({ text: '你好', language: 'zh-TW' })
    expect(played).toEqual(['blob:fake'])
    vi.unstubAllGlobals()
  })

  it('stays silent when stopped while the voices are still loading (S-05.5)', async () => {
    const { synth, spoken } = installSynthesis([])
    const voices = [{ lang: 'zh-TW', name: 'tw' }]
    let loaded: () => void = () => {}
    synth.getVoices = () => []
    synth.addEventListener = ((_: string, fn: () => void) => {
      loaded = fn
    }) as typeof synth.addEventListener
    const speaker = createSpeaker('zh-TW', 'zh-TW')
    const done = speaker.speak('太晚了')
    await new Promise((r) => setTimeout(r, 0))
    speaker.stop()
    expect(await done).toBe(false)
    synth.getVoices = () => voices
    loaded()
    await new Promise((r) => setTimeout(r, 0))
    expect(spoken).toEqual([])
  })

  it('never plays backend audio that arrives after a stop (S-05.5)', async () => {
    let release: () => void = () => {}
    const gate = new Promise<void>((r) => (release = r))
    server.use(
      http.post('/api/voice/speech', async () => {
        await gate
        return new HttpResponse(new Uint8Array([1]), { headers: { 'Content-Type': 'audio/mpeg' } })
      }),
    )
    const played: string[] = []
    vi.stubGlobal(
      'Audio',
      class {
        constructor(src: string) {
          played.push(src)
        }
      },
    )
    const speaker = createSpeaker('zh-TW', 'zh-TW')
    const done = speaker.speak('太晚了')
    await new Promise((r) => setTimeout(r, 10))
    speaker.stop()
    expect(await done).toBe(false)
    release()
    await new Promise((r) => setTimeout(r, 10))
    expect(played).toEqual([])
    vi.unstubAllGlobals()
  })

  it('a new reply replaces the one still playing', async () => {
    const { spoken } = installSynthesis([{ lang: 'zh-TW', name: 'tw' }], { autoEnd: false })
    const speaker = createSpeaker('zh-TW', 'zh-TW')
    const first = speaker.speak('舊的回覆')
    await new Promise((r) => setTimeout(r, 0))
    void speaker.speak('新的回覆')
    expect(await first).toBe(false)
    await new Promise((r) => setTimeout(r, 0))
    expect(spoken.map((u) => u.text)).toEqual(['舊的回覆', '新的回覆'])
  })

  it('refreshes the AI usage display after backend speech, even when it fails (S-03)', async () => {
    const stale = vi.fn()
    window.addEventListener(AI_USAGE_STALE_EVENT, stale)
    server.use(http.post('/api/voice/speech', () => HttpResponse.json({ error: { code: 'voice_unavailable' } }, { status: 503 })))
    await expect(createSpeaker('zh-TW', 'zh-TW').speak('你好')).rejects.toThrow()
    window.removeEventListener(AI_USAGE_STALE_EVENT, stale)
    expect(stale).toHaveBeenCalledTimes(1)
  })

  it('ignores a start event that arrives after a stop', async () => {
    const { synth } = installSynthesis([{ lang: 'zh-TW', name: 'tw' }])
    let utterance: FakeUtterance | undefined
    synth.speak = (u: FakeUtterance) => {
      utterance = u
    }
    const onStart = vi.fn()
    const speaker = createSpeaker('zh-TW', 'zh-TW')
    const done = speaker.speak('你好', onStart)
    await new Promise((r) => setTimeout(r, 0))
    speaker.stop()
    await done
    utterance?.onstart?.()
    expect(onStart).not.toHaveBeenCalled()
  })

  it('reports when and how playback really started', async () => {
    installSynthesis([{ lang: 'zh-TW', name: 'tw' }])
    const onStart = vi.fn()
    await createSpeaker('zh-TW', 'zh-TW').speak('你好', onStart)
    expect(onStart).toHaveBeenCalledExactlyOnceWith('synthesis')
  })

  it('rejects when neither the browser nor the backend can speak', async () => {
    server.use(http.post('/api/voice/speech', () => HttpResponse.json({ error: { code: 'voice_unavailable' } }, { status: 503 })))
    await expect(createSpeaker('zh-TW', 'zh-TW').speak('你好')).rejects.toThrow()
  })
})

// ---- 對話畫面 ----

const sse = (...frames: string[]) =>
  new HttpResponse(
    new ReadableStream<Uint8Array>({
      start(c) {
        const enc = new TextEncoder()
        for (const f of frames) c.enqueue(enc.encode(f))
        c.close()
      },
    }),
    { headers: { 'Content-Type': 'text/event-stream' } },
  )
const delta = (text: string) => `event: delta\ndata: ${JSON.stringify({ text })}\n\n`
const done = `event: done\ndata: ${JSON.stringify({ message_id: 'ai-1', question_type: null, stage: 1, suggest_end: false })}\n\n`

type Posted = { content: string; source: string }

function mockChat(lang: string = 'zh-TW', reply = 'AI 的追問') {
  const posted: Posted[] = []
  server.use(
    http.get('/api/conversations/c1', () => HttpResponse.json(makeDetail({ language: lang }, []))),
    http.post('/api/conversations/c1/messages', async ({ request }) => {
      const body = (await request.json()) as Posted
      posted.push(body)
      const saved = { id: `srv-${posted.length}`, role: 'student', ...body, question_type: null, created_at: '2026-03-05T08:01:00Z', auto_ended: false }
      return sse(`event: message\ndata: ${JSON.stringify(saved)}\n\n`, delta(reply), done)
    }),
  )
  return posted
}

function renderChat() {
  mockMe(makeMe({ student: true }))
  return renderApp(
    <Routes>
      <Route path="/conversations/:id" element={<ConversationPage />} />
    </Routes>,
    { route: '/conversations/c1' },
  )
}

const input = () => screen.findByRole('textbox', { name: '訊息輸入' })

describe('voice input (S-05.1: confirm before sending)', () => {
  it('fills the input with the recognized text, lets the student edit it, and records the browser-speech source', async () => {
    installRecognition()
    const posted = mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    expect(FakeRecognition.last.lang).toBe('zh-TW')
    FakeRecognition.last.say('我支持拉桿', true)
    await waitFor(() => expect(screen.getByRole('textbox', { name: '訊息輸入' })).toHaveValue('我支持拉桿'))
    await userEvent.click(screen.getByRole('button', { name: '結束錄音' }))
    // 沒有自動送出：還在輸入框，學生可以先修改
    expect(posted).toEqual([])
    await userEvent.type(screen.getByRole('textbox', { name: '訊息輸入' }), '，因為救五個人')
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    await waitFor(() => expect(posted).toEqual([{ content: '我支持拉桿，因為救五個人', source: 'speech-browser' }]))
    expect(await screen.findByText('AI 的追問')).toBeInTheDocument()

    // 下一則用打字送出時，來源回到文字
    await userEvent.type(screen.getByRole('textbox', { name: '訊息輸入' }), '再想想')
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    await waitFor(() => expect(posted[1]).toEqual({ content: '再想想', source: 'text' }))
  })

  it('recognizes in the conversation language, not a fixed one', async () => {
    installRecognition()
    mockChat('es')
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    expect(FakeRecognition.last.lang).toBe('es-ES')
  })

  it('shows a message and keeps text input when the microphone is denied', async () => {
    installRecognition()
    mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    FakeRecognition.last.fail('not-allowed')
    expect(await screen.findByText(/無法使用麥克風/u)).toBeInTheDocument()
    expect(sessionStorage.getItem('voice.useOpenAi')).toBeNull()
    await userEvent.type(screen.getByRole('textbox', { name: '訊息輸入' }), '改用打字')
    expect(screen.getByRole('textbox', { name: '訊息輸入' })).toHaveValue('改用打字')
  })

  it('uses the OpenAI fallback when the browser has no speech recognition', async () => {
    // 沒有 SpeechRecognition：錄音後整段上傳到後端辨識
    const posted = mockChat()
    let upload: { type: string; size: number; url: string } | undefined
    server.use(
      http.post('/api/voice/transcribe', async ({ request }) => {
        const buf = await request.arrayBuffer()
        upload = { type: request.headers.get('content-type') ?? '', size: buf.byteLength, url: request.url }
        return HttpResponse.json({ text: '備援辨識的文字' })
      }),
    )
    const { tracks, getUserMedia } = installMediaDevices()
    class FakeRecorder {
      static isTypeSupported = () => true
      state: 'inactive' | 'recording' = 'inactive'
      mimeType = 'audio/webm'
      ondataavailable: ((e: { data: Blob }) => void) | null = null
      onstop: (() => void) | null = null
      start() {
        this.state = 'recording'
      }
      stop() {
        this.state = 'inactive'
        this.ondataavailable?.({ data: new Blob([new Uint8Array(10)], { type: 'audio/webm' }) })
        this.onstop?.()
      }
    }
    vi.stubGlobal('MediaRecorder', FakeRecorder)
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    await userEvent.click(await screen.findByRole('button', { name: '結束錄音' }))
    await waitFor(() => expect(screen.getByRole('textbox', { name: '訊息輸入' })).toHaveValue('備援辨識的文字'))
    expect(upload).toMatchObject({ type: 'audio/webm', size: 10 })
    expect(upload?.url).toContain('language=zh-TW')
    expect(tracks[0].stop).toHaveBeenCalled() // 釋放麥克風，不保留音訊
    // 備援錄音也用瀏覽器原生前處理（S-05.5）
    expect(getUserMedia).toHaveBeenCalledWith({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true } })
    await userEvent.click(screen.getByRole('button', { name: '送出' }))
    await waitFor(() => expect(posted).toEqual([{ content: '備援辨識的文字', source: 'speech-openai' }]))
    vi.unstubAllGlobals()
    removeMediaDevices()
  })

  it('tells the student to type when the fallback service is not enabled', async () => {
    mockChat()
    server.use(http.post('/api/voice/transcribe', () => HttpResponse.json({ error: { code: 'voice_unavailable' } }, { status: 503 })))
    installMediaDevices()
    class FakeRecorder {
      static isTypeSupported = () => true
      state: 'inactive' | 'recording' = 'inactive'
      mimeType = 'audio/webm'
      ondataavailable: ((e: { data: Blob }) => void) | null = null
      onstop: (() => void) | null = null
      start() {
        this.state = 'recording'
      }
      stop() {
        this.state = 'inactive'
        this.ondataavailable?.({ data: new Blob([new Uint8Array(4)]) })
        this.onstop?.()
      }
    }
    vi.stubGlobal('MediaRecorder', FakeRecorder)
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    await userEvent.click(await screen.findByRole('button', { name: '結束錄音' }))
    expect(await screen.findByText(/目前無法使用語音/u)).toBeInTheDocument()
    vi.unstubAllGlobals()
    removeMediaDevices()
  })
})

describe('voice conversation (S-05.2: turn taking)', () => {
  it('sends after a pause, speaks the AI reply, then listens again', async () => {
    installRecognition()
    const { spoken } = installSynthesis([{ lang: 'zh-TW', name: 'tw' }])
    const posted = mockChat('zh-TW', 'AI 的追問')
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    expect(await screen.findByText('聆聽中…請說話')).toBeInTheDocument()
    const first = FakeRecognition.last
    first.say('我支持拉桿', true)

    // 靜音 → 倒數 → 自動送出（來源為瀏覽器語音）
    await waitFor(() => expect(posted).toEqual([{ content: '我支持拉桿', source: 'speech-browser' }]))
    // AI 回覆自動播放；播完自動開始下一輪聆聽（新的辨識器）
    await waitFor(() => expect(spoken).toEqual([{ text: 'AI 的追問', lang: 'zh-TW' }]))
    await waitFor(() => expect(FakeRecognition.instances.length).toBeGreaterThan(1))
    expect(await screen.findByText('聆聽中…請說話')).toBeInTheDocument()
  })

  it('does not send an empty utterance and keeps listening', async () => {
    installRecognition()
    installSynthesis([{ lang: 'zh-TW', name: 'tw' }])
    const posted = mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    await userEvent.click(await screen.findByRole('button', { name: '立即送出' }))
    // 用「立即送出」按鈕但沒說任何話：不送出
    await new Promise((r) => setTimeout(r, 80))
    expect(posted).toEqual([])
    expect(screen.getByText('聆聽中…請說話')).toBeInTheDocument()
  })

  it('the auto-send switch off means only the send button submits', async () => {
    installRecognition()
    installSynthesis([{ lang: 'zh-TW', name: 'tw' }])
    const posted = mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    await userEvent.click(await screen.findByRole('switch', { name: '自動送出' }))
    FakeRecognition.last.say('不要自動送出', true)
    await new Promise((r) => setTimeout(r, 100))
    expect(posted).toEqual([])
    await userEvent.click(screen.getByRole('button', { name: '立即送出' }))
    await waitFor(() => expect(posted).toEqual([{ content: '不要自動送出', source: 'speech-browser' }]))
  })

  it('stopping the playback hands the turn to the student', async () => {
    installRecognition()
    const { spoken } = installSynthesis([{ lang: 'zh-TW', name: 'tw' }], { autoEnd: false })
    mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    FakeRecognition.last.say('開始', true)
    await waitFor(() => expect(spoken).toHaveLength(1))
    // 播放中不收音
    const before = FakeRecognition.instances.length
    await userEvent.click(await screen.findByRole('button', { name: '停止播放' }))
    await waitFor(() => expect(FakeRecognition.instances.length).toBeGreaterThan(before))
    expect(await screen.findByText('聆聽中…請說話')).toBeInTheDocument()
  })

  it('ending the voice conversation stops listening', async () => {
    installRecognition()
    installSynthesis([{ lang: 'zh-TW', name: 'tw' }])
    mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    await userEvent.click(await screen.findByRole('button', { name: '結束語音對話' }))
    expect(screen.queryByText('聆聽中…請說話')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '語音對話' })).toBeInTheDocument()
  })
})

describe('microphone problems in the OpenAI fallback (S-05.5)', () => {
  class IdleRecorder {
    static isTypeSupported = () => true
    state: 'inactive' | 'recording' = 'inactive'
    mimeType = 'audio/webm'
    ondataavailable: ((e: { data: Blob }) => void) | null = null
    onstop: (() => void) | null = null
    start() {
      this.state = 'recording'
    }
    stop() {
      this.state = 'inactive'
      this.onstop?.()
    }
  }

  afterEach(() => {
    vi.unstubAllGlobals()
    removeMediaDevices()
  })

  it('does not blame permissions when there is no microphone', async () => {
    mockChat()
    vi.stubGlobal('MediaRecorder', IdleRecorder)
    installMediaDevices({ getUserMedia: () => Promise.reject(new DOMException('x', 'NotFoundError')) })
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    expect(await screen.findByText(/找不到可用的麥克風/u)).toBeInTheDocument()
    expect(screen.queryByText(/允許麥克風權限/u)).not.toBeInTheDocument()
  })

  it('tells the student when the microphone is unplugged mid-recording', async () => {
    mockChat()
    vi.stubGlobal('MediaRecorder', IdleRecorder)
    const { tracks } = installMediaDevices()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音輸入' }))
    await waitFor(() => expect(tracks).toHaveLength(1))
    tracks[0].unplug()
    expect(await screen.findByText(/找不到可用的麥克風/u)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '語音輸入' })).toBeInTheDocument()
  })
})

describe('barge-in during AI playback (S-05.5)', () => {
  beforeEach(() => {
    installAudioGraph()
    installRecognition()
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
    removeMediaDevices()
  })

  /** 進入語音對話並讓 AI 開始唸，停在播放中。 */
  async function untilSpeaking() {
    const synthesis = installSynthesis([{ lang: 'zh-TW', name: 'tw' }], { autoEnd: false })
    mockChat()
    const view = renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    FakeRecognition.last.say('開始', true)
    await waitFor(() => expect(synthesis.spoken).toHaveLength(1))
    return { ...synthesis, view }
  }

  function studentSpeaks() {
    const emit = (s: Float32Array) => FakeWorkletNode.last.emit(s)
    feed(noiseSource(48_000, -60), 1200, emit)
    feed(voicedSource(48_000, -20), 600, emit)
  }

  it('keeps capturing while the AI speaks, without running the recognizer', async () => {
    const { tracks } = installMediaDevices()
    await untilSpeaking()
    const recognizers = FakeRecognition.instances.length
    await waitFor(() => expect(FakeWorkletNode.instances).toHaveLength(1))
    expect(tracks[0].readyState).toBe('live')
    expect(FakeRecognition.instances).toHaveLength(recognizers)
  })

  it('speaking stops the playback, frees the microphone, and hands the turn over', async () => {
    const { tracks } = installMediaDevices()
    const { synth } = await untilSpeaking()
    await waitFor(() => expect(FakeWorkletNode.instances).toHaveLength(1))
    const recognizers = FakeRecognition.instances.length
    studentSpeaks()
    expect(synth.cancel).toHaveBeenCalled()
    expect(tracks[0].readyState).toBe('ended')
    await waitFor(() => expect(FakeRecognition.instances.length).toBeGreaterThan(recognizers))
    expect(await screen.findByText('聆聽中…請說話')).toBeInTheDocument()
  })

  it('does not open the microphone when the switch is off', async () => {
    const { getUserMedia } = installMediaDevices()
    localStorage.setItem('voice.bargeIn', 'false')
    await untilSpeaking()
    await new Promise((r) => setTimeout(r, 20))
    expect(getUserMedia).not.toHaveBeenCalled()
    expect(screen.getByRole('switch', { name: '開口打斷 AI' })).not.toBeChecked()
  })

  it('turning the switch off during playback releases the microphone at once', async () => {
    const { tracks } = installMediaDevices()
    await untilSpeaking()
    await waitFor(() => expect(FakeWorkletNode.instances).toHaveLength(1))
    await userEvent.click(screen.getByRole('switch', { name: '開口打斷 AI' }))
    expect(tracks[0].readyState).toBe('ended')
    expect(FakeAudioContext.last.close).toHaveBeenCalled()
    expect(localStorage.getItem('voice.bargeIn')).toBe('false')
  })

  it('does not reopen anything when the switch goes off before the AI starts speaking', async () => {
    const { getUserMedia } = installMediaDevices()
    const { synth } = installSynthesis([{ lang: 'zh-TW', name: 'tw' }], { autoEnd: false })
    let utterance: FakeUtterance | undefined
    synth.speak = (u: FakeUtterance) => {
      utterance = u
    }
    mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    FakeRecognition.last.say('開始', true)
    await waitFor(() => expect(utterance).toBeDefined())
    await userEvent.click(screen.getByRole('switch', { name: '開口打斷 AI' }))
    utterance?.onstart?.()
    await new Promise((r) => setTimeout(r, 20))
    expect(getUserMedia).not.toHaveBeenCalled()
    expect(FakeAudioContext.instances.filter((c) => c.state !== 'closed')).toEqual([])
  })

  it('keeps playing and offers the button when the microphone is denied', async () => {
    installMediaDevices({ getUserMedia: () => Promise.reject(new DOMException('x', 'NotAllowedError')) })
    await untilSpeaking()
    expect(await screen.findByText(/開口偵測: 無法使用/u)).toBeInTheDocument()
    expect(screen.getByText('AI 正在說話…')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '停止播放' })).toBeInTheDocument()
    expect(screen.queryByText(/無法使用麥克風/u)).not.toBeInTheDocument()
  })

  it('shows what the browser reported, and warns that speechSynthesis is high risk', async () => {
    installMediaDevices({
      supported: { echoCancellation: true },
      settings: { echoCancellation: true },
    })
    await untilSpeaking()
    const panel = await screen.findByTestId('voice-audio-diagnostics')
    await waitFor(() => expect(panel).toHaveTextContent('AEC: 已開啟（瀏覽器回報）'))
    expect(panel).toHaveTextContent('NS: 不支援')
    expect(panel).toHaveTextContent(/建議戴耳機/u)
  })

  it('still releases everything when the voice conversation ends during playback', async () => {
    const { tracks } = installMediaDevices()
    await untilSpeaking()
    await waitFor(() => expect(FakeWorkletNode.instances).toHaveLength(1))
    await userEvent.click(screen.getByRole('button', { name: '結束語音對話' }))
    expect(tracks[0].readyState).toBe('ended')
    expect(FakeWorkletNode.last.disconnect).toHaveBeenCalled()
    expect(FakeAudioContext.last.close).toHaveBeenCalled()
  })

  it('releases everything when the page goes away during playback', async () => {
    const { tracks } = installMediaDevices()
    const { view } = await untilSpeaking()
    await waitFor(() => expect(FakeWorkletNode.instances).toHaveLength(1))
    const recognizers = FakeRecognition.instances.length
    view.unmount()
    expect(tracks[0].readyState).toBe('ended')
    expect(FakeAudioContext.last.close).toHaveBeenCalled()
    await new Promise((r) => setTimeout(r, 20))
    // 播放被中斷時不能把回合交給已經不存在的畫面
    expect(FakeRecognition.instances).toHaveLength(recognizers)
  })

  it('releases the audio context when the voice conversation cannot start', async () => {
    installMediaDevices()
    mockChat()
    renderChat()
    await input()
    await userEvent.click(screen.getByRole('button', { name: '語音對話' }))
    FakeRecognition.last.fail('not-allowed')
    expect(await screen.findByText(/無法使用麥克風/u)).toBeInTheDocument()
    expect(FakeAudioContext.last.close).toHaveBeenCalled()
  })

  it('the manual button still interrupts while detection is running', async () => {
    const { tracks } = installMediaDevices()
    await untilSpeaking()
    await waitFor(() => expect(FakeWorkletNode.instances).toHaveLength(1))
    await userEvent.click(screen.getByRole('button', { name: '停止播放' }))
    expect(await screen.findByText('聆聽中…請說話')).toBeInTheDocument()
    expect(tracks[0].readyState).toBe('ended')
  })
})
