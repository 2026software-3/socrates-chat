import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

test.describe('Vedute work shell across roles and viewports', () => {
  test('student work shell: landmarks, navigation, account menu, drawer/sheet, axe, and screenshots', async ({
    page,
    loginAs,
  }, testInfo) => {
    await loginAs('student')
    await expect(page.getByRole('heading', { level: 1, name: '選擇題目' })).toBeVisible()
    await expect(page.getByRole('banner')).toBeVisible()
    await expect(page.getByRole('main')).toBeVisible()
    await expect(page.getByRole('contentinfo')).toBeVisible()

    // No horizontal overflow at any viewport
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    const project = testInfo.project.name

    if (project === 'desktop') {
      const sidebar = page.getByRole('navigation', { name: '主導覽' })
      await expect(sidebar).toBeVisible()
      await expect(sidebar.getByRole('link', { name: /我的教室/ })).toHaveAttribute('aria-current', 'page')
      await expect(page.getByRole('banner').getByText('Schola Socratica')).toBeVisible()
      await expect(page.getByRole('button', { name: '開啟導覽' })).toBeHidden()
      await expect(page.getByRole('navigation', { name: '主要區域' })).toBeHidden()
    } else if (project === 'tablet') {
      const menuBtn = page.getByRole('button', { name: '開啟導覽' })
      await expect(menuBtn).toBeVisible()
      await expect(page.getByRole('navigation', { name: '主要區域' })).toBeHidden()

      await menuBtn.click()
      const drawer = page.getByRole('dialog', { name: '主導覽' })
      await expect(drawer).toBeVisible()
      await expect(drawer.getByRole('link', { name: /我的教室/ })).toHaveAttribute('aria-current', 'page')
      await expectNoA11yViolations(page)
      await expectScreenshot(page, 'shell-student-drawer.png')

      await page.keyboard.press('Escape')
      await expect(drawer).toBeHidden()
      await expect(menuBtn).toBeFocused()
    } else {
      // mobile (<768)
      await expect(page.getByRole('button', { name: '開啟導覽' })).toBeHidden()
      const tabBar = page.getByRole('navigation', { name: '主要區域' })
      await expect(tabBar).toBeVisible()
      await expect(tabBar.getByRole('link', { name: /我的教室/ })).toHaveAttribute('aria-current', 'page')

      // Open PlanSheet via Plaza tab
      const plazaBtn = tabBar.getByRole('button', { name: /廣場/ })
      await plazaBtn.click()
      const sheet = page.getByRole('dialog', { name: /Pianta del Foro/i })
      await expect(sheet).toBeVisible()
      await page.mouse.move(0, 0)
      await expectNoA11yViolations(page)
      await expectScreenshot(page, 'shell-student-sheet.png')

      // Escape closes sheet and returns focus to Plaza button
      await page.keyboard.press('Escape')
      await expect(sheet).toBeHidden()
      await expect(plazaBtn).toBeFocused()

      // Topbar carved location button also opens PlanSheet
      const locBtn = page.getByRole('banner').getByRole('button', { name: /目前位置/ })
      await locBtn.click()
      await expect(sheet).toBeVisible()
      await sheet.getByRole('link', { name: /點數與任務/ }).click()
      await expect(page).toHaveURL(/\/tasks$/)
      await expect(sheet).toBeHidden()
      // Non-frequent area (IV) highlights Plaza button with IV・點數
      await expect(tabBar.getByRole('button', { name: /IV・點數/ })).toHaveAttribute('aria-current', 'page')
      await page.goto('/')
    }

    // Account menu open, Escape close, focus returns
    const accountBtn = page.getByRole('banner').getByRole('button', { name: /合成學生/ })
    await accountBtn.click()
    await expect(page.getByRole('button', { name: '登出' })).toBeVisible()
    await expect(page.getByLabel('語言')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('button', { name: '登出' })).toBeHidden()
    await expect(accountBtn).toBeFocused()

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'shell-student.png')
  })

  test('teacher work shell across viewports with axe and screenshots', async ({ page, loginAs }, testInfo) => {
    await loginAs('teacher')
    await expect(page).toHaveURL(/\/teacher\/classrooms$/)
    await expect(page.getByRole('heading', { level: 1, name: '我的教室' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    if (testInfo.project.name === 'desktop') {
      const sidebar = page.getByRole('navigation', { name: '主導覽' })
      await expect(sidebar.getByRole('link')).toHaveCount(5)
      await expect(sidebar.getByRole('link', { name: /教室管理/ })).toHaveAttribute('aria-current', 'page')
    } else if (testInfo.project.name === 'mobile') {
      const tabBar = page.getByRole('navigation', { name: '主要區域' })
      await expect(tabBar.getByRole('link', { name: /教室管理/ })).toHaveAttribute('aria-current', 'page')
      await expect(tabBar.getByRole('link', { name: /題目庫/ })).toBeVisible()
      await expect(tabBar.getByRole('link', { name: /學生總結/ })).toBeVisible()
    }

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'shell-teacher.png')
  })

  test('admin work shell across viewports with grouped sub-tabs, axe, and screenshots', async ({
    page,
    loginAs,
  }, testInfo) => {
    await loginAs('admin')
    await expect(page).toHaveURL(/\/admin$/)
    await expect(page.getByRole('heading', { level: 1, name: '系統維運' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    if (testInfo.project.name === 'desktop') {
      const sidebar = page.getByRole('navigation', { name: '主導覽' })
      await expect(sidebar.getByRole('link')).toHaveCount(7)
      await expect(sidebar.getByRole('link', { name: /總覽/ })).toHaveAttribute('aria-current', 'page')
    }

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'shell-admin.png')
  })

  test('focus shell on conversation page shows BackHeader without sidebar, tab bar, or colophon', async ({
    page,
    loginAs,
  }) => {
    await loginAs('student')
    await page.goto('/conversations/e2e-conv-1')
    await expect(page.getByRole('link', { name: /返回我的對話/ })).toBeVisible()
    await expect(page.getByRole('heading', { level: 1, name: '正義是強者的利益嗎？' })).toBeVisible()
    await expect(page.getByRole('navigation', { name: '主導覽' })).toBeHidden()
    await expect(page.getByRole('navigation', { name: '主要區域' })).toBeHidden()
    await expect(page.getByRole('contentinfo')).toBeHidden()
    // Exclude pre-existing legacy voice button (aria-label="語音對話" / visible "即時") in voice-controls.tsx; rebuilt in #683.
    await expectNoA11yViolations(page, { exclude: ['button[aria-label="語音對話"]'] })
    await expectScreenshot(page, 'shell-focus.png')
  })
})
