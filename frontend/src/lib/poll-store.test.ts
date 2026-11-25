import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PollStore, type PollSpec } from '@/lib/poll-store'

type Job = { status: 'pending' | 'ready' }

function spec(results: Array<Job | Error>, extra: Partial<PollSpec<Job>> = {}): PollSpec<Job> & { calls: number } {
  const s: PollSpec<Job> & { calls: number } = {
    calls: 0,
    intervalMs: 100,
    maxMs: 1000,
    fetch: async (): Promise<Job> => {
      const r = results[Math.min(s.calls++, results.length - 1)]
      if (r instanceof Error) throw r
      return r
    },
    next: (d: Job) => (d.status === 'pending' ? ('poll-limited' as const) : ('done' as const)),
    ...extra,
  }
  return s
}

describe('PollStore', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('polls until the job is done, then stops', async () => {
    const store = new PollStore()
    const s = spec([{ status: 'pending' }, { status: 'pending' }, { status: 'ready' }])
    store.watch('k', s)
    await vi.advanceTimersByTimeAsync(0)
    expect(store.get('k')).toMatchObject({ data: { status: 'pending' }, polling: true })
    await vi.advanceTimersByTimeAsync(200)
    expect(store.get('k')).toMatchObject({ data: { status: 'ready' }, polling: false, timedOut: false })
    await vi.advanceTimersByTimeAsync(1000)
    expect(s.calls).toBe(3)
  })

  it('keeps polling with no subscribers (the page was left)', async () => {
    const store = new PollStore()
    const s = spec([{ status: 'pending' }, { status: 'pending' }, { status: 'ready' }])
    const off = store.subscribe(() => {})
    store.watch('k', s)
    off()
    await vi.advanceTimersByTimeAsync(300)
    expect(store.get('k')).toMatchObject({ data: { status: 'ready' }, polling: false })
  })

  it('does not start a second loop for a key that is already polling', async () => {
    const store = new PollStore()
    const s = spec([{ status: 'pending' }, { status: 'ready' }])
    store.watch('k', s)
    store.watch('k', s)
    await vi.advanceTimersByTimeAsync(0)
    expect(s.calls).toBe(1)
  })

  it('times out only while the result is time limited', async () => {
    const store = new PollStore()
    store.watch('k', spec([{ status: 'pending' }], { maxMs: 250 }))
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.get('k')).toMatchObject({ timedOut: true, polling: false, data: { status: 'pending' } })
  })

  it('keeps going past maxMs when the result is not time limited', async () => {
    const store = new PollStore()
    const s = spec([{ status: 'pending' }, { status: 'pending' }, { status: 'pending' }, { status: 'ready' }], {
      maxMs: 50,
      next: (d) => (d.status === 'pending' ? 'poll' : 'done'),
    })
    store.watch('k', s)
    await vi.advanceTimersByTimeAsync(500)
    expect(store.get('k')).toMatchObject({ data: { status: 'ready' }, timedOut: false })
  })

  it('records a fetch error and stops', async () => {
    const store = new PollStore()
    const boom = new Error('boom')
    store.watch('k', spec([boom]))
    await vi.advanceTimersByTimeAsync(500)
    expect(store.get('k')).toMatchObject({ error: boom, polling: false })
  })

  it('restart reseeds the data and fetches again immediately', async () => {
    const store = new PollStore()
    const s = spec([{ status: 'ready' }])
    store.watch('k', s)
    await vi.advanceTimersByTimeAsync(0)
    store.restart('k', { data: { status: 'pending' } })
    expect(store.get('k')).toMatchObject({ data: { status: 'pending' }, polling: true })
    await vi.advanceTimersByTimeAsync(0)
    expect(s.calls).toBe(2)
    expect(store.get('k')).toMatchObject({ data: { status: 'ready' }, polling: false })
  })

  it('refresh on watch re-reads an idle entry but leaves a running one alone', async () => {
    const store = new PollStore()
    const s = spec([{ status: 'ready' }])
    store.watch('k', s)
    await vi.advanceTimersByTimeAsync(0)
    store.watch('k', s, { refresh: true })
    await vi.advanceTimersByTimeAsync(0)
    expect(s.calls).toBe(2)
  })

  it('notifies subscribers with a new snapshot on every change', async () => {
    const store = new PollStore()
    const seen: unknown[] = []
    store.subscribe(() => seen.push(store.get('k')))
    store.watch('k', spec([{ status: 'ready' }]))
    await vi.advanceTimersByTimeAsync(0)
    expect(new Set(seen).size).toBe(seen.length)
    expect(seen.length).toBeGreaterThan(1)
  })

  it('dispose aborts every running loop', async () => {
    const store = new PollStore()
    let aborted = false
    store.watch('k', {
      intervalMs: 100,
      maxMs: 1000,
      fetch: (signal) =>
        new Promise((_, reject) => signal.addEventListener('abort', () => { aborted = true; reject(new DOMException('x', 'AbortError')) })),
      next: () => 'done',
    })
    store.dispose()
    expect(aborted).toBe(true)
  })

  it('reports every key that is still polling', async () => {
    const store = new PollStore()
    store.watch('a', spec([{ status: 'pending' }]))
    store.watch('b', spec([{ status: 'ready' }]))
    await vi.advanceTimersByTimeAsync(0)
    expect(store.pollingKeys()).toEqual(['a'])
  })
})
