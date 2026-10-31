/**
 * 播放中的開口偵測（S-05.5）。同時要求三件事，所以不是單純的音量門檻：
 *
 * 1. 比近幾秒的環境音量（含 AI 殘響，取第 90 百分位）高出 `marginDb`
 * 2. 有人聲的週期性（基頻 90–400 Hz），排除鍵盤、風扇等噪音
 * 3. 在 `2 × minVoicedMs` 內累積達 `minVoicedMs`，排除咳嗽、碰撞等短促聲音；
 *    不要求連續，因為語音本來就是音節與子音交替
 *
 * AI 的語音同樣有週期性，週期性分不出是誰在說話，所以第 1 點才是防止被自己打斷的關鍵：
 * 開始的 `calibrationMs` 只學音量，之後也持續追蹤。代價是學生講話若沒有明顯蓋過殘響就不會觸發，
 * 這時「說話」按鈕仍然可用。這是刻意的取捨：寧可漏判，也不要 AI 自己打斷自己。
 *
 * 用百分位而不是最大值：一次短促的聲音不該讓後面幾秒的開口都偵測不到。
 */

export type DetectorConfig = {
  calibrationMs: number
  marginDb: number
  minVoicedMs: number
}

export type DetectorFrame = { samples: Float32Array; sampleRate: number }

const LEVEL_WINDOW_MS = 3000
/** 低於這個音量（dBFS）一律不算開口；要高過 AI 語句間的停頓。 */
const MIN_LEVEL_DB = -50
const SILENCE_DB = -120
const MIN_PERIODICITY = 0.5
const AMBIENT_PERCENTILE = 0.9
const ANALYSIS_RATE = 8000
const MIN_F0 = 90
const MAX_F0 = 400

function analyze({ samples, sampleRate }: DetectorFrame): { levelDb: number; periodicity: number } {
  // 降頻後再算自相關：人聲基頻不需要高取樣率，運算量差一個數量級以上
  const factor = Math.max(1, Math.floor(sampleRate / ANALYSIS_RATE))
  const n = Math.floor(samples.length / factor)
  const x = new Float32Array(n)
  let mean = 0
  for (let i = 0; i < n; i++) {
    let sum = 0
    for (let j = 0; j < factor; j++) sum += samples[i * factor + j]
    x[i] = sum / factor
    mean += x[i]
  }
  mean /= n
  let energy = 0
  for (let i = 0; i < n; i++) {
    x[i] -= mean
    energy += x[i] * x[i]
  }
  if (energy === 0) return { levelDb: SILENCE_DB, periodicity: 0 }
  const levelDb = 10 * Math.log10(energy / n)

  const rate = sampleRate / factor
  const minLag = Math.floor(rate / MAX_F0)
  const maxLag = Math.min(Math.ceil(rate / MIN_F0), n - 1)
  let periodicity = 0
  for (let lag = minLag; lag <= maxLag; lag++) {
    let cross = 0
    let a = 0
    let b = 0
    for (let i = 0; i + lag < n; i++) {
      cross += x[i] * x[i + lag]
      a += x[i] * x[i]
      b += x[i + lag] * x[i + lag]
    }
    if (a > 0 && b > 0) periodicity = Math.max(periodicity, cross / Math.sqrt(a * b))
  }
  return { levelDb, periodicity }
}

export type SpeechDetector = {
  /** 餵一幀；判定開口的那一幀回傳 `true`，之後都回傳 `false`。 */
  push: (frame: DetectorFrame) => boolean
}

export function createSpeechDetector({ calibrationMs, marginDb, minVoicedMs }: DetectorConfig): SpeechDetector {
  const levels: { at: number; db: number }[] = []
  // 疑似開口、尚未定論的幀：先不算進環境音量，否則學生的聲音會墊高自己的門檻
  const candidates: { at: number; db: number; ms: number }[] = []
  let elapsedMs = 0
  let candidateMs = 0
  let fired = false

  const remember = (at: number, db: number) => {
    levels.push({ at, db })
    while (levels[0].at < elapsedMs - LEVEL_WINDOW_MS) levels.shift()
  }
  const ambientDb = () => {
    const sorted = levels.map((l) => l.db).sort((a, b) => a - b)
    return sorted[Math.floor((sorted.length - 1) * AMBIENT_PERCENTILE)]
  }

  return {
    push(frame) {
      if (fired) return false
      const frameMs = (frame.samples.length / frame.sampleRate) * 1000
      elapsedMs += frameMs
      const { levelDb, periodicity } = analyze(frame)
      if (elapsedMs <= calibrationMs) {
        remember(elapsedMs, levelDb)
        return false
      }

      // 過期還沒定論：這個音量算是環境的一部分，反覆出現的聲音才不會在之後觸發
      while (candidates.length > 0 && candidates[0].at < elapsedMs - 2 * minVoicedMs) {
        const old = candidates.shift()
        if (old) {
          candidateMs -= old.ms
          remember(old.at, old.db)
        }
      }

      if (levelDb < Math.max(ambientDb() + marginDb, MIN_LEVEL_DB) || periodicity < MIN_PERIODICITY) {
        remember(elapsedMs, levelDb)
        return false
      }
      candidates.push({ at: elapsedMs, db: levelDb, ms: frameMs })
      candidateMs += frameMs
      fired = candidateMs >= minVoicedMs
      return fired
    },
  }
}
