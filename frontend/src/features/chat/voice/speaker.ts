/**
 * 語音合成（S-05.2、S-05.3）。預設用瀏覽器的 speechSynthesis；沒有 speechSynthesis，
 * 或找不到對話語言的語音時，改用 OpenAI（經後端 `/api/voice/speech`，前端不接觸 OpenAI）。
 * 找語音的順序：完全相符的地區，其次是同語言的其他地區（例如 `zh-*`）。
 */
export type Speaker = {
  /** 播放文字；播完時 resolve，被 `stop` 中斷也會 resolve（`false`）。 */
  speak: (text: string) => Promise<boolean>
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

export function createSpeaker(lang: string, conversationLang: string): Speaker {
  let cancel: (() => void) | undefined

  async function speakWithBrowser(text: string): Promise<boolean | undefined> {
    const synth = typeof window !== 'undefined' ? window.speechSynthesis : undefined
    if (!synth || typeof SpeechSynthesisUtterance === 'undefined') return undefined
    const voice = findVoice(await loadVoices(synth), lang)
    if (!voice) return undefined
    return new Promise<boolean>((resolve) => {
      const u = new SpeechSynthesisUtterance(text)
      u.voice = voice
      u.lang = voice.lang
      let finished = false
      const finish = (ok: boolean) => {
        if (finished) return
        finished = true
        cancel = undefined
        resolve(ok)
      }
      u.onend = () => finish(true)
      u.onerror = (e) => finish(e.error === 'canceled' || e.error === 'interrupted' ? false : true)
      cancel = () => {
        synth.cancel()
        finish(false)
      }
      synth.speak(u)
    })
  }

  async function speakWithBackend(text: string): Promise<boolean> {
    const ctrl = new AbortController()
    let audio: HTMLAudioElement | undefined
    let url: string | undefined
    let settled = false
    return new Promise<boolean>((resolve, reject) => {
      const finish = (ok: boolean) => {
        if (settled) return
        settled = true
        cancel = undefined
        if (url) URL.revokeObjectURL(url)
        resolve(ok)
      }
      cancel = () => {
        ctrl.abort()
        audio?.pause()
        finish(false)
      }
      fetch('/api/voice/speech', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text, language: conversationLang }),
        signal: ctrl.signal,
      })
        .then(async (res) => {
          if (!res.ok) throw new SpeechUnavailableError()
          url = URL.createObjectURL(await res.blob())
          audio = new Audio(url)
          audio.onended = () => finish(true)
          audio.onerror = () => reject(new SpeechUnavailableError())
          await audio.play()
        })
        .catch((e: unknown) => {
          if (settled) return
          if (e instanceof DOMException && e.name === 'AbortError') return finish(false)
          settled = true
          cancel = undefined
          reject(e instanceof SpeechUnavailableError ? e : new SpeechUnavailableError())
        })
    })
  }

  return {
    async speak(text) {
      const viaBrowser = await speakWithBrowser(text)
      if (viaBrowser !== undefined) return viaBrowser
      return speakWithBackend(text)
    },
    stop() {
      cancel?.()
    },
  }
}
