import { act, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AiJobsProvider, usePollJob } from '@/lib/ai-jobs'
import type { PollSpec } from '@/lib/poll-store'

type Job = { status: 'pending' | 'ready' }

let calls = 0
const spec: PollSpec<Job> = {
  intervalMs: 100,
  maxMs: 10_000,
  fetch: async () => ({ status: ++calls >= 3 ? 'ready' : 'pending' }),
  next: (d) => (d.status === 'pending' ? 'poll-limited' : 'done'),
}

function Watcher() {
  const { state } = usePollJob('job', spec)
  return <p>{state.data?.status ?? 'none'}</p>
}

function Page() {
  const [shown, setShown] = useState(true)
  return (
    <>
      <button onClick={() => setShown((s) => !s)}>toggle</button>
      {shown ? <Watcher /> : null}
    </>
  )
}

describe('usePollJob', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    calls = 0
  })
  afterEach(() => vi.useRealTimers())

  it('keeps polling inside the provider after the component is unmounted, and picks the result up on return', async () => {
    render(<AiJobsProvider><Page /></AiJobsProvider>)
    await act(() => vi.advanceTimersByTimeAsync(0))
    expect(screen.getByText('pending')).toBeInTheDocument()

    act(() => screen.getByText('toggle').click())
    expect(screen.queryByText('pending')).toBeNull()
    await act(() => vi.advanceTimersByTimeAsync(300))
    expect(calls).toBe(3)

    act(() => screen.getByText('toggle').click())
    expect(screen.getByText('ready')).toBeInTheDocument()
  })

  it('without a provider the polling stops when the component unmounts', async () => {
    render(<Page />)
    await act(() => vi.advanceTimersByTimeAsync(0))
    act(() => screen.getByText('toggle').click())
    await act(() => vi.advanceTimersByTimeAsync(1000))
    expect(calls).toBe(1)
  })

  it('clears the previous user results when userKey changes', async () => {
    const { rerender } = render(<AiJobsProvider userKey="a"><Watcher /></AiJobsProvider>)
    await act(() => vi.advanceTimersByTimeAsync(300))
    expect(screen.getByText('ready')).toBeInTheDocument()
    rerender(<AiJobsProvider userKey="b"><Watcher /></AiJobsProvider>)
    await act(() => vi.advanceTimersByTimeAsync(0))
    expect(screen.getByText('none')).toBeInTheDocument()
  })
})
