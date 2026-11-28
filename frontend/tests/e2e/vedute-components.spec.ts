import type { Locator } from '@playwright/test'
import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

// Capture the short press state in the browser before its release timer can expire.
async function quickPress(button: Locator) {
  return button.evaluate(async (element: HTMLElement) => {
    element.click()
    await Promise.resolve()
    const style = getComputedStyle(element)
    return {
      pressed: element.hasAttribute('data-pressed'),
      background: style.backgroundColor,
      color: style.color,
      translate: style.translate,
      scale: style.scale,
    }
  })
}

test('Vedute component states, keyboard and pointer interactions', async ({ page }) => {
  await page.goto('/__design')
  await expect(page.getByRole('heading', { name: 'Vedute 基礎元件' })).toBeVisible()

  const notifications = page.getByRole('switch', { name: '活動通知' })
  await notifications.focus()
  await page.keyboard.press('Space')
  await expect(notifications).toBeChecked()
  await expect(page.getByRole('switch', { name: '公開反思' })).toBeDisabled()

  const field = page.getByRole('textbox', { name: '活動名稱' })
  await expect(field).toHaveAccessibleDescription('請填寫活動名稱。')
  await field.fill('正義的意義')

  await page.getByRole('combobox', { name: '教室' }).click()
  await page.getByRole('option', { name: '倫理學討論室' }).click()
  await expect(page.getByRole('combobox', { name: '教室' })).toHaveText('倫理學討論室')

  const activeTab = page.getByRole('tab', { name: '進行中' })
  await activeTab.focus()
  await page.keyboard.press('ArrowRight')
  await expect(page.getByRole('tab', { name: '已封存' })).toBeFocused()
  await expect(page.getByRole('tabpanel')).toHaveText('沒有已封存的活動。')

  const pager = page.getByRole('navigation', { name: '教室頁碼' })
  await expect(pager.getByRole('button', { name: '上一頁' })).toBeDisabled()
  await pager.getByRole('button', { name: '下一頁' }).click()
  await expect(pager.getByRole('button', { name: '第 2 頁' })).toHaveAttribute('aria-current', 'page')

  const toastTrigger = page.getByRole('button', { name: '顯示儲存失敗' })
  const error = page.getByRole('alert').filter({ hasText: '儲存失敗，請重試。' })
  await toastTrigger.click()
  await expect(error).toBeAttached()
  await page.getByRole('button', { name: '重試', exact: true }).click()
  await expect(error).toHaveCount(0)
  await expect(page.getByRole('status').filter({ hasText: '討論活動已儲存。' })).toBeAttached()
  await page.getByRole('button', { name: '關閉通知' }).click()
  await expect(page.getByRole('button', { name: '關閉通知' })).toHaveCount(0)
})

