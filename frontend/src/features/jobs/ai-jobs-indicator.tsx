import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { Skeleton } from '@/components/vedute/skeleton'
import { jobPath, useActiveJobs, type ActiveJob } from '@/features/jobs/active-jobs'
import { useT, type MessageKey, type TFunction } from '@/i18n'

function jobName(t: TFunction, job: ActiveJob) {
  return t(job.label_key as MessageKey)
}

function elapsedText(t: TFunction, startedAt: string, now: number) {
  const secs = Math.max(0, Math.floor((now - Date.parse(startedAt)) / 1000))
  return secs < 60 ? t('jobs.elapsed', { n: secs }) : t('jobs.elapsedMinutes', { n: Math.floor(secs / 60) })
}

/** 每秒更新的目前時間，只在工作清單打開時才需要。 */
function useNow(active: boolean) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [active])
  return now
}

/**
 * 工作從進行中變成完成或失敗時通知使用者（S-20 §4.2）。第一次載入就已結束的工作不通知；
 * 使用者還在來源頁面時只更新畫面、不重複跳通知。
 */
function useFinishedToasts(jobs: ActiveJob[], pathname: string) {
  const t = useT()
  const navigate = useNavigate()
  const seen = useRef<Map<string, ActiveJob['status']>>(new Map())

  useEffect(() => {
    for (const job of jobs) {
      const before = seen.current.get(job.id)
      seen.current.set(job.id, job.status)
      if (before !== 'pending' || job.status === 'pending') continue
      const path = jobPath(job)
      if (pathname === path) continue
      const name = jobName(t, job)
      const action = { label: t('jobs.open'), onClick: () => void navigate(path) }
      if (job.status === 'ready') toast.success(t('jobs.done', { name }), { action })
      else toast.error(t('jobs.failed', { name }), { action })
    }
  }, [jobs, pathname, t, navigate])
}

/**
 * 全域 AI 工作進度（S-20 §4.2）：有進行中的工作時顯示「AI 工作中（N）」，點開看每個工作與經過時間，
 * 並提示可以離開此頁。沒有進行中的工作時不佔位置；結束時由 toast 通知。
 */
export function AiJobsIndicator() {
  const t = useT()
  const { pathname } = useLocation()
  const { jobs } = useActiveJobs(pathname)
  useFinishedToasts(jobs, pathname)
  const pending = jobs.filter((j) => j.status === 'pending')
  const [open, setOpen] = useState(false)
  const now = useNow(open && pending.length > 0)

  if (pending.length === 0) return null
  return (
    <div role="status" aria-live="polite">
      {pending.length > 0 && (
        <div className="border-b border-ink-faint bg-paper px-4 py-2 md:px-7 lg:px-10">
          <button
            type="button"
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
            className="type-control inline-flex items-center gap-2 text-ink"
          >
            <Skeleton className="h-2 w-8" />
            {t('jobs.indicator', { n: pending.length })}
          </button>
          {open && (
            <div className="mt-2">
              <p className="type-note text-ink-soft">{t('jobs.leaveHint')}</p>
              <ul aria-label={t('jobs.panelTitle')} className="mt-1 flex flex-col gap-1">
                {pending.map((job) => (
                  <li key={job.id} className="type-note flex flex-wrap items-baseline gap-x-3">
                    <Link to={jobPath(job)} className="text-ink underline">
                      {jobName(t, job)}
                    </Link>
                    <span className="text-ink-soft">{elapsedText(t, job.started_at, now)}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
