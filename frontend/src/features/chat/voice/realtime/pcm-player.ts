/** 把串流來的 PCM 接續排程到 Web Audio；只負責發聲，不知道回合或打斷的存在。 */

/** 第一段的緩衝：太短會在網路抖動時斷音。 */
const START_LEAD_S = 0.1

type Scheduled = { source: AudioBufferSourceNode; start: number; duration: number }

export type PcmPlayer = {
  push: (samples: Int16Array) => void
  /** 立刻停止並丟掉所有片段，聽到的時間歸零；不會觸發 `onIdle`。 */
  clear: () => void
  busy: () => boolean
  /** 這一輪實際唸出的毫秒數，不含片段之間斷音的時間。 */
  playedMs: () => number
}

export function createPcmPlayer(ctx: AudioContext, sampleRate: number, onIdle: () => void): PcmPlayer {
  let scheduled: Scheduled[] = []
  let playing = 0
  let nextStart = 0

  return {
    push(samples) {
      const floats = Float32Array.from(samples, (s) => s / 32768)
      const buffer = ctx.createBuffer(1, floats.length, sampleRate)
      buffer.copyToChannel(floats, 0)
      const source = ctx.createBufferSource()
      source.buffer = buffer
      source.connect(ctx.destination)
      const start = Math.max(nextStart, ctx.currentTime + START_LEAD_S)
      nextStart = start + buffer.duration
      source.onended = () => {
        source.disconnect()
        playing -= 1
        if (playing === 0) onIdle()
      }
      playing += 1
      scheduled.push({ source, start, duration: buffer.duration })
      source.start(start)
    },
    clear() {
      for (const { source } of scheduled) {
        source.onended = null
        source.stop()
        source.disconnect()
      }
      scheduled = []
      playing = 0
      nextStart = 0
    },
    busy: () => playing > 0,
    playedMs() {
      const now = ctx.currentTime
      const seconds = scheduled.reduce((sum, c) => sum + Math.min(Math.max(now - c.start, 0), c.duration), 0)
      return seconds * 1000
    },
  }
}