test('Vedute interaction motion and disabled options', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  await page.goto('/__design')

  const field = page.getByRole('textbox', { name: '活動名稱' })
  await field.focus()
  await expect(field).toHaveCSS('background-size', '100% 2px, 100% 1px')

  const tab = page.getByRole('tab', { name: '已封存' })
  await tab.click()
  await expect(tab).toHaveAttribute('aria-selected', 'true')
  expect(await tab.evaluate((el) => getComputedStyle(el, '::after').transitionProperty)).toBe('scale')

  const switchThumb = page.getByRole('switch', { name: '活動通知' }).locator('[data-slot="switch-thumb"]')
  await expect(switchThumb).toHaveCSS('transition-duration', '0.32s, 0.25s')

  for (const name of ['建立活動', '取消', '上一步']) {
    const button = page.getByRole('button', { name, exact: true })
    const resting = await button.evaluate((el) => getComputedStyle(el).backgroundColor)
    await button.hover()
    if (name !== '取消') await expect(button).not.toHaveCSS('background-color', resting)
    await button.evaluate(async (el) => {
      await Promise.all(el.getAnimations().map((animation) => animation.finished))
    })
    const hovered = await button.evaluate((el) => getComputedStyle(el).backgroundColor)
    await page.mouse.down()
    await expect(button).not.toHaveCSS('background-color', hovered)
    await expect(button).toHaveCSS('translate', 'none')
    await page.mouse.up()
    await expect(button).toHaveCSS('background-color', hovered)
  }

  for (const name of ['取消', '上一步']) {
    const button = page.getByRole('button', { name, exact: true })
    await button.hover()
    await button.evaluate(async (el) => {
      await Promise.all(el.getAnimations().map((animation) => animation.finished))
    })
    const hovered = await button.evaluate((el) => getComputedStyle(el).backgroundColor)
    const pressed = await quickPress(button)
    expect(pressed).toMatchObject({ pressed: true, translate: 'none', scale: 'none' })
    expect(pressed.background).not.toBe(hovered)
    await expect(button).not.toHaveAttribute('data-pressed')
    await expect(button).toHaveCSS('background-color', hovered)
  }

  const pager = page.getByRole('navigation', { name: '教室頁碼' })
  const pageTwo = pager.getByRole('button', { name: '第 2 頁' })
  const nextPage = pager.getByRole('button', { name: '下一頁' })
  const pressedPageColor = await pager.evaluate((el) => {
    const probe = document.createElement('span')
    probe.style.color = 'color-mix(in srgb, var(--color-chalk) 75%, var(--color-chalk-deep))'
    el.append(probe)
    const color = getComputedStyle(probe).color
    probe.remove()
    return color
  })
  for (const [control, selected] of [[pageTwo, pageTwo], [nextPage, pager.getByRole('button', { name: '第 3 頁' })]]) {
    await control.hover()
    await expect(control).toHaveCSS('color', 'rgb(169, 68, 43)')
    await expect(control).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
    expect(await quickPress(control)).toMatchObject({
      pressed: true, color: pressedPageColor, background: 'rgba(0, 0, 0, 0)',
    })
    await expect(selected).toHaveAttribute('aria-current', 'page')
    await expect(control).toHaveCSS('color', 'rgb(169, 68, 43)')
  }

  const help = page.getByRole('link', { name: '操作通知說明' })
  await help.hover()
  await page.mouse.down()
  await expect(help).toHaveCSS('text-decoration-thickness', '2px')
  await expect(help).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await page.mouse.move(0, 0)
  await page.mouse.up()

  const icon = page.getByRole('button', { name: '編輯', exact: true })
  const normalIconColor = await icon.evaluate((el) => getComputedStyle(el).color)
  await icon.hover()
  await expect(icon).not.toHaveCSS('color', normalIconColor)
  await expect(icon.locator('svg')).toHaveCSS('stroke-width', '2.5px')
  await expect(icon).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await page.mouse.down()
  await expect(icon).toHaveCSS('translate', 'none')
  await expect(icon.locator('svg')).toHaveCSS('scale', 'none')
  await expect(icon.locator('svg')).toHaveCSS('stroke-width', '3px')
  await page.mouse.up()

  const saving = page.getByRole('button', { name: '儲存中…' })
  await saving.hover()
  await page.mouse.down()
  await expect(saving).toHaveCSS('translate', 'none')
  await page.mouse.up()

  await page.getByRole('combobox', { name: '教室' }).click()
  const enabledOption = page.getByRole('option', { name: '倫理學討論室' })
  await enabledOption.hover()
  // 指到的選項：深紙底＋左側紅粉筆線，不加底線
  await expect(enabledOption).toHaveCSS('background-color', 'rgb(221, 212, 190)')
  await expect(enabledOption).toHaveCSS('text-decoration-line', 'none')
  const archived = page.getByRole('option', { name: /形上學討論室/ })
  await expect(archived).toBeDisabled()
  await expect(archived).toContainText('已封存')
  await archived.hover()
  await expect(archived).toHaveCSS('cursor', 'not-allowed')
  await expect(archived).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await expect(archived).toHaveCSS('color', 'rgb(74, 64, 56)')
  await expect(archived.locator('span').first()).toHaveCSS('text-decoration-line', 'line-through')
  await archived.click({ force: true })
  await expect(page.getByRole('listbox')).toBeVisible()
  await page.keyboard.press('Escape')

  await page.getByRole('button', { name: '顯示儲存失敗' }).click()
  const notice = page.locator('.vedute-toast')
  await expect(notice).toHaveCSS('animation-name', 'vedute-toast-enter')
  // 取代與關閉的離場動畫只有 150–250ms，不在 e2e 取樣中間狀態（機器忙時會錯過）；這裡只驗證結果，
  // 動畫參數由 primitives.css 決定。
  await page.getByRole('button', { name: '重試' }).click()
  await expect(notice).toHaveCount(1)
  await expect(notice).toContainText('討論活動已儲存。')
  await expect(notice).not.toContainText('儲存失敗，請重試。')
  await expect(notice).toHaveCSS('animation-name', 'vedute-toast-enter')
  await page.getByRole('button', { name: '關閉通知' }).click()
  await expect(notice).toHaveCount(0)
})

