/**
 * 語音合成（S-05.2、S-05.3）。預設用瀏覽器的 speechSynthesis；沒有 speechSynthesis，
 * 或找不到對話語言的語音時，改用 OpenAI（經後端 `/api/voice/speech`，前端不接觸 OpenAI）。
 * 找語音的順序：完全相符的地區，其次是同語言的其他地區（例如 `zh-*`）。
 *
 * 播放都經過 `PlaybackQueue`：打斷後，等待中的語音載入或網路請求都不會再發出聲音（S-05.5）。
 */
import type { PlaybackPath } from '@/features/chat/voice/audio/barge-in'
import { createPlaybackQueue } from '@/features/chat/voice/audio/playback-queue'
import { isAbortError } from '@/lib/api'

export type Speaker = {
  /**
   * 播放文字；播完時 resolve，被 `stop` 中斷也會 resolve（`false`）。
   * `onStart` 在真正開始發聲時呼叫，並告知發聲途徑。
   */
  speak: (text: string, onStart?: (path: PlaybackPath) => void) => Promise<boolean>
  stop: () => void
}

function findVoice(voices: SpeechSynthesisVoice[], lang: string): SpeechSynthesisVoice | undefined {
  const norm = (l: string) => l.replace('_', '-').toLowerCase()
  const exact = voices.find((v) => norm(v.lang) === norm(lang))
  if (exact) return exact
  const base = norm(lang).split('-')[0]
  return voices.find((v) => norm(v.lang).split('-')[0] === base)
}

/** 語音清單常常是非同步載入的：空的時候最多等一下 `voiceschanged`。 */
async function loadVoices(synth: SpeechSynthesis): Promise<SpeechSynthesisVoice[]> {
  const now = synth.getVoices()
  if (now.length > 0) return now
  return new Promise((resolve) => {
    const done = () => {
      synth.removeEventListener?.('voiceschanged', done)
      resolve(synth.getVoices())
    }
    synth.addEventListener?.('voiceschanged', done)
    setTimeout(done, 500)
  })
}

export class SpeechUnavailableError extends Error {}

type Chunk = { text: string; onStart?: (path: PlaybackPath) => void }

export function createSpeaker(lang: string, conversationLang: string): Speaker {
  /** 回傳 `false` 表示這個瀏覽器不能唸，要改用後端。 */
  async function speakWithBrowser({ text, onStart }: Chunk, signal: AbortSignal): Promise<boolean> {
    const synth = typeof window !== 'undefined' ? window.speechSynthesis : undefined
    if (!synth || typeof SpeechSynthesisUtterance === 'undefined') return false
    const voice = findVoice(await loadVoices(synth), lang)
    if (!voice) return false
    if (signal.aborted) return true
    await new Promise<void>((resolve) => {
      const u = new SpeechSynthesisUtterance(text)
      u.voice = voice
      u.lang = voice.lang
      const onAbort = () => synth.cancel()
      const done = () => {
        signal.removeEventListener('abort', onAbort)
        resolve()
      }
      signal.addEventListener('abort', onAbort, { once: true })
      // 取消後才到的 start 事件不算開始發聲
      u.onstart = () => {
        if (!signal.aborted) onStart?.('synthesis')
      }
      u.onend = done
      u.onerror = done
      synth.speak(u)
    })
    return true
  }

  async function speakWithBackend({ text, onStart }: Chunk, signal: AbortSignal): Promise<void> {
    let url: string | undefined
    try {
      const res = await fetch('/api/voice/speech', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text, language: conversationLang }),
        signal,
      })
      if (!res.ok) throw new SpeechUnavailableError()
      url = URL.createObjectURL(await res.blob())
      if (signal.aborted) return
      const src = url
      await new Promise<void>((resolve, reject) => {
        const audio = new Audio(src)
        signal.addEventListener(
          'abort',
          () => {
            audio.pause()
            resolve()
          },
          { once: true },
        )
        audio.onended = () => resolve()
        audio.onerror = () => reject(new SpeechUnavailableError())
        audio.play().then(() => {
          if (!signal.aborted) onStart?.('element')
        }, reject)
      })
    } catch (e) {
      if (signal.aborted || isAbortError(e)) return
      throw e instanceof SpeechUnavailableError ? e : new SpeechUnavailableError()
    } finally {
      if (url) URL.revokeObjectURL(url)
    }
  }

  const queue = createPlaybackQueue<Chunk>(async (chunk, signal) => {
    if (!(await speakWithBrowser(chunk, signal)) && !signal.aborted) await speakWithBackend(chunk, signal)
  })
  let turn = 0

  return {
    async speak(text, onStart) {
      const id = String(++turn)
      queue.begin(id)
      queue.enqueue(id, { text, onStart })
      return (await queue.finish(id)) === 'finished'
    },
    stop() {
      queue.interrupt()
    },
  }
}
