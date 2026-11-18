import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const part = { id: 'synthetic', name_zh_tw: '測試部件', name_en: 'Test part', name_es: 'Pieza de prueba', price: 30, enabled: true, owned: false }

describe('part purchase', () => {
  beforeEach(() => mockMe(makeMe({ student: true })))
  it('shows an empty state when no parts are configured', async () => {
    server.use(http.get('/api/parts', () => HttpResponse.json([])), http.get('/api/points', () => HttpResponse.json({ balance: 0 })))
    renderApp(null, { route: '/parts' })
    expect(await screen.findByText('目前沒有可兌換的部件。')).toBeInTheDocument()
  })
  it('purchases a part and refreshes ownership', async () => {
    let owned = false
    server.use(
      http.get('/api/parts', () => HttpResponse.json([{ ...part, owned }])),
      http.get('/api/points', () => HttpResponse.json({ balance: owned ? 20 : 50 })),
      http.post('/api/parts/synthetic/purchase', () => { owned = true; return HttpResponse.json({ ...part, owned }) }),
    )
    renderApp(null, { route: '/parts' })
    expect(await screen.findByText('測試部件')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '解鎖' }))
    expect(await screen.findByText('已擁有')).toBeInTheDocument()
  })
  it('disables purchase until the catalog refreshes ownership', async () => {
    let owned = false
    let refreshing = false
    let finishRefresh = () => {}
    const refresh = new Promise<void>((resolve) => { finishRefresh = resolve })
    server.use(
      http.get('/api/parts', async () => {
        if (owned) {
          refreshing = true
          await refresh
        }
        return HttpResponse.json([{ ...part, owned }])
      }),
      http.get('/api/points', () => HttpResponse.json({ balance: owned ? 20 : 50 })),
      http.post('/api/parts/synthetic/purchase', () => {
        owned = true
        return HttpResponse.json({ ...part, owned })
      }),
    )
    renderApp(null, { route: '/parts' })
    await userEvent.click(await screen.findByRole('button', { name: '解鎖' }))
    try {
      await waitFor(() => expect(refreshing).toBe(true))
      expect(screen.getByRole('button', { name: '解鎖' })).toBeDisabled()
    } finally {
      finishRefresh()
    }
    expect(await screen.findByText('已擁有')).toBeInTheDocument()
  })
  it('disables purchase while the balance refreshes after a purchase', async () => {
    let owned = false
    let balanceRefreshing = false
    let finishBalance = () => {}
    const balanceRefresh = new Promise<void>((resolve) => { finishBalance = resolve })
    const other = { ...part, id: 'other', name_zh_tw: '其他部件', owned: false }
    server.use(
      http.get('/api/parts', () => HttpResponse.json([{ ...part, owned }, other])),
      http.get('/api/points', async () => {
        if (owned) {
          balanceRefreshing = true
          await balanceRefresh
        }
        return HttpResponse.json({ balance: owned ? 20 : 50 })
      }),
      http.post('/api/parts/synthetic/purchase', () => {
        owned = true
        return HttpResponse.json({ ...part, owned })
      }),
    )
    renderApp(null, { route: '/parts' })
    await userEvent.click((await screen.findAllByRole('button', { name: '解鎖' }))[0])
    try {
      await waitFor(() => expect(balanceRefreshing).toBe(true))
      expect(await screen.findByText('已擁有')).toBeInTheDocument()
      const otherSection = screen.getByRole('heading', { name: '其他部件' }).closest('section') as HTMLElement
      expect(within(otherSection).getByRole('button', { name: '解鎖' })).toBeDisabled()
    } finally {
      finishBalance()
    }
  })
  it('disables purchase when the catalog refresh fails after a purchase', async () => {
    let owned = false
    let failRefresh = false
    server.use(
      http.get('/api/parts', () => {
        if (failRefresh) return HttpResponse.json({ error: { code: 'internal', request_id: 'r1' } }, { status: 500 })
        return HttpResponse.json([{ ...part, owned }])
      }),
      http.get('/api/points', () => HttpResponse.json({ balance: owned ? 20 : 50 })),
      http.post('/api/parts/synthetic/purchase', () => {
        owned = true
        failRefresh = true
        return HttpResponse.json({ ...part, owned })
      }),
    )
    renderApp(null, { route: '/parts' })
    await userEvent.click(await screen.findByRole('button', { name: '解鎖' }))
    expect(await screen.findByRole('button', { name: /重試|retry/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '解鎖' })).toBeDisabled()
  })
  it('shows an actionable error when points are insufficient', async () => {
    server.use(
      http.get('/api/parts', () => HttpResponse.json([part])),
      http.get('/api/points', () => HttpResponse.json({ balance: 0 })),
      http.post('/api/parts/synthetic/purchase', () => HttpResponse.json({ error: { code: 'insufficient_points', request_id: 't' } }, { status: 409 })),
    )
    renderApp(null, { route: '/parts' })
    await userEvent.click(await screen.findByRole('button', { name: '解鎖' }))
    await waitFor(() => expect(screen.getByText('點數不足，無法解鎖。')).toBeInTheDocument())
  })
})

describe('part settings', () => {
  it('allows the administrator to update price and availability', async () => {
    mockMe(makeMe({ admin: true }))
    server.use(
      http.get('/api/admin/parts', () => HttpResponse.json([part])),
      http.patch('/api/admin/parts/synthetic', async ({ request }) => {
        expect(await request.json()).toEqual({ price: 12, enabled: false })
        return HttpResponse.json({ ...part, price: 12, enabled: false })
      }),
    )
    renderApp(null, { route: '/admin/parts' })
    const price = await screen.findByLabelText('價格')
    await userEvent.clear(price); await userEvent.type(price, '12')
    await userEvent.click(screen.getByRole('checkbox', { name: '啟用' }))
    await userEvent.click(screen.getByRole('button', { name: '儲存' }))
    expect(await screen.findByText('設定已儲存。')).toBeInTheDocument()
  })
})
