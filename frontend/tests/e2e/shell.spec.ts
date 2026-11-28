import type { Page } from '@playwright/test'
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

    // Resting state a11y + screenshot before interactive focus rings
    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'shell-student.png')

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
      await expectScreenshot(page, 'shell-student-drawer.png', { fullPage: false })

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
      await expectScreenshot(page, 'shell-student-sheet.png', { fullPage: false })

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

    // Account menu open (includes 個人設定 for students too), Escape close, focus returns
    const accountBtn = page.getByRole('banner').getByRole('button', { name: /合成學生/ })
    await accountBtn.click()
    await expect(page.getByRole('link', { name: '個人設定' })).toHaveAttribute('href', '/account')
    await expect(page.getByRole('button', { name: '登出' })).toBeVisible()
    await expect(page.getByLabel('語言')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('button', { name: '登出' })).toBeHidden()
    await expect(accountBtn).toBeFocused()
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

  test('topbar location: pointer cursor on the phone trigger and no trailing letter-spacing after the inscription', async ({
    page,
    loginAs,
  }, testInfo) => {
    await loginAs('student')
    const banner = page.getByRole('banner')
    const inscription = banner.locator('[data-vedute-inscription]').filter({ visible: true })
    await expect(inscription).toHaveCount(1)
    // letter-spacing is added after every glyph, including the last one; cancel it so the inscription's box
    // ends at its ink and the gaps to the pin and to the area label (or ⌄) match Figma Page/Topbar 2269:1573.
    const tracking = await inscription.evaluate((el) => {
      const cs = getComputedStyle(el)
      return { letterSpacing: parseFloat(cs.letterSpacing), marginRight: parseFloat(cs.marginRight) }
    })
    expect(tracking.letterSpacing).toBeGreaterThan(0)
    expect(tracking.marginRight).toBeCloseTo(-tracking.letterSpacing, 2)

    if (testInfo.project.name === 'mobile') {
      await expect(banner.getByRole('button', { name: /目前位置/ })).toHaveCSS('cursor', 'pointer')
    } else {
      await expect(banner.getByRole('button', { name: /合成學生/ })).toHaveCSS('cursor', 'pointer')
    }
    if (testInfo.project.name === 'tablet') {
      await expect(banner.getByRole('button', { name: '開啟導覽' })).toHaveCSS('cursor', 'pointer')
    }
  })

  test('focus shell on conversation page shows BackHeader with title and round count without sidebar, tab bar, or colophon', async ({
    page,
    loginAs,
  }) => {
    await loginAs('student')
    await page.goto('/conversations/e2e-conv-1')
    const header = page.getByRole('banner')
    await expect(header.getByRole('link', { name: /返回我的對話/ })).toBeVisible()
    await expect(header.getByText('正義是強者的利益嗎？')).toBeVisible()
    await expect(header.getByText('講壇・第 1 輪')).toBeVisible()
    await expect(page.getByRole('heading', { level: 1, name: '正義是強者的利益嗎？' })).toBeVisible()
    await expect(page.getByRole('navigation', { name: '主導覽' })).toBeHidden()
    await expect(page.getByRole('navigation', { name: '主要區域' })).toBeHidden()
    await expect(page.getByRole('contentinfo')).toBeHidden()
    // Exclude pre-existing legacy voice button (aria-label="語音對話" / visible "即時") in voice-controls.tsx; rebuilt in #683.
    await expectNoA11yViolations(page, { exclude: ['button[aria-label="語音對話"]'] })
    await expectScreenshot(page, 'shell-focus.png')
  })

  test('drawer and sheet play CSS slide transitions and close when crossing breakpoints', async ({
    page,
    loginAs,
  }, testInfo) => {
    const project = testInfo.project.name
    if (project === 'desktop') return

    await page.emulateMedia({ reducedMotion: 'no-preference' })
    await loginAs('student')
    await expect(page.getByRole('heading', { level: 1, name: '選擇題目' })).toBeVisible()

    if (project === 'tablet') {
      const menuBtn = page.getByRole('button', { name: '開啟導覽' })
      await menuBtn.click()
      const drawer = page.getByRole('dialog', { name: '主導覽' })
      await expect(drawer).toHaveClass(/is-open/)
      const duration = await drawer.evaluate((el) => getComputedStyle(el).transitionDuration)
      expect(duration).toBe('0.24s')

      // Crossing 768 -> 1024 closes the drawer so it never overlays the persistent desktop sidebar
      await page.setViewportSize({ width: 1024, height: 900 })
      await expect(drawer).toBeHidden()
      await expect(page.getByRole('navigation', { name: '主導覽' })).toBeVisible()
    } else {
      const tabBar = page.getByRole('navigation', { name: '主要區域' })
      await tabBar.getByRole('button', { name: /廣場/ }).click()
      const sheet = page.getByRole('dialog', { name: /Pianta del Foro/i })
      await expect(sheet).toHaveClass(/is-open/)
      const duration = await sheet.evaluate((el) => getComputedStyle(el).transitionDuration)
      expect(duration).toBe('0.32s')

      // Crossing 375 -> 768 closes the sheet and releases focus trap so Tab reaches visible controls
      await page.setViewportSize({ width: 768, height: 1024 })
      await expect(sheet).toBeHidden()
      await page.keyboard.press('Tab')
      const focusedInsideSheet = await page.evaluate(
        () => document.activeElement?.closest('#vedute-plan-sheet') !== null,
      )
      expect(focusedInsideSheet).toBe(false)
    }
  })
})