test('Vedute select distinguishes the current value from keyboard focus', async ({ page }) => {
  await page.goto('/__design')
  const trigger = page.getByRole('combobox', { name: '教室' })
  await trigger.click()
  await page.getByRole('option', { name: '倫理學討論室' }).click()
  const fieldBox = (await trigger.boundingBox())!
  await trigger.press('Enter')

  const menu = page.getByRole('listbox')
  const current = menu.getByRole('option', { name: '倫理學討論室' })
  const next = menu.getByRole('option', { name: '知識論討論室' })
  await expect(current).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(next).toBeFocused()
  // 已選：粗體＋墨色勾；鍵盤指到：深紙底＋左側紅粉筆線（::before）
  await expect(current).toHaveCSS('font-weight', '700')
  await expect(current.locator('[data-vedute-check] svg')).toBeVisible()
  await expect(current).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await expect(next).toHaveCSS('background-color', 'rgb(221, 212, 190)')
  await expect(next.locator('[data-vedute-check]')).toHaveCount(0)
  expect(await next.evaluate((el) => getComputedStyle(el, '::before').backgroundColor)).toBe('rgb(169, 68, 43)')
  await expect(menu).toHaveCSS('box-shadow', 'none')
  const menuBox = (await menu.boundingBox())!
  expect(Math.abs(menuBox.x - fieldBox.x)).toBeLessThanOrEqual(1)
  expect(menuBox.width).toBeGreaterThanOrEqual(fieldBox.width - 1)
  // Popper may open above the field when the viewport has less space below it.
  expect(menuBox.y >= fieldBox.y + fieldBox.height || menuBox.y + menuBox.height <= fieldBox.y).toBe(true)

  await page.keyboard.press('Tab')
  await expect(next).toBeFocused()
  await page.mouse.move(0, 0)
  await expect(menu).toBeVisible()
  await page.evaluate(() => document.fonts.ready)
  await expect(menu).toHaveScreenshot('vedute-select.png')
  await page.keyboard.press('Escape')
  await expect(trigger).toHaveText('倫理學討論室')
  await expect(trigger).toBeFocused()
  await expectNoA11yViolations(page)
})

test('Vedute visual baseline and accessibility', async ({ page }) => {
  await page.goto('/__design')
  await page.getByRole('button', { name: '顯示儲存失敗' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '無法載入學生名單。' })).toHaveCSS('font-size', '13px')
  const disabled = page.getByRole('button', { name: '儲存中…' })
  await expect(disabled).toHaveCSS('opacity', '1')
  await expect(page.getByRole('textbox', { name: '活動名稱' })).toHaveCSS('box-shadow', 'none')
  const locked = page.getByRole('switch', { name: '公開反思' })
  expect(await locked.evaluate((element) => getComputedStyle(element, '::before').borderStyle)).toBe('solid')
  await expect(page.locator('[data-slot="skeleton"]').first()).toHaveCSS('animation-name', 'none')

  // Keep the fixed toast visible near the bottom of the full-page snapshot.
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight))
  await page.mouse.move(0, 0)
  await expectNoA11yViolations(page)
  await expectScreenshot(page, 'vedute-components.png')
})

