import { screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { RadarResult } from '@/features/chat/radar-chart'
import { makeRadar } from '@/features/chat/test-data'
import type { Lang } from '@/i18n/define'
import type { RadarView } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'

async function renderResult(radar: RadarView, lang: Lang = 'zh-TW') {
  // 介面語言跟著帳號語言
  mockMe(makeMe({ student: true }, { language: lang }))
  renderApp(<RadarResult radar={radar} />)
  // 等帳號語言套用完成
  const settled = { 'zh-TW': /單場分析|沒有可以顯示/, en: /Single-discussion|no radar chart data/, es: /Análisis de un solo|No hay datos/ }[lang]
  await screen.findByText(settled)
}

describe('RadarResult', () => {
  it('draws the topic dimensions and lists every score out of 5', async () => {
    await renderResult(makeRadar())

    const chart = screen.getByRole('img', { name: '單場雷達圖：這個題目各維度的分數，0 到 5 分' })
    expect(chart).toHaveAttribute('data-testid', 'radar-chart')
    const items = screen.getAllByRole('listitem')
    expect(items).toHaveLength(6)
    expect(within(items[0]).getByText('合成傾向1')).toBeInTheDocument()
    expect(within(items[0]).getByText('5 / 5')).toBeInTheDocument()
    // 0 分照樣列出，只顯示 0
    expect(within(items[1]).getByText('合成傾向2')).toBeInTheDocument()
    expect(within(items[1]).getByText('0 / 5')).toBeInTheDocument()
    expect(within(items[5]).getByText('2 / 5')).toBeInTheDocument()
  })

  it('plots each score on its own axis, starting from the top', async () => {
    await renderResult(makeRadar())
    const points = screen.getByTestId('radar-shape').getAttribute('points')!.split(' ')
    expect(points).toHaveLength(6)
    // d1 = 5：正上方的最外圈；d2 = 0：圓心
    expect(points[0]).toBe('220.0,60.0')
    expect(points[1]).toBe('220.0,160.0')
    // d4 = 1：正下方的第一圈
    expect(points[3]).toBe('220.0,180.0')
  })

  it('keeps each dimension description in an expandable block', async () => {
    await renderResult(makeRadar())
    const first = screen.getAllByRole('listitem')[0]
    const details = first.querySelector('details')!
    expect(details).not.toHaveAttribute('open')
    expect(within(details).getByText('合成說明1')).toBeInTheDocument()
  })

  it('marks the result as a single-discussion analysis that is not a diagnosis', async () => {
    await renderResult(makeRadar())
    const note = screen.getByText(/單場分析/)
    expect(note).toHaveTextContent('只根據這一場對話計算')
    expect(note).toHaveTextContent('不代表固定的人格，也不是心理診斷')
  })

  it.each([
    ['en', 'Synthetic tendency 3', 'Synthetic description 3', 'not a psychological diagnosis'],
    ['es', 'Tendencia sintética 3', 'Descripción sintética 3', 'ni un diagnóstico psicológico'],
  ] as const)('shows names and descriptions in the interface language (%s)', async (lang, name, description, disclaimer) => {
    await renderResult(makeRadar(), lang)
    const third = screen.getAllByRole('listitem')[2]
    expect(within(third).getByText(name)).toBeInTheDocument()
    expect(within(third).getByText(description)).toBeInTheDocument()
    expect(within(third).getByText('3 / 5')).toBeInTheDocument()
    expect(screen.getByText(new RegExp(disclaimer))).toBeInTheDocument()
    expect(screen.queryByText('合成傾向3')).not.toBeInTheDocument()
  })

  it('shortens a long name on the axis but keeps it whole in the list', async () => {
    const long = '這是一個非常非常非常長的合成維度名稱用來測試'
    const radar = makeRadar()
    radar.dimensions![0].name.zh_tw = long
    await renderResult(radar)

    expect(within(screen.getAllByRole('listitem')[0]).getByText(long)).toBeInTheDocument()
    const axisLabel = screen.getByTestId('radar-chart').querySelector('text')!
    expect(axisLabel.textContent).toBe(`${long.slice(0, 13)}…`)
  })

  it('explains an all-zero result instead of leaving an empty chart unexplained', async () => {
    await renderResult(makeRadar({ scores: { d1: 0, d2: 0, d3: 0, d4: 0, d5: 0, d6: 0 } }))
    expect(screen.getByTestId('radar-chart')).toBeInTheDocument()
    expect(screen.getByText('這場討論的內容還不足以判斷各個傾向，所以每個維度都是 0 分。')).toBeInTheDocument()
    expect(screen.getAllByText('0 / 5')).toHaveLength(6)
  })

  it('does not explain zeros when only some dimensions are 0', async () => {
    await renderResult(makeRadar())
    expect(screen.queryByText(/每個維度都是 0 分/)).not.toBeInTheDocument()
  })

  it.each([
    ['scores are missing', makeRadar({ scores: null })],
    ['dimensions are missing', makeRadar({ dimensions: null })],
    ['a dimension has no score', makeRadar({ scores: { d1: 5, d2: 0, d3: 3, d4: 1, d5: 0 } })],
    ['a score is out of range', makeRadar({ scores: { d1: 6, d2: 0, d3: 3, d4: 1, d5: 0, d6: 2 } })],
    ['a score is not an integer', makeRadar({ scores: { d1: 2.5, d2: 0, d3: 3, d4: 1, d5: 0, d6: 2 } })],
  ])('shows an empty state and no partial chart when %s', async (_, radar) => {
    await renderResult(radar)
    expect(screen.getByText('這場對話目前沒有可以顯示的雷達圖資料。')).toBeInTheDocument()
    expect(screen.queryByTestId('radar-chart')).not.toBeInTheDocument()
    expect(screen.queryByRole('listitem')).not.toBeInTheDocument()
  })
})
