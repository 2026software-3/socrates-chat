import { screen, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { PersonalDashboardPage } from '@/features/dashboard/personal-dashboard-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { PersonalDashboard } from '@/lib/types'
import { server } from '@/test/server'

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
