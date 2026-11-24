import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

for (const scenario of ['complete', 'partial', 'unknown'] as const) {
  test(`signed stance radar: ${scenario}`, async ({ page, loginAs }) => {
    await loginAs('student')
    await page.goto(`/conversations/stance-${scenario}`)
    await expect(page.getByTestId('radar-chart')).toBeVisible()
    const missing = scenario === 'unknown' ? 6 : scenario === 'partial' ? 2 : 0
    await expect(page.getByText('證據不足', { exact: true })).toHaveCount(missing)
    await expect(page.getByTestId('stance-point')).toHaveCount(scenario === 'unknown' ? 0 : scenario === 'partial' ? 4 : 6)
    if (scenario !== 'unknown') {
      await expect(page.getByText('-2：強烈反對', { exact: true })).toBeVisible()
      await expect(page.getByText('0：無感／不在乎', { exact: true }).first()).toBeVisible()
    }
    await page.locator('summary').filter({ hasText: '合成傾向1' }).click()
    await expect(page.getByText('合成說明1', { exact: true })).toBeVisible()
    await page.getByRole('log').focus()
    await expect(page.getByRole('log')).toBeFocused()
    await page.getByTestId('radar-chart').scrollIntoViewIfNeeded()
    await expect(page.getByTestId('radar-chart')).toBeInViewport({ ratio: 1 })
    await expectNoA11yViolations(page)
    await expectScreenshot(page, `stance-${scenario}.png`)
  })
}