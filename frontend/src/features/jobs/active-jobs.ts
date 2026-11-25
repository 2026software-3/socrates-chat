import { useCallback, useEffect, useMemo, useRef } from 'react'
import { api } from '@/lib/api'
import { AI_JOBS_WAKE_EVENT } from '@/lib/ai-jobs-events'
import { AI_USAGE_STALE_EVENT } from '@/lib/ai-usage-events'
import { usePollJob } from '@/lib/ai-jobs'
import type { PollSpec } from '@/lib/poll-store'

export type ActiveJob = {
  id: string
  kind: 'reply' | 'summary' | 'radar' | 'argument_tree' | 'summary_translation' | 'regroup' | 'summary_translation_view'
  status: 'pending' | 'ready' | 'failed'
  label_key: string
  /** 只有識別碼；背景工作（`regroup` 等）帶的是活動、題目與教室。 */
  scope: { conversation_id?: string; activity_id?: string; topic_id?: string; classroom_id?: string }
  started_at: string
  finished_at: string | null
  retryable: boolean
}
type ActiveJobs = { jobs: ActiveJob[] }

/** 有進行中的工作時的查詢間隔；沒有時不輪詢，只在被喚醒時查一次。 */
const ACTIVE_POLL_MS = 2000
/** 收到喚醒後稍等一下再查：一次操作常連續發出好幾個通知。 */
const WAKE_DELAY_MS = 300

/** 工作的來源頁面；完成通知的連結與「使用者已在該頁」的判斷都用它。 */
export function jobPath(job: ActiveJob): string {
  const { classroom_id, activity_id } = job.scope
  if (job.kind === 'regroup' && classroom_id && activity_id) return `/teacher/classrooms/${classroom_id}/activities/${activity_id}`
  if (job.kind === 'summary_translation_view' && classroom_id) return `/teacher/classrooms/${classroom_id}/conversations/${job.scope.conversation_id}`
  return `/conversations/${job.scope.conversation_id}`
}

/**
 * 目前使用者進行中與剛結束的 AI 工作（`GET /api/ai-jobs/active`，S-20）。
 * 輪詢在全域登記處進行；沒有進行中的工作就停下來，換頁、開始新工作（喚醒事件）、AI 用量變動時再查一次。
 */
export function useActiveJobs(pathname: string) {
  const spec = useMemo<PollSpec<ActiveJobs>>(
    () => ({
      fetch: (signal) => api.get<ActiveJobs>('/api/ai-jobs/active', signal),
      next: (d) => (d.jobs.some((j) => j.status === 'pending') ? 'poll' : 'done'),
      intervalMs: ACTIVE_POLL_MS,
      maxMs: Number.POSITIVE_INFINITY,
    }),
    [],
  )
  const { state, restart } = usePollJob('active-jobs', spec)
  const refresh = useCallback(() => restart({ keep: true }), [restart])

  // 換頁時查一次（第一次不用：掛載時 usePollJob 已經查過）
  const lastPath = useRef(pathname)
  useEffect(() => {
    if (lastPath.current === pathname) return
    lastPath.current = pathname
    refresh()
  }, [pathname, refresh])

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    const wake = () => {
      clearTimeout(timer)
      timer = setTimeout(refresh, WAKE_DELAY_MS)
    }
    window.addEventListener(AI_JOBS_WAKE_EVENT, wake)
    window.addEventListener(AI_USAGE_STALE_EVENT, wake)
    return () => {
      window.removeEventListener(AI_JOBS_WAKE_EVENT, wake)
      window.removeEventListener(AI_USAGE_STALE_EVENT, wake)
      clearTimeout(timer)
    }
  }, [refresh])

  const jobs = useMemo(() => state.data?.jobs ?? [], [state.data])
  return { jobs, refresh }
}
