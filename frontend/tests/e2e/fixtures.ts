import AxeBuilder from '@axe-core/playwright'
import { test as base, expect, type Page } from '@playwright/test'
import { E2E_ACCOUNTS, E2E_PASSWORD, type E2eRole } from '../../src/test/e2e-accounts'

export { expect }
export type { E2eRole }

/** 所有測試共用的固定時間（Asia/Taipei 上午 9 點），讓日期、「幾分鐘前」等文字與截圖穩定。 */
export const FIXED_NOW = new Date('2026-11-02T01:00:00Z')

type Fixtures = {
  /** 以合成帳號從登入頁登入，等到離開登入頁為止。 */
  loginAs: (role: E2eRole) => Promise<void>
}

// fixture 的第二個參數照 Playwright 文件慣例叫 `use`，但會被 oxlint 誤認成 React 的 use()，所以改叫 run
export const test = base.extend<Fixtures>({
  page: async ({ page }, run) => {
    // 沒有假資料的 /api 請求（src/test/browser.ts 的 onUnhandledRequest）與未捕捉的例外都讓測試失敗
    const problems: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error' && msg.text().includes('[MSW]')) problems.push(msg.text())
    })
    page.on('pageerror', (err) => problems.push(`pageerror: ${err.message}`))
    await page.clock.setFixedTime(FIXED_NOW)
    await run(page)
    expect(problems, 'unhandled /api requests or page errors').toEqual([])
  },
  loginAs: async ({ page }, run) => {
    await run(async (role) => {
      await page.goto('/login')
      await page.getByLabel('學號或電子郵件').fill(E2E_ACCOUNTS[role])
      await page.getByLabel('密碼').fill(E2E_PASSWORD)
      await page.getByRole('button', { name: '登入', exact: true }).click()
      await page.waitForURL((url) => url.pathname !== '/login')
    })
  },
})

/** axe 掃描整頁；serious、critical 視為失敗（S-11.6）。完整結果附在測試報告裡。 */
export async function expectNoA11yViolations(page: Page, { exclude = [] }: { exclude?: string[] } = {}) {
  let builder = new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
  for (const selector of exclude) builder = builder.exclude(selector)
  const results = await builder.analyze()
  await test.info().attach('axe-results.json', { body: JSON.stringify(results.violations, null, 2), contentType: 'application/json' })
  const blocking = results.violations
    .filter((v) => v.impact === 'serious' || v.impact === 'critical')
    .map((v) => `${v.impact} ${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(' ')).join(', ')})`)
  expect(blocking, 'axe serious/critical violations').toEqual([])
}

/** 整頁截圖比對：先等字型載入完成，避免字型還在下載時截到後備字型。 */
export async function expectScreenshot(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready)
  await expect(page).toHaveScreenshot(name, { fullPage: true })
}
