/**
 * 語音辨識（S-05.3）。預設用瀏覽器的 Web Speech API；瀏覽器不支援，或出現
 * `network`／`service-not-allowed`／`language-not-supported` 錯誤時改用 OpenAI（經後端），
 * 之後這次登入（分頁工作階段）持續使用 OpenAI。拒絕麥克風權限不會改用，因為改了也一樣無法錄音。
 *
 * 音訊不保存：瀏覽器模式的音訊不經過我們的後端；OpenAI 備援只在記憶體中上傳，沒有任何存放。
 * 這裡不記錄任何辨識內容。
 */
import { ApiError } from '@/lib/api'

export type RecognitionSource = 'speech-browser' | 'speech-openai'

export type RecognizerEvents = {
  /** 辨識到文字：`final=false` 是尚未確定的即時結果；OpenAI 備援只會給一次 `final=true`。 */
  onText: (text: string, final: boolean) => void
  /** 辨識結束（自然結束、被停止或出錯後）。 */
  onEnd: () => void
  /**
   * - `permission`：使用者拒絕麥克風權限
   * - `fallback`：瀏覽器辨識不可用，呼叫端應改用 OpenAI 備援重新開始
   * - `unavailable`：備援服務沒有啟用
   * - `failed`：其他失敗
   */
  onError: (kind: 'permission' | 'fallback' | 'unavailable' | 'failed') => void
}

export type Recognizer = {
  source: RecognitionSource
  start: () => void
  /** 停止收音；OpenAI 備援會在這時才上傳並辨識。 */
  stop: () => void
}

// ---- Web Speech API 的最小型別（lib.dom 沒有 SpeechRecognition 本體）----

type BrowserRecognition = {
  lang: string
  continuous: boolean
  interimResults: boolean
  onresult: ((e: SpeechRecognitionEvent) => void) | null
  onerror: ((e: SpeechRecognitionErrorEvent) => void) | null
  onend: (() => void) | null
  start: () => void
  stop: () => void
  abort: () => void
}
type BrowserRecognitionCtor = new () => BrowserRecognition

export function browserRecognitionCtor(): BrowserRecognitionCtor | undefined {
  const w = window as unknown as {
    SpeechRecognition?: BrowserRecognitionCtor
    webkitSpeechRecognition?: BrowserRecognitionCtor
  }
  return w.SpeechRecognition ?? w.webkitSpeechRecognition
}

const FALLBACK_KEY = 'voice.useOpenAi'

/** 這次登入是否已改用 OpenAI（瀏覽器出過需要改用的錯誤）。 */
function fallbackLatched(): boolean {
  try {
    return sessionStorage.getItem(FALLBACK_KEY) === '1'
  } catch {
    return false
  }
}

export function latchOpenAiFallback() {
  try {
    sessionStorage.setItem(FALLBACK_KEY, '1')
  } catch {
    // 忽略
  }
}

/** 目前該用哪種辨識：沒有 SpeechRecognition，或這次登入已改用 OpenAI 時用 OpenAI。 */
export function pickRecognitionSource(): RecognitionSource {
  return browserRecognitionCtor() && !fallbackLatched() ? 'speech-browser' : 'speech-openai'
}

export function createBrowserRecognizer(lang: string, ev: RecognizerEvents): Recognizer {
  const Ctor = browserRecognitionCtor()
  let rec: BrowserRecognition | undefined
  let stopped = false
  // 目前這輪辨識已確定的文字（continuous 模式下依序累加）
  let committed = ''

  function begin() {
    if (!Ctor) {
      ev.onError('fallback')
      return
    }
    rec = new Ctor()
    rec.lang = lang
    rec.continuous = true
    rec.interimResults = true
    committed = ''
    rec.onresult = (e) => {
      let interim = ''
      let finals = ''
      for (let i = 0; i < e.results.length; i++) {
        const r = e.results[i]
        if (r.isFinal) finals += r[0].transcript
        else interim += r[0].transcript
      }
      committed = finals
      ev.onText(finals + interim, interim === '')
    }
    rec.onerror = (e) => {
      switch (e.error) {
        case 'network':
        case 'service-not-allowed':
        case 'language-not-supported':
          stopped = true
          latchOpenAiFallback()
          ev.onError('fallback')
          break
        case 'not-allowed':
          stopped = true
          ev.onError('permission')
          break
        case 'no-speech':
        case 'aborted':
          break // 安靜或被我們中斷：不是錯誤
        default:
          stopped = true
          ev.onError('failed')
      }
    }
    rec.onend = () => {
      // 部分瀏覽器在靜音後會自行結束辨識：還沒要停就自動重新開始，避免打斷學生思考
      if (!stopped) {
        try {
          begin()
          rec?.start()
        } catch {
          stopped = true
          ev.onEnd()
        }
        return
      }
      ev.onEnd()
    }
  }

  return {
    source: 'speech-browser',
    start() {
      stopped = false
      begin()
      try {
        rec?.start()
      } catch {
        stopped = true
        ev.onError('failed')
      }
    },
    stop() {
      stopped = true
      if (committed === '') {
        rec?.abort()
      } else {
        rec?.stop()
      }
    },
  }
}

