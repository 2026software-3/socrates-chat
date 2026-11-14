import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { RadarPanel } from '@/features/chat/radar-panel'
import { makeRadar, makeRadarState } from '@/features/chat/test-data'
import type { Lang } from '@/i18n/define'
import type { RadarView } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const RADAR = '/api/conversations/c1/radar'
const RETRY = '/api/conversations/c1/radar/retry'

/** 依序回傳雷達圖狀態；最後一個會一直重複。 */
function mockRadar(...list: RadarView[]) {
  let i = 0
  const calls = { count: 0 }
  server.use(
    http.get(RADAR, () => {
      calls.count += 1
      return HttpResponse.json(list[Math.min(i++, list.length - 1)])
    }),
  )
  return calls
}

function renderPanel(opts: { lang?: Lang; maxPollMs?: number } = {}) {
  // 介面語言跟著帳號語言
  mockMe(makeMe({ student: true }, { language: opts.lang ?? 'zh-TW' }))
  return renderApp(<RadarPanel conversationId="c1" pollMs={5} maxPollMs={opts.maxPollMs ?? 2000} />)
}

describe('RadarPanel: calculation status', () => {
  it('polls while the calculation runs and then reports it as finished', async () => {
    const calls = mockRadar(makeRadarState('pending'), makeRadarState('pending'), makeRadar())
    renderPanel()

    expect(await screen.findByText('正在計算雷達圖…')).toBeInTheDocument()
    expect(await screen.findByText('雷達圖已計算完成。')).toBeInTheDocument()
    expect(screen.queryByText('正在計算雷達圖…')).not.toBeInTheDocument()
    // 完成後不再輪詢
    const settled = calls.count
    await delay(40)
    expect(calls.count).toBe(settled)
  })

  it('is a named region', async () => {
    mockRadar(makeRadar())
    renderPanel()
    expect(await screen.findByRole('region', { name: '單場雷達圖' })).toBeInTheDocument()
  })

  it('shows nothing while the summary is not ready', async () => {
    const calls = mockRadar(makeRadarState('summary_not_ready'))
    renderPanel()
    await waitFor(() => expect(calls.count).toBe(1))
    expect(screen.queryByRole('region', { name: '單場雷達圖' })).not.toBeInTheDocument()
  })

  it('says the calculation is slow after the polling limit and can check again', async () => {
    mockRadar(makeRadarState('pending'))
    renderPanel({ maxPollMs: 30 })
    expect(await screen.findByText('雷達圖計算花的時間比預期久。')).toBeInTheDocument()

    mockRadar(makeRadar())
    await userEvent.click(screen.getByRole('button', { name: '再檢查一次' }))
    expect(await screen.findByText('雷達圖已計算完成。')).toBeInTheDocument()
  })

  it('explains that the topic no longer exists', async () => {
    mockRadar(makeRadarState('unavailable'))
    renderPanel()
    expect(await screen.findByText('這場對話的題目已不存在，無法計算雷達圖。')).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('shows a translated error with retry when the status request fails', async () => {
    server.use(http.get(RADAR, () => apiError(500, 'internal')))
    renderPanel()
    expect(await screen.findByRole('alert')).toBeInTheDocument()

    mockRadar(makeRadar())
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('雷達圖已計算完成。')).toBeInTheDocument()
  })

  it('follows the interface language', async () => {
    mockRadar(makeRadarState('failed', true))
    renderPanel({ lang: 'en' })
    expect(await screen.findByText('The radar chart could not be calculated. Your summary is not affected.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Calculate the radar chart again' })).toBeInTheDocument()
  })
})

describe('RadarPanel: manual retry', () => {
  it('a failed calculation offers retry, posts once and goes back to polling', async () => {
    mockRadar(makeRadarState('failed', true))
    let posts = 0
    server.use(
      http.post(RETRY, async () => {
        posts += 1
        await delay(30)
        return HttpResponse.json(makeRadarState('pending'), { status: 202 })
      }),
    )
    renderPanel()
    expect(await screen.findByText('雷達圖計算失敗，總結不受影響。')).toBeInTheDocument()

    const button = screen.getByRole('button', { name: '重新計算雷達圖' })
    mockRadar(makeRadarState('pending'), makeRadar())
    // 等待中重複點擊不會重複送出
    await userEvent.click(button)
    await userEvent.click(button)
    expect(await screen.findByText('雷達圖已計算完成。')).toBeInTheDocument()
    expect(posts).toBe(1)
  })

  it('keeps the retry available and explains when the retry is refused', async () => {
    mockRadar(makeRadarState('failed', true))
    server.use(http.post(RETRY, () => apiError(409, 'radar_not_retryable')))
    renderPanel()
    await userEvent.click(await screen.findByRole('button', { name: '重新計算雷達圖' }))

    expect(await screen.findByText('目前無法計算雷達圖。')).toBeInTheDocument()
    expect(screen.getByText('雷達圖計算失敗，總結不受影響。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重新計算雷達圖' })).toBeEnabled()
  })

  it.each([
    [403, 'forbidden', '你沒有權限執行這個操作。'],
    [409, 'classroom_archived', '教室已封存，僅供檢視。'],
  ])('shows the backend refusal %s %s', async (status, code, text) => {
    mockRadar(makeRadarState('failed', true))
    server.use(http.post(RETRY, () => apiError(status, code)))
    renderPanel()
    await userEvent.click(await screen.findByRole('button', { name: '重新計算雷達圖' }))
    expect(await screen.findByText(text)).toBeInTheDocument()
  })

  it('waits for the topic dimensions, then lets the student start the calculation', async () => {
    mockRadar(makeRadarState('dimensions_pending'))
    let posts = 0
    server.use(
      http.post(RETRY, () => {
        posts += 1
        return HttpResponse.json(makeRadarState('pending'), { status: 202 })
      }),
    )
    renderPanel()
    expect(await screen.findByText(/這個題目的分析維度尚未完成/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '計算雷達圖' })).not.toBeInTheDocument()

    // 維度完成後再檢查：可以手動觸發補算
    mockRadar(makeRadarState('not_started', true))
    await userEvent.click(screen.getByRole('button', { name: '再檢查一次' }))
    expect(await screen.findByText('這場對話的雷達圖還沒有計算。')).toBeInTheDocument()

    mockRadar(makeRadar())
    await userEvent.click(screen.getByRole('button', { name: '計算雷達圖' }))
    expect(await screen.findByText('雷達圖已計算完成。')).toBeInTheDocument()
    expect(posts).toBe(1)
  })

  it('offers retry when the backend says a running calculation is stuck', async () => {
    mockRadar(makeRadarState('pending', true))
    server.use(http.post(RETRY, () => HttpResponse.json(makeRadarState('pending'), { status: 202 })))
    renderPanel()
    expect(await screen.findByText('雷達圖的計算似乎中斷了。')).toBeInTheDocument()

    mockRadar(makeRadar())
    await userEvent.click(screen.getByRole('button', { name: '重新計算雷達圖' }))
    expect(await screen.findByText('雷達圖已計算完成。')).toBeInTheDocument()
  })

  it('does not offer retry when the backend says it is not retryable', async () => {
    mockRadar(makeRadarState('failed', false))
    renderPanel()
    expect(await screen.findByText('雷達圖計算失敗，總結不受影響。')).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })
})
