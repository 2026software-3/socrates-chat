import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { ClassDashboardPage } from '@/features/dashboard/class-dashboard-page'
import { PersonalDashboardPage } from '@/features/dashboard/personal-dashboard-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { ActivityDistribution, ClaimGroup, ClassDashboard, PersonalDashboard } from '@/lib/types'
import { apiError } from '@/test/render'
import { server } from '@/test/server'

const group = (name: string, count: number, over: Partial<ClaimGroup> = {}): ClaimGroup => ({
  name,
  other: false,
  count,
  reasons: [],
  ...over,
})

const activity = (over: Partial<ActivityDistribution> = {}): ActivityDistribution => ({
  activity_id: 'act-1',
  title: '第一週討論',
  completed: 20,
  claims: [
    group('拉桿', 12, { reasons: ['救五人比救一人重要', '結果最重要'] }),
    group('不拉桿', 6, { reasons: ['不能把人當工具'] }),
    { ...group('', 2), name: null, other: true },
  ],
  ungrouped: 0,
  ...over,
})

const classData: ClassDashboard = {
  masked: false,
  students_total: 30,
  students_participating: 12,
  completed: 20,
  activities: [activity()],
}

function mockClass(data: ClassDashboard = classData) {
  server.use(http.get('/api/dashboard/class', () => HttpResponse.json(data)))
}

describe('class dashboard', () => {
  it('shows participation and the claim distribution of each activity without any names', async () => {
    mockClass()
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByRole('heading', { name: '班上論點分布' })).toBeInTheDocument()
    const stat = (label: string) => screen.getByText(label).parentElement as HTMLElement
    expect(within(stat('修課學生')).getByText('30')).toBeInTheDocument()
    expect(within(stat('已參與學生')).getByText('12')).toBeInTheDocument()

    expect(screen.getByRole('heading', { name: '第一週討論' })).toBeInTheDocument()
    expect(screen.getByText('20 場完成')).toBeInTheDocument()
    const groups = screen.getAllByTestId('claim-group')
    expect(groups).toHaveLength(3)
    // 每列是一種主張，顯示人數與比例；最小的幾群併成「其他」
    expect(groups[0]).toHaveTextContent('拉桿12(60%)')
    expect(groups[1]).toHaveTextContent('不拉桿6(30%)')
    expect(groups[2]).toHaveTextContent('其他2(10%)')
  })

  it('lists each activity separately, even when two activities share a title', async () => {
    mockClass({
      ...classData,
      completed: 25,
      activities: [
        activity({ activity_id: 'act-1', title: '第一週討論', completed: 20 }),
        activity({ activity_id: 'act-2', title: '第一週討論', completed: 5, claims: [group('贊成', 5)] }),
      ],
    })
    renderTeacherPage(<ClassDashboardPage />)
    const headings = await screen.findAllByRole('heading', { name: '第一週討論' })
    expect(headings).toHaveLength(2)
    expect(screen.getByText('20 場完成')).toBeInTheDocument()
    expect(screen.getByText('5 場完成')).toBeInTheDocument()
    const cards = headings.map((h) => h.closest('div.space-y-4') as HTMLElement)
    expect(within(cards[1]).getByTestId('claim-group')).toHaveTextContent('贊成5(100%)')
  })

  it('opens a claim group to show its core reasons', async () => {
    mockClass()
    renderTeacherPage(<ClassDashboardPage />)
    const first = (await screen.findAllByTestId('claim-group'))[0]
    await userEvent.click(within(first).getByText('拉桿'))
    expect(within(first).getByText('救五人比救一人重要')).toBeInTheDocument()
    expect(within(first).getByText('結果最重要')).toBeInTheDocument()
  })

  it('draws no radar chart and mentions no ethical school', async () => {
    mockClass()
    renderTeacherPage(<ClassDashboardPage />)
    await screen.findByRole('heading', { name: '第一週討論' })
    expect(screen.queryByTestId('radar-chart')).not.toBeInTheDocument()
    expect(screen.queryByText(/效益主義|義務論|德行倫理|契約論|關懷倫理|存在主義|論證傾向/)).not.toBeInTheDocument()
  })

  it('regroups an activity by activity_id on request and reloads', async () => {
    let body: unknown
    let loads = 0
    server.use(
      http.get('/api/dashboard/class', () => {
        loads += 1
        return HttpResponse.json({ ...classData, activities: [activity({ ungrouped: loads === 1 ? 2 : 0 })] })
      }),
      http.post('/api/dashboard/class/regroup', async ({ request }) => {
        body = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByText('2 場的主張還沒有歸群。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重新分組' }))
    await waitFor(() => expect(screen.queryByText('2 場的主張還沒有歸群。')).not.toBeInTheDocument())
    expect(body).toEqual({ activity_id: 'act-1' })
  })

  it('shows a translated message when regrouping fails', async () => {
    mockClass()
    server.use(http.post('/api/dashboard/class/regroup', () => apiError(502, 'ai_unavailable')))
    renderTeacherPage(<ClassDashboardPage />)
    await userEvent.click(await screen.findByRole('button', { name: '重新分組' }))
    expect(await screen.findByText('AI 暫時無法回覆。')).toBeInTheDocument()
    // 失敗時原本的分組還在
    expect(screen.getAllByTestId('claim-group')).toHaveLength(3)
  })

  it('shows an empty state before anyone has finished a conversation', async () => {
    mockClass({ ...classData, completed: 0, students_participating: 0, activities: [] })
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByText(/還沒有學生完成對話/u)).toBeInTheDocument()
  })

  it('tells admins that analysis results are hidden', async () => {
    mockClass({ ...classData, masked: true, completed: 0, students_participating: 0, activities: [] })
    renderTeacherPage(<ClassDashboardPage />, undefined, { admin: true })
    expect(await screen.findByText(/管理者看不到分析結果/u)).toBeInTheDocument()
    expect(screen.queryByTestId('claim-group')).not.toBeInTheDocument()
  })

  it('shows an error with retry', async () => {
    let calls = 0
    server.use(
      http.get('/api/dashboard/class', () => {
        calls += 1
        return calls === 1 ? apiError(500, 'internal') : HttpResponse.json(classData)
      }),
    )
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('修課學生')).toBeInTheDocument()
  })
})

