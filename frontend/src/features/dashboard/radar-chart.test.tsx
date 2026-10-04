import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { RadarChart } from '@/features/dashboard/radar-chart'
import { I18nProvider } from '@/i18n'

const polygons = () => Array.from(screen.getByTestId('radar-chart').querySelectorAll('polygon'))

describe('RadarChart', () => {
  it('puts each score on its own axis, from the top clockwise, and caps values at the maximum', () => {
    render(
      <I18nProvider initial="zh-TW">
        <RadarChart
          label="單場"
          max={5}
          scores={{ utilitarianism: 5, deontology: 0, virtue: 0, contractarianism: 0, care: 0, existentialism: 99 }}
        />
      </I18nProvider>,
    )
    const data = polygons().at(-1)!.getAttribute('points')!.split(' ').map((p) => p.split(',').map(Number))
    expect(data).toHaveLength(6)
    const [cx, cy] = [220, 160]
    // 效益主義在正上方、滿分：距中心 100
    expect(data[0][0]).toBeCloseTo(cx, 0)
    expect(data[0][1]).toBeCloseTo(cy - 100, 0)
    // 0 分落在中心
    expect(data[1][0]).toBeCloseTo(cx, 0)
    expect(data[1][1]).toBeCloseTo(cy, 0)
    // 超過滿分的值夾在外圈（存在主義是第 6 軸）
    expect(Math.hypot(data[5][0] - cx, data[5][1] - cy)).toBeCloseTo(100, 0)
    expect(screen.getByRole('img', { name: '單場' })).toBeInTheDocument()
    expect(screen.getByText('存在主義：5 / 5')).toBeInTheDocument()
  })

  it('treats a missing school as 0', () => {
    render(
      <I18nProvider initial="en">
        <RadarChart label="x" max={5} scores={{ care: 3 }} />
      </I18nProvider>,
    )
    expect(screen.getByText('Ethics of care: 3 / 5')).toBeInTheDocument()
    expect(screen.getByText('Deontology: 0 / 5')).toBeInTheDocument()
  })
})
