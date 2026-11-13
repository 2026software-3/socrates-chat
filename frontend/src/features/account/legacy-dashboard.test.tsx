import { screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { useLocation } from 'react-router'
import { describe, expect, it } from 'vitest'
import { AppRoutes } from '@/app/routes'
import type { Roles } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

/** `/dashboard` 已併入 `/account`（S-18）：舊網址一律導向，保留查詢字串；全部使用合成資料。 */

function Probe() {
  const { pathname, search } = useLocation()
  return <output data-testid="location">{pathname + search}</output>
}

const here = () => screen.getByTestId('location').textContent

async function visit(roles: Partial<Roles>, url: string) {
  server.use(
    http.get('/api/dashboard/me', () =>
      HttpResponse.json({ total_conversations: 0, completed: 0, total_turns: 0, recent: [] }),
    ),
  )
  mockMe(makeMe(roles))
  renderApp(
    <>
      <AppRoutes />
      <Probe />
    </>,
    { route: url },
  )
  await screen.findByRole('heading', { level: 1, name: '帳號' })
}

describe('the retired /dashboard URL', () => {
  it.each<[string, Partial<Roles>]>([
    ['a student', { student: true }],
    ['a teacher', { teacher: true }],
    ['an admin-only account', { admin: true }],
    ['a teacher who is also a student', { teacher: true, student: true }],
  ])('sends %s to /account', async (_label, roles) => {
    await visit(roles, '/dashboard')
    expect(here()).toBe('/account')
  })

  it.each(['?link=ok', '?link_error=email_in_use', '?a=1&b=2'])('keeps the query string (%s)', async (query) => {
    await visit({ student: true }, `/dashboard${query}`)
    expect(here()).toBe(`/account${query}`)
  })

  it('shows the Google link result of an old bookmark or an in-flight link flow', async () => {
    await visit({ student: true }, '/dashboard?link=ok')
    expect(await screen.findByText('已連結 Google 帳號。')).toBeInTheDocument()
  })

  it('does not leave /dashboard in the history: the redirect replaces the entry', async () => {
    await visit({ student: true }, '/dashboard')
    expect(here()).not.toContain('dashboard')
  })

  it('leaves the teacher dashboard redirects alone', async () => {
    server.use(http.get('/api/classrooms', () => HttpResponse.json([])))
    mockMe(makeMe({ teacher: true }))
    renderApp(
      <>
        <AppRoutes />
        <Probe />
      </>,
      { route: '/teacher/dashboard' },
    )
    await screen.findByRole('heading', { name: '我的教室' })
    expect(here()).toBe('/teacher/classrooms')
  })
})