test('Vedute print components interactions, equal card heights, and semantics', async ({ page }) => {
  await page.goto('/__design')
  await expect(page.getByRole('heading', { name: 'Vedute 版畫元件' })).toBeVisible()

  // WorkRow expand/collapse toggle by click and keyboard
  const collapseJustice = page.getByRole('button', { name: '收合 2 個版本' })
  await expect(collapseJustice).toHaveAttribute('aria-expanded', 'true')
  await expect(page.getByText("Justice: What's the Right Thing to Do?")).toBeVisible()
  await collapseJustice.click()
  const expandJustice = page.getByRole('button', { name: '展開 2 個版本' })
  await expect(expandJustice).toHaveAttribute('aria-expanded', 'false')
  await expect(page.getByText("Justice: What's the Right Thing to Do?")).toHaveCount(0)

  const expandRepublic = page.getByRole('button', { name: '展開 1 個版本' })
  await expandRepublic.focus()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: '收合 1 個版本' })).toHaveAttribute('aria-expanded', 'true')
  await expect(page.getByText('柏拉圖')).toBeVisible()

  // PlateCards in the same grid row share equal height on tablet/desktop
  const grid = page.getByTestId('plate-card-grid')
  const cards = grid.locator('[data-vedute-plate-card]')
  await expect(cards).toHaveCount(3)
  const firstBox = (await cards.nth(0).boundingBox())!
  const secondBox = (await cards.nth(1).boundingBox())!
  if (Math.abs(firstBox.y - secondBox.y) < 4) {
    expect(Math.abs(firstBox.height - secondBox.height)).toBeLessThanOrEqual(1)
  }

  // StackBar segments follow The No Right-or-Wrong Rule (caeruleum -> ochra -> aerugo -> neutral)
  const distribution = page.getByRole('group', { name: '為救五人該犧牲一人嗎 論點分布' })
  const tones = await distribution.locator('[data-vedute-segment]').evaluateAll((els) =>
    els.map((el) => el.getAttribute('data-tone')),
  )
  expect(tones).toEqual(['caeruleum', 'ochra', 'aerugo', 'neutral'])

  // Compact StageScale shows only the current stage label
  await expect(page.locator('[data-vedute-stage-scale][data-variant="compact"]').first()).toContainText('釐清')

  // PlateCardSkeleton -> PlateCard transition should not shift content below it
  const swapRegion = page.getByTestId('plate-card-swap')
  const belowHeading = page.getByRole('heading', { name: '書目與共用書庫' })
  const getDocTop = (el: Element) => el.getBoundingClientRect().top + window.scrollY
  const skeletonCard = swapRegion.locator('[data-vedute-plate-card-skeleton]')
  const skeletonHeight = (await skeletonCard.boundingBox())!.height
  const beforeY = await belowHeading.evaluate(getDocTop)
  await page.getByRole('button', { name: '模擬載入完成' }).click()
  const loadedCard = swapRegion.locator('[data-vedute-plate-card]')
  await expect(loadedCard).toBeVisible()
  const afterY = await belowHeading.evaluate(getDocTop)
  const loadedHeight = (await loadedCard.boundingBox())!.height
  expect(
    Math.abs(loadedHeight - skeletonHeight),
    `card height changed from ${skeletonHeight}px to ${loadedHeight}px`,
  ).toBeLessThanOrEqual(1)
  expect(
    Math.abs(afterY - beforeY),
    `below content shifted by ${afterY - beforeY}px (skeleton=${skeletonHeight}, card=${loadedHeight})`,
  ).toBeLessThanOrEqual(1)
  await page.getByRole('button', { name: '重置骨架' }).click()
  await expect(loadedCard).toHaveCount(0)

  // No horizontal overflow across mobile, tablet, or desktop viewports
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

  await expectNoA11yViolations(page)
})

// Tailwind v4 preflight resets <button> to `cursor: default`; Vedute controls set their own (prototypes/c-interior/shared.css .btn / .tab).
test('Vedute clickable controls show a pointer cursor and disabled ones show not-allowed', async ({ page }) => {
  await page.goto('/__design')
  const pointer = [
    page.getByRole('button', { name: '建立活動', exact: true }),
    page.getByRole('button', { name: '取消', exact: true }),
    page.getByRole('button', { name: '編輯', exact: true }),
    page.getByRole('navigation', { name: '教室頁碼' }).getByRole('button', { name: '第 2 頁' }),
    page.getByRole('tab', { name: '進行中' }),
    page.getByRole('switch', { name: '活動通知' }),
    page.getByText('活動通知', { exact: true }),
    page.getByRole('combobox', { name: '教室' }),
  ]
  for (const control of pointer) await expect(control).toHaveCSS('cursor', 'pointer')

  const notAllowed = [
    page.getByRole('button', { name: '儲存中…' }),
    page.getByRole('navigation', { name: '教室頁碼' }).getByRole('button', { name: '上一頁' }),
    page.getByRole('switch', { name: '公開反思' }),
    page.getByText('公開反思', { exact: true }),
  ]
  for (const control of notAllowed) await expect(control).toHaveCSS('cursor', 'not-allowed')
})
