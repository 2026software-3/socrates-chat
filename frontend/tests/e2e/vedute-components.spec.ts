import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

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
  await expect(field).toHaveCSS('background-size', '100% 2px')

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
    await button.click()
    await expect(button).toHaveAttribute('data-pressed', '')
    await expect(button).not.toHaveCSS('background-color', hovered)
    await expect(button).toHaveCSS('translate', 'none')
    await expect(button).toHaveCSS('scale', 'none')
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
    await control.click()
    await expect(control).toHaveAttribute('data-pressed', '')
    await expect(control).toHaveCSS('color', pressedPageColor)
    await expect(control).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
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

  const icon = page.getByRole('button', { name: '編輯' })
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
  const hoverBackground = await enabledOption.evaluate((el) => getComputedStyle(el).backgroundColor)
  const archived = page.getByRole('option', { name: '已封存的教室' })
  await expect(archived).toBeDisabled()
  const disabledBackground = await archived.evaluate((el) => getComputedStyle(el).backgroundColor)
  expect(disabledBackground).not.toBe('rgba(0, 0, 0, 0)')
  await archived.hover()
  await expect(archived).toHaveCSS('cursor', 'not-allowed')
  await expect(archived).not.toHaveCSS('background-color', hoverBackground)
  await expect(archived).toHaveCSS('background-color', disabledBackground)
  await expect(archived).toHaveCSS('text-decoration-line', 'line-through')
  await archived.click({ force: true })
  await expect(page.getByRole('listbox')).toBeVisible()
  await page.keyboard.press('Escape')

  await page.getByRole('button', { name: '顯示儲存失敗' }).click()
  const notice = page.locator('.vedute-toast')
  await expect(notice).toHaveCSS('animation-name', 'vedute-toast-enter')
  await page.getByRole('button', { name: '重試' }).click()
  await expect(notice).toHaveCSS('animation-name', 'vedute-toast-exit')
  await expect(notice).toHaveCSS('animation-duration', '0.15s')
  await expect(notice).toContainText('儲存失敗，請重試。')
  await expect(notice).toHaveCount(1)
  await expect(notice).toContainText('討論活動已儲存。')
  await expect(notice).toHaveCSS('animation-name', 'vedute-toast-enter')
  await page.getByRole('button', { name: '關閉通知' }).click()
  await expect(notice).toHaveCSS('animation-name', 'vedute-toast-exit')
  await expect(notice).toHaveCount(0)
})

test('Vedute visual baseline and accessibility', async ({ page }) => {
  await page.goto('/__design')
  await page.getByRole('button', { name: '顯示儲存失敗' }).click()
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
