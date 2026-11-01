import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPcmPlayer } from '@/features/chat/voice/realtime/pcm-player'

class FakeSource {
  buffer: { duration: number } | null = null
  onended: (() => void) | null = null
  connect = vi.fn()
  disconnect = vi.fn()
  start = vi.fn()
  stop = vi.fn()
}

function fakeContext() {
  const sources: FakeSource[] = []
  const ctx = {
    currentTime: 10,
    destination: {},
    createBuffer: (_channels: number, length: number, rate: number) => ({
      duration: length / rate,
      copyToChannel: vi.fn(),
    }),
    createBufferSource: () => {
      const source = new FakeSource()
      sources.push(source)
      return source
    },
  }
  return { ctx: ctx as unknown as AudioContext, raw: ctx, sources }
}

const chunk = (ms: number) => new Int16Array((24_000 * ms) / 1000)

describe('createPcmPlayer', () => {
  let t: ReturnType<typeof fakeContext>
  let onIdle: () => void

  beforeEach(() => {
    t = fakeContext()
    onIdle = vi.fn()
  })

  it('schedules chunks back to back without gaps', () => {
    const player = createPcmPlayer(t.ctx, 24_000, onIdle)
    player.push(chunk(100))
    player.push(chunk(100))
    const first = t.sources[0].start.mock.calls[0][0] as number
    const second = t.sources[1].start.mock.calls[0][0] as number
    expect(first).toBeGreaterThanOrEqual(10)
    expect(second).toBeCloseTo(first + 0.1)
  })

  it('reports idle only after the last chunk ended', () => {
    const player = createPcmPlayer(t.ctx, 24_000, onIdle)
    player.push(chunk(100))
    player.push(chunk(100))
    t.sources[0].onended?.()
    expect(onIdle).not.toHaveBeenCalled()
    t.sources[1].onended?.()
    expect(onIdle).toHaveBeenCalledTimes(1)
    expect(player.busy()).toBe(false)
  })

  it('restarts from the current time after an underrun', () => {
    const player = createPcmPlayer(t.ctx, 24_000, onIdle)
    player.push(chunk(100))
    t.sources[0].onended?.()
    t.raw.currentTime = 20
    player.push(chunk(100))
    expect(t.sources[1].start.mock.calls[0][0]).toBeGreaterThanOrEqual(20)
  })

  it('clear stops everything at once and does not report idle', () => {
    const player = createPcmPlayer(t.ctx, 24_000, onIdle)
    player.push(chunk(100))
    player.push(chunk(100))
    player.clear()
    expect(t.sources[0].stop).toHaveBeenCalled()
    expect(t.sources[1].stop).toHaveBeenCalled()
    t.sources[0].onended?.()
    expect(onIdle).not.toHaveBeenCalled()
    expect(player.busy()).toBe(false)
  })

  it('counts only the audio that was actually played, ignoring underrun gaps', () => {
    const player = createPcmPlayer(t.ctx, 24_000, onIdle)
    player.push(chunk(100))
    const start = t.sources[0].start.mock.calls[0][0] as number
    t.raw.currentTime = start + 0.05
    expect(player.playedMs()).toBeCloseTo(50)
    t.raw.currentTime = start + 5 // 第一段早已播完，第二段還沒開始
    t.sources[0].onended?.()
    player.push(chunk(100))
    expect(player.playedMs()).toBeCloseTo(100)
  })

  it('starts counting again after reset', () => {
    const player = createPcmPlayer(t.ctx, 24_000, onIdle)
    player.push(chunk(100))
    t.raw.currentTime = 30
    player.clear()
    expect(player.playedMs()).toBe(0)
  })
})
