import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { mockMe, makeMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const tasks = [
  { id: 'daily_dialogue', period: 'daily', goal: 1, reward: 5, period_start: '2026-11-07', progress: 1, claimed: false },
  { id: 'weekly_dialogue', period: 'weekly', goal: 5, reward: 20, period_start: '2026-11-02', progress: 1, claimed: false },
]

describe('tasks', () => {
  beforeEach(() => { mockMe(makeMe({ student: true })) })
  it('shows progress and allows claiming a completed task once', async () => {
    let claimed = false
    server.use(
      http.get('/api/tasks', () => HttpResponse.json(tasks.map((task) => task.id === 'daily_dialogue' ? { ...task, claimed } : task))),
      http.post('/api/tasks/daily_dialogue/claim', async ({ request }) => {
        expect(await request.json()).toEqual({ period_start: '2026-11-07' })
        claimed = true
        return HttpResponse.json({ ...tasks[0], claimed: true })
      }),
    )
    renderApp(null, { route: '/tasks' })
    expect(await screen.findByText('每日對話')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '領取' }))
    expect(await screen.findByText('已領取')).toBeInTheDocument()
    expect(screen.getByText('每週對話')).toBeInTheDocument()
  })
  it('shows a retryable loading error', async () => {
    server.use(http.get('/api/tasks', () => HttpResponse.json({ error: { code: 'internal', request_id: 't' } }, { status: 500 })))
    renderApp(null, { route: '/tasks' })
    await waitFor(() => expect(screen.getByRole('button', { name: '重試' })).toBeInTheDocument())
  })

  it('keeps the claim button disabled until the refreshed status arrives', async () => {
    let claimed = false
    let refreshing = false
    let finishRefresh = () => {}
    const refresh = new Promise<void>((resolve) => { finishRefresh = resolve })
    server.use(
      http.get('/api/tasks', async () => {
        if (claimed) {
          refreshing = true
          await refresh
        }
        return HttpResponse.json([{ ...tasks[0], claimed }])
      }),
      http.post('/api/tasks/daily_dialogue/claim', () => {
        claimed = true
        return HttpResponse.json({ ...tasks[0], claimed })
      }),
    )
    renderApp(null, { route: '/tasks' })
    const claimButton = await screen.findByRole('button', { name: '領取' })
    const enabledAfterClaim: boolean[] = []
    const observer = new MutationObserver((records) => {
      // 按鈕的 disabled 屬性被移除，代表在刷新完成前已可再次點擊
      if (claimed && records.some((record) => record.oldValue !== null)) enabledAfterClaim.push(true)
    })
    observer.observe(claimButton, { attributes: true, attributeFilter: ['disabled'], attributeOldValue: true })
    await userEvent.click(claimButton)
    try {
      await waitFor(() => expect(refreshing).toBe(true))
      expect(screen.getByRole('button', { name: '領取' })).toBeDisabled()
    } finally {
      finishRefresh()
    }
    expect(await screen.findByText('已領取')).toBeInTheDocument()
    observer.disconnect()
    expect(enabledAfterClaim).toEqual([])
  })

  it('keeps the claim button disabled and offers retry when the refresh fails after claiming', async () => {
    let claimed = false
    server.use(
      http.get('/api/tasks', () => claimed
        ? HttpResponse.json({ error: { code: 'internal', request_id: 't' } }, { status: 500 })
        : HttpResponse.json([{ ...tasks[0], claimed }])),
      http.post('/api/tasks/daily_dialogue/claim', () => {
        claimed = true
        return HttpResponse.json({ ...tasks[0], claimed })
      }),
    )
    renderApp(null, { route: '/tasks' })
    await userEvent.click(await screen.findByRole('button', { name: '領取' }))
    expect(await screen.findByRole('button', { name: '重試' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '領取' })).toBeDisabled()
  })

  it('shows the empty state when there are no tasks', async () => {
    server.use(http.get('/api/tasks', () => HttpResponse.json([])))
    renderApp(null, { route: '/tasks' })
    expect(await screen.findByText('目前沒有任務。')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '領取' })).not.toBeInTheDocument()
  })
})

describe('task settings', () => {
  it('allows an administrator to update the target and reward', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(
      http.get('/api/admin/tasks', () => HttpResponse.json([tasks[0]])),
      http.patch('/api/admin/tasks/daily_dialogue', async ({ request }) => {
        expect(await request.json()).toEqual({ goal: 2, reward: 8 })
        return HttpResponse.json({ ...tasks[0], goal: 2, reward: 8 })
      }),
    )
    renderApp(null, { route: '/admin/tasks' })
    const goal = await screen.findByLabelText('完成場數目標')
    const reward = screen.getByLabelText('獎勵點數')
    await userEvent.clear(goal); await userEvent.type(goal, '2')
    await userEvent.clear(reward); await userEvent.type(reward, '8')
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('設定已儲存。')).toBeInTheDocument()
  })
})
