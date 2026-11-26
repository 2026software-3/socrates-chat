import { useEffect, useMemo, useRef, useState } from 'react'
import { api, ApiError } from '@/lib/api'
import { usePollJob, usePollState } from '@/lib/ai-jobs'
import type { PollSpec } from '@/lib/poll-store'
import type { ActiveJob } from './active-jobs'

type JobDetail = { status: string; error_code?: string }
export type JobOutcome = { ok: true } | { ok: false; error: unknown }

/**
 * 看著一個背景工作（`ai_jobs`，S-20）直到結束。`watch(id)` 接上剛登記的工作；
 * 元件在工作進行中被卸載、之後又掛載時，會從全域進行中清單（`kind` 與 `matches` 吻合的那一筆）接回同一個工作。
 * 結束時呼叫 `onSettled` 一次：成功，或帶著錯誤（工作失敗時為固定代碼的 `ApiError`）。
 */
export function useWatchedJob(
  kind: ActiveJob['kind'],
  matches: (scope: ActiveJob['scope']) => boolean,
  onSettled: (outcome: JobOutcome) => void,
) {
  const [jobId, setJobId] = useState<string | null>(null)
  const handled = useRef(new Set<string>())
  const settled = useRef(onSettled)
  settled.current = onSettled
  const matchRef = useRef(matches)
  matchRef.current = matches

  const active = usePollState<{ jobs: ActiveJob[] }>('active-jobs')
  const activeJob = (active.data?.jobs ?? []).find((j) => j.kind === kind && j.status === 'pending' && matchRef.current(j.scope))
  useEffect(() => {
    if (jobId === null && activeJob && !handled.current.has(activeJob.id)) setJobId(activeJob.id)
  }, [jobId, activeJob])

  const spec = useMemo<PollSpec<JobDetail>>(
    () => ({
      fetch: (signal) => api.get<JobDetail>(`/api/ai-jobs/${jobId}`, signal),
      next: (d) => (d.status === 'pending' ? 'poll' : 'done'),
      intervalMs: 2000,
      maxMs: 600_000,
    }),
    [jobId],
  )
  const { state: job } = usePollJob(jobId ? `job:${jobId}` : null, spec)

  useEffect(() => {
    if (jobId === null) return
    const finish = (outcome: JobOutcome) => {
      handled.current.add(jobId)
      setJobId(null)
      settled.current(outcome)
    }
    if (job.error !== undefined) finish({ ok: false, error: job.error })
    else if (job.data?.status === 'failed') finish({ ok: false, error: new ApiError(502, job.data.error_code ?? 'ai_unavailable', '') })
    else if (job.data?.status === 'ready') finish({ ok: true })
  }, [jobId, job])

  return { running: jobId !== null, watch: setJobId }
}
