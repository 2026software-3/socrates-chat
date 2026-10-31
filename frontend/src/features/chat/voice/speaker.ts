/**
 * 語音合成（S-05.2、S-05.3）。預設用瀏覽器的 speechSynthesis；沒有 speechSynthesis，
 * 或找不到對話語言的語音時，改用 OpenAI（經後端 `/api/voice/speech`，前端不接觸 OpenAI）。
 * 找語音的順序：完全相符的地區，其次是同語言的其他地區（例如 `zh-*`）。
 *
 * 播放都經過 `PlaybackQueue`：打斷後，等待中的語音載入或網路請求都不會再發出聲音（S-05.5）。
 *
 * 後端合成每次都扣一筆 AI 用量，所以逐句送進來時，播放期間累積的句子會合併成一次請求；
 * 瀏覽器語音不計費，一句一句唸。
 */
import type { PlaybackPath } from '@/features/chat/voice/audio/barge-in'
import { createPlaybackQueue } from '@/features/chat/voice/audio/playback-queue'
import { markAiUsageStale } from '@/lib/ai-usage-events'
import { isAbortError } from '@/lib/api'

/** 邊生成邊唸的一則回覆。 */
export type SpeechTurn = {
  /** 加入一段文字；這一輪已被打斷時回傳 `false`，文字被丟棄。 */
  say: (text: string) => boolean
  /** 不會再有新的文字；唸完後 `finished` 才會 resolve。 */
  end: () => void
  /** 全部唸完時為 `true`，被 `stop` 或下一輪中斷時為 `false`；播放失敗時 reject。 */
  finished: Promise<boolean>
}

export type Speaker = {
  /** 開始一輪；`onStart` 在這一輪真正開始發聲時呼叫一次，並告知發聲途徑。 */
  open: (onStart?: (path: PlaybackPath) => void) => SpeechTurn
  /** 一次唸完整段文字；結果同 `SpeechTurn.finished`。 */
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
      }).finally(markAiUsageStale)
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

  let usesBackend = false
  const queue = createPlaybackQueue<Chunk>(
    async (chunk, signal) => {
      usesBackend = !(await speakWithBrowser(chunk, signal))
      if (usesBackend && !signal.aborted) await speakWithBackend(chunk, signal)
    },
    { merge: (chunk, next) => (usesBackend ? { ...chunk, text: `${chunk.text} ${next.text}` } : undefined) },
  )
  let turn = 0

  function open(onStart?: (path: PlaybackPath) => void): SpeechTurn {
    const id = String(++turn)
    const outcome = queue.begin(id)
    let started = false
    const startOnce = (path: PlaybackPath) => {
      if (started) return
      started = true
      onStart?.(path)
    }
    return {
      say: (text) => queue.enqueue(id, { text, onStart: startOnce }),
      end: () => void queue.finish(id),
      finished: outcome.then((o) => o === 'finished'),
    }
  }

  return {
    open,
    speak(text, onStart) {
      const speech = open(onStart)
      speech.say(text)
      speech.end()
      return speech.finished
    },
    stop() {
      queue.interrupt()
    },
  }
}
