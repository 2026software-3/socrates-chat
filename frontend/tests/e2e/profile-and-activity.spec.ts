import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

test.describe('profile tabs (settings, stats, privacy) and activity detail', () => {
  test('student profile switches between settings, stats, and privacy tabs with a11y and screenshots', async ({
    page,
    loginAs,
  }) => {
    test.setTimeout(60_000)
    await loginAs('student')
    await page.goto('/account')

    await expect(page.getByRole('heading', { level: 1, name: '個人檔案' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'account-settings.png')

    // 點擊切換至「統計資料」頁籤
    const statsTab = page.getByRole('tab', { name: '統計資料' })
    await statsTab.click()
    await expect(statsTab).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('heading', { name: '最近完成的討論' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'account-stats.png')

    // 點擊切換至「隱私設定」頁籤
    const privacyTab = page.getByRole('tab', { name: '隱私設定' })
    await privacyTab.click()
    await expect(privacyTab).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('heading', { name: '同學看得到什麼' })).toBeVisible()
    await expect(page.getByRole('heading', { name: '同學看到的樣子（預覽）' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'account-privacy.png')
  })

  test('student opens classroom activity detail from home with a11y and screenshots', async ({ page, loginAs }) => {
    await loginAs('student')
    await expect(page.getByRole('heading', { level: 1, name: '選擇題目' })).toBeVisible()

    // 點擊首頁教室活動「什麼是正義？」底下的題目進入活動詳細頁
    await page.getByRole('link', { name: /正義是強者的利益嗎？/ }).first().click()
    await expect(page).toHaveURL(/\/topics\/topic-justice\?activity=act-1$/)

    await expect(page.getByRole('heading', { level: 1, name: '正義是強者的利益嗎？' })).toBeVisible()
    await expect(page.getByText('開放中')).toBeVisible()
    await expect(page.getByRole('heading', { name: '活動說明' })).toBeVisible()
    await expect(page.getByRole('heading', { name: '引導問題' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'activity-detail.png')
  })
})