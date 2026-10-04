import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { ClassDashboardPage } from '@/features/dashboard/class-dashboard-page'
import { PersonalDashboardPage } from '@/features/dashboard/personal-dashboard-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { ClaimGroup, ClassDashboard, Counts, PersonalDashboard, TopicDistribution } from '@/lib/types'
import { apiError } from '@/test/render'
import { server } from '@/test/server'

const frameworks = (over: Counts = {}): Counts => ({
  utilitarianism: 0,
  deontology: 0,
  virtue: 0,
  contractarianism: 0,
  care: 0,
  existentialism: 0,
  other: 0,
  unclassified: 0,
  ...over,
})

const radar = (average: Counts = {}, scored = 0) => ({
  scored,
  max: 5,
  average: { utilitarianism: 0, deontology: 0, virtue: 0, contractarianism: 0, care: 0, existentialism: 0, ...average },
})

const group = (name: string, count: number, over: Partial<ClaimGroup> = {}): ClaimGroup => ({
  name,
  other: false,
  count,
  reasons: [],
  radar: radar(),
  ...over,
})

const topic = (over: Partial<TopicDistribution> = {}): TopicDistribution => ({
  title: '電車難題',
  completed: 20,
  frameworks: frameworks(),
  radar: radar({ utilitarianism: 3.5 }, 12),
  claims: [
    group('拉桿', 12, { reasons: ['救五人比救一人重要', '結果最重要'], radar: radar({ utilitarianism: 4.5 }, 12) }),
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
  frameworks: frameworks(),
  radar: radar({ utilitarianism: 3.5, deontology: 2, virtue: 1.25 }, 12),
  topics: [topic()],
}

function mockClass(data: ClassDashboard = classData) {
  server.use(http.get('/api/dashboard/class', () => HttpResponse.json(data)))
}

describe('class dashboard', () => {
  it('shows participation and the claim distribution of each topic without any names', async () => {
    mockClass()
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByRole('heading', { name: '班上論點分布' })).toBeInTheDocument()
    const stat = (label: string) => screen.getByText(label).parentElement as HTMLElement
    expect(within(stat('修課學生')).getByText('30')).toBeInTheDocument()
    expect(within(stat('已參與學生')).getByText('12')).toBeInTheDocument()

    expect(screen.getByRole('heading', { name: '電車難題' })).toBeInTheDocument()
    expect(screen.getByText('20 場完成')).toBeInTheDocument()
    const groups = screen.getAllByTestId('claim-group')
    expect(groups).toHaveLength(3)
    // 每列是一種主張，顯示人數與比例；最小的幾群併成「其他」
    expect(groups[0]).toHaveTextContent('拉桿12(60%)')
    expect(groups[1]).toHaveTextContent('不拉桿6(30%)')
    expect(groups[2]).toHaveTextContent('其他2(10%)')
  })

  it('opens a claim group to show its core reasons and reasoning radar', async () => {
    mockClass()
    renderTeacherPage(<ClassDashboardPage />)
    const first = (await screen.findAllByTestId('claim-group'))[0]
    await userEvent.click(within(first).getByText('拉桿'))
    expect(within(first).getByText('救五人比救一人重要')).toBeInTheDocument()
    expect(within(first).getByText('結果最重要')).toBeInTheDocument()
    const chart = within(first).getByTestId('radar-chart').closest('figure') as HTMLElement
    expect(within(chart).getByText('效益主義：4.5 / 5')).toBeInTheDocument()
    // 沒有評分的群組不畫空雷達
    const second = screen.getAllByTestId('claim-group')[1]
    expect(within(second).queryByTestId('radar-chart')).not.toBeInTheDocument()
  })

  it('draws the overall 6-school average radar', async () => {
    mockClass()
    renderTeacherPage(<ClassDashboardPage />)
    const chart = (await screen.findAllByTestId('radar-chart'))[0]
    // 5 個格線環 + 1 個資料多邊形；6 個軸與 6 個資料點
    expect(chart.querySelectorAll('polygon')).toHaveLength(6)
    expect(chart.querySelectorAll('line')).toHaveLength(6)
    expect(chart.querySelectorAll('circle')).toHaveLength(6)
    const list = chart.closest('figure')!.querySelector('ul') as HTMLElement
    expect(within(list).getByText('效益主義：3.5 / 5')).toBeInTheDocument()
    expect(within(list).getByText('德行倫理：1.3 / 5')).toBeInTheDocument()
    expect(screen.getAllByText('依 12 份論點總結平均').length).toBeGreaterThan(0)
  })

  it('says there are no scores yet instead of drawing an empty radar', async () => {
    mockClass({ ...classData, radar: radar(), topics: [topic({ claims: [] })] })
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByText('還沒有評分資料。')).toBeInTheDocument()
    expect(screen.queryByTestId('radar-chart')).not.toBeInTheDocument()
    expect(screen.getByText('這個題目還沒有主張資料。')).toBeInTheDocument()
  })

  it('regroups a topic on request and reloads', async () => {
    let body: unknown
    let loads = 0
    server.use(
      http.get('/api/dashboard/class', () => {
        loads += 1
        return HttpResponse.json({ ...classData, topics: [topic({ ungrouped: loads === 1 ? 2 : 0 })] })
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
    expect(body).toEqual({ title: '電車難題' })
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
    mockClass({ ...classData, completed: 0, students_participating: 0, radar: radar(), topics: [] })
    renderTeacherPage(<ClassDashboardPage />)
    expect(await screen.findByText(/還沒有學生完成對話/u)).toBeInTheDocument()
  })

  it('tells admins that analysis results are hidden', async () => {
    mockClass({ ...classData, masked: true, completed: 0, students_participating: 0, radar: { scored: 0, max: 5, average: {} }, topics: [] })
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
  frameworks: frameworks({ deontology: 3 }),
  radar: radar({ deontology: 4, utilitarianism: 1 }, 3),
  recent: [
    { conversation_id: 'c1', title: '電車難題', ended_at: '2026-10-02T10:00:00Z', stage: 3, turn_count: 6, stance: '傾向拉桿', claim: '該拉桿', framework: 'utilitarianism', framework_scores: null },
    { conversation_id: 'c2', title: '說謊可以嗎', ended_at: '2026-10-01T10:00:00Z', stage: 2, turn_count: 5, stance: '不可以', claim: null, framework: null, framework_scores: null },
  ],
}

describe('personal dashboard', () => {
  it('shows my own counts, my 6-school radar and the claims of recent discussions', async () => {
    server.use(http.get('/api/dashboard/me', () => HttpResponse.json(personal)))
    renderTeacherPage(<PersonalDashboardPage />, undefined, { student: true })
    expect(await screen.findByText('對話數')).toBeInTheDocument()
    expect(screen.getByText('17')).toBeInTheDocument()
    const chart = screen.getByTestId('radar-chart').closest('figure') as HTMLElement
    expect(within(chart).getByText('義務論：4 / 5')).toBeInTheDocument()
    const link = screen.getByRole('link', { name: '電車難題' })
    expect(link).toHaveAttribute('href', '/conversations/c1')
    const card = link.closest('li') as HTMLElement
    expect(within(card).getByText('該拉桿')).toBeInTheDocument()
    expect(within(card).getByText('傾向拉桿')).toBeInTheDocument()
    expect(within(card).getByText('效益主義')).toBeInTheDocument()
    // 沒有主張的討論仍列出，改顯示立場
    expect(screen.getByRole('link', { name: '說謊可以嗎' })).toBeInTheDocument()
    expect(screen.getByText('不可以')).toBeInTheDocument()
  })

  it('invites the student to start when nothing is finished yet', async () => {
    server.use(
      http.get('/api/dashboard/me', () =>
        HttpResponse.json({ ...personal, total_conversations: 1, completed: 0, recent: [], radar: radar() }),
      ),
    )
    renderTeacherPage(<PersonalDashboardPage />, undefined, { student: true })
    expect(await screen.findByText(/完成第一場討論後/u)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '選擇題目開始討論' })).toHaveAttribute('href', '/')
  })
})