// ---- OpenAI 備援（經後端）----

/** 上傳錄音並取得文字；不保存音訊。 */
export async function transcribeViaBackend(blob: Blob, conversationLang: string): Promise<string> {
  let res: Response
  try {
    res = await fetch(`/api/voice/transcribe?language=${encodeURIComponent(conversationLang)}`, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': blob.type || 'audio/webm', Accept: 'application/json' },
      body: blob,
    })
  } catch {
    throw new ApiError(0, 'network', '')
  }
  if (!res.ok) {
    let code = 'internal'
    try {
      code = (await res.json())?.error?.code ?? code
    } catch {
      // 非 JSON
    }
    throw new ApiError(res.status, code, res.headers.get('x-request-id') ?? '')
  }
  const data = (await res.json()) as { text?: string }
  return (data.text ?? '').trim()
}

function recorderMime(): string | undefined {
  if (typeof MediaRecorder === 'undefined') return undefined
  return ['audio/webm;codecs=opus', 'audio/webm', 'audio/mp4', 'audio/ogg'].find((m) => MediaRecorder.isTypeSupported?.(m))
}

/** 備援辨識：錄音到停止為止，再整段送後端辨識（沒有即時文字，也沒有自動靜音偵測，需手動結束）。 */
export function createOpenAiRecognizer(conversationLang: string, ev: RecognizerEvents): Recognizer {
  let recorder: MediaRecorder | undefined
  let stream: MediaStream | undefined
  let chunks: Blob[] = []
  let cancelled = false

  function release() {
    stream?.getTracks().forEach((t) => t.stop())
    stream = undefined
  }

  return {
    source: 'speech-openai',
    start() {
      cancelled = false
      chunks = []
      if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
        ev.onError('unavailable')
        return
      }
      navigator.mediaDevices
        .getUserMedia({ audio: true })
        .then((s) => {
          if (cancelled) {
            s.getTracks().forEach((t) => t.stop())
            return
          }
          stream = s
          const mime = recorderMime()
          recorder = new MediaRecorder(s, mime ? { mimeType: mime } : undefined)
          recorder.ondataavailable = (e) => {
            if (e.data.size > 0) chunks.push(e.data)
          }
          recorder.onstop = () => {
            release()
            const blob = new Blob(chunks, { type: recorder?.mimeType || mime || 'audio/webm' })
            chunks = []
            if (cancelled || blob.size === 0) {
              ev.onEnd()
              return
            }
            transcribeViaBackend(blob, conversationLang)
              .then((text) => {
                if (text) ev.onText(text, true)
              })
              .catch((e: unknown) => {
                ev.onError(e instanceof ApiError && e.code === 'voice_unavailable' ? 'unavailable' : 'failed')
              })
              .finally(() => ev.onEnd())
          }
          recorder.start()
        })
        .catch(() => ev.onError('permission'))
    },
    stop() {
      if (recorder && recorder.state !== 'inactive') {
        recorder.stop()
      } else {
        cancelled = true
        release()
        ev.onEnd()
      }
    },
  }
}

export function createRecognizer(source: RecognitionSource, speech: string, conversationLang: string, ev: RecognizerEvents): Recognizer {
  return source === 'speech-browser' ? createBrowserRecognizer(speech, ev) : createOpenAiRecognizer(conversationLang, ev)
}
