import { screen, waitFor, waitForElementToBeRemoved } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const notice = {
  id: '94384311-da78-4d3b-9e02-91f881761f2b',
  kind: 'student_number_corrected',
  created_at: '2026-11-16T06:30:00Z',
}

describe('student number correction notifications', () => {
  it('shows the correction time at login and remembers dismissal on a later login', async () => {
    mockMe(makeMe({ student: true }))
    let read = false
    let reads = 0
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/notifications', () => {
        reads++
        return HttpResponse.json(read ? [] : [notice])
      }),
      http.post(`/api/notifications/${notice.id}/read`, () => {
        read = true
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const first = renderApp()
    const message = await screen.findByText('學號已由教師或管理者更正。')
    expect(message.parentElement?.querySelector('time')).toHaveAttribute('datetime', notice.created_at)
    const removed = waitForElementToBeRemoved(message)
    await userEvent.click(screen.getByRole('button', { name: '關閉通知' }))
    await removed
    expect(read).toBe(true)

    first.unmount()
    renderApp()
    await waitFor(() => expect(reads).toBe(2))
    expect(screen.queryByText('學號已由教師或管理者更正。')).not.toBeInTheDocument()
  })

  it('keeps the localized notification visible if marking it read fails', async () => {
    mockMe(makeMe({ student: true }, { language: 'es' }))
    let fail = true
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/notifications', () => HttpResponse.json([notice])),
      http.post(`/api/notifications/${notice.id}/read`, () => {
        if (fail) {
          fail = false
          return apiError(500, 'internal')
        }
        return new HttpResponse(null, { status: 204 })
      }),
    )

    renderApp(null, { lang: 'es' })
    const message = await screen.findByText('Un docente o administrador corrigió tu número de estudiante.')
    const dismiss = screen.getByRole('button', { name: 'Cerrar notificación' })
    await userEvent.click(dismiss)
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(message).toBeInTheDocument()

    const removed = waitForElementToBeRemoved(message)
    await userEvent.click(dismiss)
    await removed
  })
})
