import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { lazy, Suspense } from 'react'
import { Link, Route, Routes, useLocation } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { RouteErrorBoundary } from '@/app/route-error-boundary'
import { makeMe, mockMe, renderApp } from '@/test/render'

// 重新部署後，舊分頁要載入的 chunk 已經不存在
const Broken = lazy(() => Promise.reject(new Error('Failed to fetch dynamically imported module')))

function Harness({ onRetry }: { onRetry: () => void }) {
  const { pathname } = useLocation()
  return (
    <>
      <Link to="/ok">ok</Link>
      <RouteErrorBoundary pathname={pathname} onRetry={onRetry}>
        <Suspense fallback={null}>
          <Routes>
            <Route path="/broken" element={<Broken />} />
            <Route path="/ok" element={<p>fine</p>} />
          </Routes>
        </Suspense>
      </RouteErrorBoundary>
    </>
  )
}

describe('RouteErrorBoundary', () => {
  it('shows an error with retry instead of a blank app when a page chunk fails to load', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    mockMe(makeMe({ student: true }))
    const retry = vi.fn()
    renderApp(<Harness onRetry={retry} />, { route: '/broken' })

    expect(await screen.findByRole('alert')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(retry).toHaveBeenCalledOnce()
  })

  it('clears the error after navigating to another page', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    mockMe(makeMe({ student: true }))
    renderApp(<Harness onRetry={() => {}} />, { route: '/broken' })

    await screen.findByRole('alert')
    await userEvent.click(screen.getByRole('link', { name: 'ok' }))
    expect(await screen.findByText('fine')).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
})
