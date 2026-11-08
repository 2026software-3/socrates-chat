import { screen, within } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { it, expect } from 'vitest'
import { AdminHome } from '@/features/admin/admin-home'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

it('shows the number of active topics on the admin home page', async () => {
  mockMe(makeMe({ admin: true }))
  server.use(
    http.get('/api/admin/teachers', () => HttpResponse.json({ emails: [] })),
    http.get('/api/dashboard/class', () => HttpResponse.json({ students_total: 0 })),
    http.get('/api/topics', () => HttpResponse.json([
      { id: '1', title: 'Active one', is_active: true },
      { id: '2', title: 'Active two', is_active: true },
      { id: '3', title: 'Inactive', is_active: false },
    ])),
  )

  renderApp(<AdminHome />)
  const card = screen.getByText('啟用中的題目').closest('[data-slot="card"]')
  expect(card).not.toBeNull()
  expect(await within(card as HTMLElement).findByText('2')).toBeInTheDocument()
})