const personal: PersonalDashboard = {
  total_conversations: 4,
  total_turns: 17,
  completed: 3,
  recent: [
    { conversation_id: 'c1', title: '電車難題', ended_at: '2026-10-02T10:00:00Z', stage: 3, turn_count: 6, stance: '傾向拉桿', claim: '該拉桿' },
    { conversation_id: 'c2', title: '說謊可以嗎', ended_at: '2026-10-01T10:00:00Z', stage: 2, turn_count: 5, stance: '不可以', claim: null },
  ],
}

describe('personal dashboard', () => {
  it('shows my own counts and the claims, stances and turns of recent discussions', async () => {
    server.use(http.get('/api/dashboard/me', () => HttpResponse.json(personal)))
    renderTeacherPage(<PersonalDashboardPage />, undefined, { student: true })
    expect(await screen.findByText('對話數')).toBeInTheDocument()
    expect(screen.getByText('17')).toBeInTheDocument()
    const link = screen.getByRole('link', { name: '電車難題' })
    expect(link).toHaveAttribute('href', '/conversations/c1')
    const card = link.closest('li') as HTMLElement
    expect(within(card).getByText('該拉桿')).toBeInTheDocument()
    expect(within(card).getByText('傾向拉桿')).toBeInTheDocument()
    expect(within(card).getByText('6 回合')).toBeInTheDocument()
    // 沒有主張的討論仍列出，改顯示立場
    expect(screen.getByRole('link', { name: '說謊可以嗎' })).toBeInTheDocument()
    expect(screen.getByText('不可以')).toBeInTheDocument()
  })

  it('shows no radar chart and no ethical school label', async () => {
    server.use(http.get('/api/dashboard/me', () => HttpResponse.json(personal)))
    renderTeacherPage(<PersonalDashboardPage />, undefined, { student: true })
    await screen.findByRole('link', { name: '電車難題' })
    expect(screen.queryByTestId('radar-chart')).not.toBeInTheDocument()
    expect(screen.queryByText(/效益主義|義務論|德行倫理|契約論|關懷倫理|存在主義|論證傾向|學派/)).not.toBeInTheDocument()
  })

  it('invites the student to start when nothing is finished yet', async () => {
    server.use(
      http.get('/api/dashboard/me', () => HttpResponse.json({ ...personal, total_conversations: 1, completed: 0, recent: [] })),
    )
    renderTeacherPage(<PersonalDashboardPage />, undefined, { student: true })
    expect(await screen.findByText(/完成第一場討論後/u)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '選擇題目開始討論' })).toHaveAttribute('href', '/')
  })
})
