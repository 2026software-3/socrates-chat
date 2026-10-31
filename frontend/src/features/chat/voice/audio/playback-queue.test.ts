import { describe, expect, it, vi } from 'vitest'
import { createPlaybackQueue } from '@/features/chat/voice/audio/playback-queue'

/** 可手動結束每個片段的播放端。 */
function sink() {
  const played: string[] = []
  const signals: AbortSignal[] = []
  const pending: { resolve: () => void; reject: (e: unknown) => void }[] = []
  const play = (chunk: string, signal: AbortSignal) =>
    new Promise<void>((resolve, reject) => {
      played.push(chunk)
      signals.push(signal)
      pending.push({ resolve, reject })
    })
  const tick = () => new Promise((r) => setTimeout(r, 0))
  const endCurrent = async () => {
    pending.shift()?.resolve()
    await tick()
  }
  return { played, signals, pending, play, endCurrent, tick }
}

describe('playback queue', () => {
  it('plays chunks of a response in order, one at a time', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'a')
    q.enqueue('r1', 'b')
    const done = q.finish('r1')
    await s.tick()
    expect(s.played).toEqual(['a'])
    await s.endCurrent()
    expect(s.played).toEqual(['a', 'b'])
    await s.endCurrent()
    await expect(done).resolves.toBe('finished')
  })

  it('accepts chunks that arrive while the previous one plays', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'a')
    await s.tick()
    await s.endCurrent()
    q.enqueue('r1', 'b')
    await s.tick()
    expect(s.played).toEqual(['a', 'b'])
    const done = q.finish('r1')
    await s.endCurrent()
    await expect(done).resolves.toBe('finished')
  })

  it('interrupt aborts the current chunk and drops the queued ones immediately', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'a')
    q.enqueue('r1', 'b')
    const done = q.finish('r1')
    await s.tick()
    q.interrupt()
    expect(s.signals[0].aborted).toBe(true)
    await expect(done).resolves.toBe('interrupted')
    await s.endCurrent()
    expect(s.played).toEqual(['a'])
  })

  it('refuses a late chunk from an interrupted response', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'a')
    q.interrupt()
    expect(q.enqueue('r1', 'late')).toBe(false)
    await s.tick()
    expect(s.played).not.toContain('late')
  })

  it('a new response cancels the old one, and old chunks cannot sneak in', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'old')
    const first = q.finish('r1')
    await s.tick()
    q.begin('r2')
    await expect(first).resolves.toBe('interrupted')
    expect(q.enqueue('r1', 'stale')).toBe(false)
    expect(q.enqueue('r2', 'new')).toBe(true)
    await s.endCurrent() // 舊片段這時才結束
    await s.tick()
    expect(s.played).toEqual(['old', 'new'])
  })

  it('a stale chunk finishing late does not end the current response', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'old')
    await s.tick()
    q.begin('r2')
    q.enqueue('r2', 'new')
    const second = q.finish('r2')
    const settled = vi.fn()
    void second.then(settled)
    await s.tick()
    s.pending[0].reject(new Error('aborted'))
    await s.tick()
    expect(settled).not.toHaveBeenCalled()
    s.pending[1].resolve()
    await expect(second).resolves.toBe('finished')
  })

  it('a failed chunk rejects finish and drops the rest', async () => {
    const s = sink()
    const q = createPlaybackQueue(s.play)
    q.begin('r1')
    q.enqueue('r1', 'a')
    q.enqueue('r1', 'b')
    const done = q.finish('r1')
    await s.tick()
    s.pending[0].reject(new Error('boom'))
    await expect(done).rejects.toThrow('boom')
    await s.tick()
    expect(s.played).toEqual(['a'])
  })

  it('finish with no chunks resolves at once', async () => {
    const q = createPlaybackQueue<string>(() => Promise.resolve())
    q.begin('r1')
    await expect(q.finish('r1')).resolves.toBe('finished')
  })

  it('finish for a response that is not current reports interrupted', async () => {
    const q = createPlaybackQueue<string>(() => Promise.resolve())
    await expect(q.finish('gone')).resolves.toBe('interrupted')
  })
})
