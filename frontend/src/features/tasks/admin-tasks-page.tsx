import { useState, type FormEvent } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { Task } from './tasks-page'

function TaskForm({ task, reload }: { task: Task; reload: () => void }) {
  const t = useT()
  const [goal, setGoal] = useState(String(task.goal))
  const [reward, setReward] = useState(String(task.reward))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>()
  const [saved, setSaved] = useState(false)
  async function save(event: FormEvent) {
    event.preventDefault()
    if (busy) return
    setBusy(true); setError(undefined); setSaved(false)
    try {
      await api.patch(`/api/admin/tasks/${encodeURIComponent(task.id)}`, { goal: Number(goal), reward: Number(reward) })
      setSaved(true); reload()
    } catch (err) { setError(err) } finally { setBusy(false) }
  }
  return <form onSubmit={(event) => void save(event)} className="space-y-3 rounded-md border p-4">
    <h2 className="font-medium">{t(task.period === 'daily' ? 'tasks.daily' : 'tasks.weekly')}</h2>
    <Label htmlFor={`${task.id}-goal`}>{t('tasks.goal')}</Label>
    <Input id={`${task.id}-goal`} type="number" min="1" step="1" required value={goal} onChange={(e) => setGoal(e.target.value)} />
    <Label htmlFor={`${task.id}-reward`}>{t('tasks.points')}</Label>
    <Input id={`${task.id}-reward`} type="number" min="1" step="1" required value={reward} onChange={(e) => setReward(e.target.value)} />
    <Button disabled={busy}>{t('common.save')}</Button>
    {saved ? <p role="status">{t('tasks.saved')}</p> : null}
    {error ? <ErrorBlock error={error} onRetry={() => setError(undefined)} /> : null}
  </form>
}

export function AdminTasksPage() {
  const t = useT()
  const { data, loading, error, reload } = useFetch<Task[]>('/api/admin/tasks')
  return <Page title={t('tasks.admin')}>
    {loading && !data ? <LoadingBlock /> : null}
    {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
    {data?.length === 0 ? <EmptyBlock>{t('tasks.empty')}</EmptyBlock> : null}
    {data?.map((task) => <TaskForm key={task.id} task={task} reload={reload} />)}
  </Page>
}
