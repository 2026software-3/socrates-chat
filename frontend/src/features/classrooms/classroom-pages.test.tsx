import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { classroom, ROOM_A } from '@/features/teacher/test-helpers'
import type { Roles, Activity, ActivityDistribution, ClaimGroup, ClassDashboard, TeacherSummary } from '@/lib/types'
import { apiError, makeMe, mockMe, pageOf, renderApp } from '@/test/render'
import { server } from '@/test/server'

const ACT = 'act-1'
const STUDENT = 'stu-1'

const group = (name: string, count: number, over: Partial<ClaimGroup> = {}): ClaimGroup => ({
  name,
  other: false,
  count,
  reasons: [],
  ...over,
})

const distribution = (over: Partial<ActivityDistribution> = {}): ActivityDistribution => ({
  activity_id: ACT,
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

const dashboard: ClassDashboard = {
  masked: false,
  students_total: 30,
  students_participating: 12,
  completed: 20,
  activities: [distribution()],
}

const activity: Activity = {
  id: ACT,
  title: '第一週討論',
  description: '電車難題',
  topic_id: 't1',
  opens_at: null,
  closes_at: null,
  classroom_id: ROOM_A,
  status: 'published',
  is_archived: false,
  created_at: '2026-01-01T00:00:00Z',
}

const summary: TeacherSummary = {
  conversation_id: 'c1',
  classroom_id: ROOM_A,
  title: '電車難題',
  ended_at: '2026-03-05T10:00:00Z',
  student_id: STUDENT,
  student_name: '王小明',
  student_email: 'ming@example.com',
  language: 'zh-TW',
  stance: '應該拉桿',
  reasons: '拯救較多人',
}

const seen: URL[] = []

function mockApi(data: ClassDashboard = dashboard) {
  server.use(
    http.get('/api/classrooms', () => HttpResponse.json([classroom(ROOM_A, '2026 春季班')])),
    http.get('/api/topics', () => HttpResponse.json([])),
    http.get('/api/activities', ({ request }) => {
      const url = new URL(request.url)
      seen.push(url)
      return pageOf(url.searchParams.get('archived') === 'true' ? [] : [activity])
    }),
    http.get('/api/dashboard/class', ({ request }) => {
      seen.push(new URL(request.url))
      return HttpResponse.json(data)
    }),
    http.get('/api/summaries', ({ request }) => {
      const url = new URL(request.url)
      seen.push(url)
      return pageOf([summary])
    }),
  )
}

function renderAt(route: string, roles: Partial<Roles> = { teacher: true }) {
  seen.length = 0
  mockMe(makeMe(roles))
  return renderApp(null, { route })
}

describe('classroom → activity → student navigation', () => {
  it('walks from the classroom list down to a student, keeping every request scoped', async () => {
    mockApi()
    renderAt('/teacher/classrooms')

    await userEvent.click(await screen.findByRole('link', { name: '2026 春季班' }))
    expect(await screen.findByRole('heading', { level: 1, name: '2026 春季班' })).toBeInTheDocument()
    expect(await screen.findByText('30')).toBeInTheDocument()
    // 該教室的活動只查這間教室
    expect(seen.some((u) => u.pathname === '/api/activities' && u.searchParams.get('classroom_id') === ROOM_A)).toBe(true)

    await userEvent.click(await screen.findByRole('link', { name: '第一週討論' }))
    expect(await screen.findAllByTestId('claim-group')).toHaveLength(3)
    // 論點分布與學生總結都只看這個活動
    await screen.findByText('王小明')
    const call = seen.find((u) => u.pathname === '/api/summaries')!
    expect(call.searchParams.get('activity')).toBe(ACT)
    expect(call.searchParams.get('classroom_id')).toBe(ROOM_A)

    await userEvent.click(screen.getByRole('link', { name: '王小明' }))
    expect(await screen.findByRole('heading', { level: 1, name: '王小明' })).toBeInTheDocument()
    await screen.findByText('應該拉桿')
    const studentCall = seen.filter((u) => u.pathname === '/api/summaries').at(-1)!
    expect(studentCall.searchParams.get('student')).toBe(STUDENT)
    expect(studentCall.searchParams.get('activity')).toBeNull()
  })

  it('offers breadcrumbs back up the hierarchy', async () => {
    mockApi()
    renderAt(`/teacher/classrooms/${ROOM_A}/activities/${ACT}`)
    const crumbs = await screen.findByRole('navigation', { name: 'breadcrumb' })
    await userEvent.click(within(crumbs).getByRole('link', { name: '2026 春季班' }))
    expect(await screen.findByRole('heading', { level: 1, name: '2026 春季班' })).toBeInTheDocument()
  })

  it('shows not found for a classroom that is not in the list', async () => {
    mockApi()
    renderAt('/teacher/classrooms/00000000-0000-4000-8000-000000000000')
    expect(await screen.findByRole('link', { name: '回到教室列表' })).toBeInTheDocument()
  })

  it('links roster students to their own page, only when they have an account', async () => {
    mockApi()
    server.use(
      http.get(`/api/classrooms/${ROOM_A}/students`, () =>
        pageOf(
          [
            { id: STUDENT, email: 'ming@example.com', display_name: '王小明', has_account: true, disabled: false, completed_conversations: 1 },
            { id: null, email: 'new@example.com', display_name: null, has_account: false, disabled: false, completed_conversations: 0 },
          ],
          null,
          { total: 2 },
        ),
      ),
    )
    renderAt(`/teacher/classrooms/${ROOM_A}/students`)
    const link = await screen.findByRole('link', { name: 'ming@example.com' })
    expect(link).toHaveAttribute('href', `/teacher/classrooms/${ROOM_A}/students/${STUDENT}`)
    expect(screen.queryByRole('link', { name: 'new@example.com' })).not.toBeInTheDocument()
  })

  it('shows an empty state for a student with no finished conversations', async () => {
    mockApi()
    server.use(http.get('/api/summaries', () => pageOf([])))
    renderAt(`/teacher/classrooms/${ROOM_A}/students/${STUDENT}`)
    expect(await screen.findByText(/還沒有完成的對話/u)).toBeInTheDocument()
  })
})

describe('activity claim distribution', () => {
  const path = `/teacher/classrooms/${ROOM_A}/activities/${ACT}`

  it('shows each claim with its count and share, the smallest merged into other', async () => {
    mockApi()
    renderAt(path)
    const groups = await screen.findAllByTestId('claim-group')
    expect(groups[0]).toHaveTextContent('拉桿12(60%)')
    expect(groups[1]).toHaveTextContent('不拉桿6(30%)')
    expect(groups[2]).toHaveTextContent('其他2(10%)')
    expect(screen.getByText('20 場完成')).toBeInTheDocument()
  })

  it('opens a claim group to show its core reasons', async () => {
    mockApi()
    renderAt(path)
    const first = (await screen.findAllByTestId('claim-group'))[0]
    await userEvent.click(within(first).getByText('拉桿'))
    expect(within(first).getByText('救五人比救一人重要')).toBeInTheDocument()
  })

  it('draws no radar chart and mentions no ethical school', async () => {
    mockApi()
    renderAt(path)
    await screen.findAllByTestId('claim-group')
    expect(screen.queryByTestId('radar-chart')).not.toBeInTheDocument()
    expect(screen.queryByText(/效益主義|義務論|德行倫理|契約論|關懷倫理|存在主義|論證傾向/)).not.toBeInTheDocument()
  })

  it('regroups by activity_id on request and reloads', async () => {
    let body: unknown
    let loads = 0
    mockApi()
    server.use(
      http.get('/api/dashboard/class', () => {
        loads += 1
        return HttpResponse.json({ ...dashboard, activities: [distribution({ ungrouped: loads === 1 ? 2 : 0 })] })
      }),
      http.post('/api/dashboard/class/regroup', async ({ request }) => {
        body = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )
    renderAt(path)
    expect(await screen.findByText('2 場的主張還沒有歸群。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重新分組' }))
    const confirm = await screen.findByRole('alertdialog')
    expect(confirm).toHaveTextContent('確定要重新分組這道題目的主張嗎？')
    expect(confirm).toHaveTextContent('不會傳送姓名或對話原文給 AI')
    expect(body).toBeUndefined()
    await userEvent.click(within(confirm).getByRole('button', { name: '重新分組' }))
    await waitFor(() => expect(screen.queryByText('2 場的主張還沒有歸群。')).not.toBeInTheDocument())
    expect(body).toEqual({ activity_id: ACT })
  })

  it('keeps the groups and shows a translated message when regrouping fails', async () => {
    mockApi()
    server.use(http.post('/api/dashboard/class/regroup', () => apiError(502, 'ai_unavailable')))
    renderAt(path)
    await userEvent.click(await screen.findByRole('button', { name: '重新分組' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '重新分組' }))
    expect(await screen.findByText('AI 暫時無法回覆。')).toBeInTheDocument()
    expect(screen.getAllByTestId('claim-group')).toHaveLength(3)
  })

  it('shows an empty state before anyone has finished the activity', async () => {
    mockApi({ ...dashboard, completed: 0, students_participating: 0, activities: [] })
    renderAt(path)
    expect(await screen.findByText(/還沒有學生完成對話/u)).toBeInTheDocument()
  })

  it('tells admins that analysis results are hidden', async () => {
    mockApi({ ...dashboard, masked: true, completed: 0, students_participating: 0, activities: [] })
    renderAt(`/admin/classrooms/${ROOM_A}/activities/${ACT}`, { admin: true })
    expect(await screen.findByText(/管理者看不到分析結果/u)).toBeInTheDocument()
    expect(screen.queryByTestId('claim-group')).not.toBeInTheDocument()
  })
})