// 頂欄中央的對齊以「墨跡」量，不看行框（2026-11-29 @whyhugo 決定）：
// 銘文大寫字的中心對齊紅圈中心（手機沒有紅圈，對齊頂欄中心）、紅圈數字在圈內置中、中文區名的字底對齊銘文的字底。
// 文字會對齊整數像素，所以容差 0.5px；修正前的偏差是 0.6–2.2px。
test.describe('topbar location optical alignment', () => {
  test.use({ deviceScaleFactor: 4 })

  // Vertical ink extent (CSS px, page coordinates) inside a clip, from a device-scale screenshot.
  async function inkRows(page: Page, clip: { x: number; y: number; width: number; height: number }) {
    const png = await page.screenshot({ clip, scale: 'device' })
    return page.evaluate(
      async ({ b64, clip }) => {
        const img = new Image()
        img.src = `data:image/png;base64,${b64}`
        await img.decode()
        const canvas = document.createElement('canvas')
        canvas.width = img.width
        canvas.height = img.height
        const ctx = canvas.getContext('2d')!
        ctx.drawImage(img, 0, 0)
        const data = ctx.getImageData(0, 0, img.width, img.height).data
        const luma = (i: number) => 0.3 * data[i] + 0.59 * data[i + 1] + 0.11 * data[i + 2]
        const paper = luma(0)
        let top = Infinity
        let bottom = -1
        for (let y = 0; y < img.height; y++)
          for (let x = 0; x < img.width; x++)
            if (paper - luma((y * img.width + x) * 4) > 60) {
              top = Math.min(top, y)
              bottom = Math.max(bottom, y)
            }
        const scale = img.width / clip.width
        return { top: clip.y + top / scale, bottom: clip.y + (bottom + 1) / scale }
      },
      { b64: png.toString('base64'), clip },
    )
  }
  const mid = (r: { top: number; bottom: number }) => (r.top + r.bottom) / 2

  test('inscription centres on the pin, numeral centres in the pin, area label shares the inscription bottom', async ({
    page,
    loginAs,
  }, testInfo) => {
    await loginAs('student')
    const banner = page.getByRole('banner')
    const inscription = banner.locator('[data-vedute-inscription]').filter({ visible: true })
    await expect(inscription).toBeVisible()
    await page.evaluate(() => document.fonts.ready)

    const header = (await banner.boundingBox())!
    const insc = (await inscription.boundingBox())!
    // 頂欄高 52 含 1px 下框線
    let centre = header.y + (header.height - 1) / 2
    const inkClip = { x: insc.x, y: header.y, width: insc.width, height: header.height - 1 }

    if (testInfo.project.name !== 'mobile') {
      const pin = (await banner.locator('.vedute-pin').filter({ visible: true }).boundingBox())!
      centre = pin.y + pin.height / 2
      // 圈線 1.2px：往內縮 7px 只留下數字
      const numeral = await inkRows(page, { x: pin.x + 7, y: pin.y + 7, width: pin.width - 14, height: pin.height - 14 })
      expect(Math.abs(mid(numeral) - centre), 'numeral centred in the pin').toBeLessThanOrEqual(0.5)
    }

    const latin = await inkRows(page, inkClip)
    expect(Math.abs(mid(latin) - centre), 'inscription centred on the pin').toBeLessThanOrEqual(0.5)

    if (testInfo.project.name === 'desktop') {
      const label = (await banner.getByText('我的教室', { exact: true }).boundingBox())!
      const zh = await inkRows(page, { x: label.x, y: header.y, width: label.width, height: header.height - 1 })
      expect(Math.abs(zh.bottom - latin.bottom), 'area label bottom on the inscription bottom').toBeLessThanOrEqual(0.5)
    }

    // 頁首右上角的大圓章（Page/Header 28px / 14px）也是數字在圈內置中
    const headerPin = (await page.locator('.vedute-pin-lg').filter({ visible: true }).first().boundingBox())!
    const headerNumeral = await inkRows(page, {
      x: headerPin.x + 7,
      y: headerPin.y + 7,
      width: headerPin.width - 14,
      height: headerPin.height - 14,
    })
    expect(Math.abs(mid(headerNumeral) - (headerPin.y + headerPin.height / 2)), 'numeral centred in the header pin').toBeLessThanOrEqual(0.5)
  })
})
