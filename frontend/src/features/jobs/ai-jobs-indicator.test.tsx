import { act, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AiJobsIndicator } from '@/features/jobs/ai-jobs-indicator'
import type { ActiveJob } from '@/features/jobs/active-jobs'
import { wakeAiJobs } from '@/lib/ai-jobs-events'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'

const { toast } = vi.hoisted(() => ({ toast: { success: vi.fn(), error: vi.fn() } }))
vi.mock('sonner', () => ({ toast }))

const C1 = '11111111-1111-4111-8111-111111111111'
const C2 = '22222222-2222-4222-8222-222222222222'

function job(kind: ActiveJob['kind'], conv: string, status: ActiveJob['status'], started = new Date().toISOString()): ActiveJob {
  return {
    id: `${kind}:${conv}`,
    kind,
    status,
    label_key: `jobs.${kind}`,
    scope: { conversation_id: conv },
    started_at: started,
    finished_at: status === 'pending' ? null : started,
    retryable: status === 'failed',
  }
}

function serve(responses: ActiveJob[][]) {
  let calls = 0
  server.use(
    http.get('/api/ai-jobs/active', () => HttpResponse.json({ jobs: responses[Math.min(calls++, responses.length - 1)] })),
  )
  return { calls: () => calls }
}

async function wake() {
  await act(async () => {
    wakeAiJobs()
    await new Promise((r) => setTimeout(r, 450))
  })
}

describe('AiJobsIndicator', () => {
  beforeEach(() => {
    toast.success.mockClear()
    toast.error.mockClear()
  })

  it('lists jobs that are not tied to a conversation (achievement translation) with a label and a working link', async () => {
    serve([[{
      id: 'j-ach', kind: 'achievement_translation', status: 'pending', label_key: 'jobs.achievement_translation',
      scope: { achievement_id: 'new_insight' }, started_at: new Date().toISOString(), finished_at: null, retryable: false,
    }]])
    renderApp(<AiJobsIndicator />)
    await userEvent.click(await screen.findByRole('button', { name: /AI 工作中/ }))
    const link = await screen.findByRole('link', { name: /成就翻譯/ })
    expect(link).toHaveAttribute('href', '/admin/achievements')
  })

  it('shows nothing when no AI work is running', async () => {
    const s = serve([[]])
    renderApp(<AiJobsIndicator />)
    await waitFor(() => expect(s.calls()).toBe(1))
    expect(screen.queryByText(/AI 工作中/)).toBeNull()
  })

  it('shows the running count, and lists each job with a link and the leave-the-page hint when opened', async () => {
    serve([[job('summary', C1, 'pending'), job('radar', C1, 'pending'), job('summary', C2, 'ready')]])
    renderApp(<AiJobsIndicator />)
    const button = await screen.findByRole('button', { name: /AI 工作中（2）/ })
    expect(button).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(button)
    expect(screen.getByText('可以離開此頁，完成後會通知你。')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '對話總結' })).toHaveAttribute('href', `/conversations/${C1}`)
    expect(screen.getByRole('link', { name: '雷達評分' })).toBeInTheDocument()
  })

  it('announces through a polite live region', async () => {
    serve([[job('summary', C1, 'pending')]])
    renderApp(<AiJobsIndicator />)
    await screen.findByRole('button', { name: /AI 工作中（1）/ })
    expect(screen.getByRole('status')).toHaveAttribute('aria-live', 'polite')
  })

  it('notifies when a running job finishes while the user is on another page', async () => {
    serve([[job('summary', C1, 'pending')], [job('summary', C1, 'ready')]])
    renderApp(<AiJobsIndicator />, { route: '/somewhere-else' })
    await screen.findByRole('button', { name: /AI 工作中（1）/ })
    await wake()
    await waitFor(() => expect(toast.success).toHaveBeenCalledTimes(1))
    expect(toast.success.mock.calls[0][0]).toBe('對話總結已完成')
    expect(screen.queryByText(/AI 工作中/)).toBeNull()
  })

  it('notifies about a failed job with an error toast', async () => {
    serve([[job('argument_tree', C1, 'pending')], [job('argument_tree', C1, 'failed')]])
    renderApp(<AiJobsIndicator />)
    await screen.findByRole('button', { name: /AI 工作中（1）/ })
    await wake()
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('論證樹失敗', expect.anything()))
  })

  it('does not notify when the user is already on the source page', async () => {
    serve([[job('summary', C1, 'pending')], [job('summary', C1, 'ready')]])
    renderApp(<AiJobsIndicator />, { route: `/conversations/${C1}` })
    await screen.findByRole('button', { name: /AI 工作中（1）/ })
    await wake()
    await waitFor(() => expect(screen.queryByText(/AI 工作中/)).toBeNull())
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('does not notify about work that was already finished when first loaded', async () => {
    serve([[job('summary', C1, 'ready'), job('radar', C1, 'failed')]])
    renderApp(<AiJobsIndicator />)
    await waitFor(() => expect(toast.success).not.toHaveBeenCalled())
    await wake()
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('keeps polling while something is running and stops once nothing is', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      const s = serve([[job('summary', C1, 'pending')], [job('summary', C1, 'pending')], [job('summary', C1, 'ready')]])
      renderApp(<AiJobsIndicator />)
      await screen.findByRole('button', { name: /AI 工作中/ })
      await act(() => vi.advanceTimersByTimeAsync(2100))
      await act(() => vi.advanceTimersByTimeAsync(2100))
      await waitFor(() => expect(screen.queryByText(/AI 工作中/)).toBeNull())
      const before = s.calls()
      await act(() => vi.advanceTimersByTimeAsync(10_000))
      expect(s.calls()).toBe(before)
    } finally {
      vi.useRealTimers()
    }
  })
})
