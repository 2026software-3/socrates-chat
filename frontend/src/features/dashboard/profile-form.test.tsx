import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

/** `/api/me` 的 GET 與 PATCH 共用一份可變的使用者，模擬後端保存名稱。 */
function mockProfileApi(initial: string | null) {
  let name = initial
  const patches: unknown[] = []
  server.use(
    http.get('/api/dashboard/me', () => HttpResponse.json({ total_conversations: 0, completed: 0, total_turns: 0, recent: [] })),
    http.get('/api/me', () => HttpResponse.json(makeMe({ student: true }, { display_name: name }))),
    http.patch('/api/me', async ({ request }) => {
      const body = (await request.json()) as { display_name: string | null }
      patches.push(body)
      name = body.display_name?.trim() || null
      return HttpResponse.json(makeMe({ student: true }, { display_name: name }))
    }),
  )
  return patches
}

describe('profile form on the personal dashboard', () => {
  it('is reached from the header name and starts with the current name', async () => {
    mockProfileApi('小明')
    renderApp(null, { route: '/' })
    await userEvent.click(await screen.findByRole('link', { name: '小明' }))
    expect(await screen.findByRole('heading', { name: '個人設定' })).toBeInTheDocument()
    expect(screen.getByLabelText('顯示名稱')).toHaveValue('小明')
  })

  it('links the email when there is no name yet', async () => {
    mockProfileApi(null)
    renderApp(null, { route: '/' })
    expect(await screen.findByRole('link', { name: 'user@example.com' })).toHaveAttribute('href', '/dashboard')
  })

  it('counts Unicode characters (an emoji is one) and never rewrites what the user typed', async () => {
    mockProfileApi(null)
    renderApp(null, { route: '/dashboard' })
    const input = await screen.findByLabelText('顯示名稱')
    await userEvent.type(input, '😀'.repeat(30))
    expect(screen.getByText('30 / 30')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '儲存' })).toBeEnabled()
    await userEvent.type(input, '😀')
    expect((input as HTMLInputElement).value).toBe('😀'.repeat(31))
    expect(screen.getByText('31 / 30')).toBeInTheDocument()
    expect(input).toBeInvalid()
    expect(screen.getByRole('button', { name: '儲存' })).toBeDisabled()
  })

  it('warns instead of silently truncating a long paste, and recovers once shortened', async () => {
    const patches = mockProfileApi(null)
    renderApp(null, { route: '/dashboard' })
    const input = await screen.findByLabelText('顯示名稱')
    await userEvent.click(input)
    await userEvent.paste('字'.repeat(35))
    expect((input as HTMLInputElement).value).toBe('字'.repeat(35))
    expect(screen.getByText('35 / 30')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '儲存' })).toBeDisabled()
    await userEvent.type(input, '{Backspace}'.repeat(5))
    expect(screen.getByRole('button', { name: '儲存' })).toBeEnabled()
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已儲存')
    expect(patches).toEqual([{ display_name: '字'.repeat(30) }])
  })

  it('measures the length after trimming, like the backend', async () => {
    mockProfileApi(null)
    renderApp(null, { route: '/dashboard' })
    const input = await screen.findByLabelText('顯示名稱')
    await userEvent.click(input)
    await userEvent.paste(`  ${'字'.repeat(30)}  `)
    expect(screen.getByText('30 / 30')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '儲存' })).toBeEnabled()
  })

  it('saves the new name and updates the header', async () => {
    const patches = mockProfileApi('小明')
    renderApp(null, { route: '/dashboard' })
    const input = await screen.findByLabelText('顯示名稱')
    await userEvent.clear(input)
    await userEvent.type(input, '阿華')
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已儲存')
    expect(patches).toEqual([{ display_name: '阿華' }])
    expect(screen.getByRole('link', { name: '阿華' })).toBeInTheDocument()
  })

  it('clearing the field sends null and the header falls back to the email', async () => {
    const patches = mockProfileApi('小明')
    renderApp(null, { route: '/dashboard' })
    await userEvent.clear(await screen.findByLabelText('顯示名稱'))
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('status')).toHaveTextContent('已儲存')
    expect(patches).toEqual([{ display_name: null }])
    expect(screen.getByRole('link', { name: 'user@example.com' })).toBeInTheDocument()
  })

  it('shows a translated message when the server rejects the name', async () => {
    mockProfileApi('小明')
    server.use(http.patch('/api/me', () => apiError(400, 'invalid_display_name')))
    renderApp(null, { route: '/dashboard' })
    await userEvent.type(await screen.findByLabelText('顯示名稱'), '1')
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('顯示名稱太長或含有不允許的字元')
  })

  it('does not link the name for teachers, who cannot rename', async () => {
    mockMe(makeMe({ teacher: true }))
    renderApp(null, { route: '/' })
    expect(await screen.findByText('Test User')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Test User' })).not.toBeInTheDocument()
  })
})
