import { useState } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'

export type Task = { id: string; period: 'daily' | 'weekly'; goal: number; reward: number }
type TaskView = Task & { period_start: string; progress: number; claimed: boolean }

export function TasksPage() {
  const t = useT()
  const { data, loading, error, reload } = useFetch<TaskView[]>('/api/tasks')
  const [busy, setBusy] = useState<string>()
  const [claimError, setClaimError] = useState<unknown>()

  async function claim(task: TaskView) {
    if (busy) return
    setBusy(task.id)
    setClaimError(undefined)
    try {
      await api.post(`/api/tasks/${encodeURIComponent(task.id)}/claim`, { period_start: task.period_start })
      reload()
    } catch (err) {
      setClaimError(err)
      reload()
    } finally {
      setBusy(undefined)
    }
  }

  return <Page title={t('tasks.title')}>
    {loading && !data ? <LoadingBlock /> : null}
    {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
    {claimError ? <ErrorBlock error={claimError} onRetry={() => { setClaimError(undefined); reload() }} /> : null}
    {data?.length === 0 ? <EmptyBlock>{t('tasks.empty')}</EmptyBlock> : null}
    {data?.map((task) => <section key={task.id} className="space-y-2 rounded-md border p-4">
      <h2 className="font-medium">{t(task.period === 'daily' ? 'tasks.daily' : 'tasks.weekly')}</h2>
      <p>{t('tasks.progress', { current: task.progress, goal: task.goal })}</p>
      <p>{t('tasks.reward', { points: task.reward })}</p>
      {task.claimed ? <span>{t('tasks.claimed')}</span> : task.progress < task.goal ? <span>{t('tasks.incomplete')}</span> :
        <Button disabled={Boolean(busy)} onClick={() => void claim(task)}>{busy === task.id ? t('tasks.claiming') : t('tasks.claim')}</Button>}
    </section>)}
  </Page>
}
