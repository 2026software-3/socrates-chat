import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

test.describe('admin observability screens (#464, #581, metrics)', () => {
  test('diagnostics page: filters, expandable details, responsive layout, axe, and screenshots', async ({
    page,
    loginAs,
  }, testInfo) => {
    await loginAs('admin')
    await page.goto('/admin/diagnostics')

    await expect(page.getByRole('heading', { level: 1, name: '診斷紀錄' })).toBeVisible()
    const events =
      testInfo.project.name === 'desktop'
        ? page.getByRole('table', { name: '診斷事件' })
        : page.getByRole('list', { name: '診斷事件' })
    const opposite =
      testInfo.project.name === 'desktop'
        ? page.getByRole('list', { name: '診斷事件' })
        : page.getByRole('table', { name: '診斷事件' })
    await expect(events).toBeVisible()
    await expect(opposite).toHaveCount(0)
    await expect(events.getByText('頻率限制')).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'admin-diagnostics.png')

    // Interactive filter + details expand
    await page.getByLabel('類型').selectOption('ai')
    await page.getByRole('button', { name: '查詢' }).click()
    await expect(page.getByText('對話回覆')).toBeVisible()
    await expect(page.getByText('密碼登入')).toHaveCount(0)

    await page.getByRole('button', { name: '詳情' }).first().click()
    await expect(page.getByText('synthetic-model')).toBeVisible()

    // Empty filtered state
    await page.getByLabel('Request ID').fill('99999999-9999-4999-8999-999999999999')
    await page.getByRole('button', { name: '查詢' }).click()
    await expect(page.getByText('沒有符合條件的失敗紀錄。')).toBeVisible()

    // Error state with request ID + traceparent and retry recovery
    const diagTraceparent = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
    await page.evaluate(() => localStorage.setItem('socrates:e2e-fail-once', '/api/admin/diagnostics'))
    await page.getByRole('button', { name: '清除篩選' }).click()
    await expect(page.getByText(/e2e-err-id/)).toBeVisible()
    await expect(page.getByText(new RegExp(diagTraceparent))).toBeVisible()
    await page.getByRole('button', { name: '重試' }).click()
    await expect(page.getByText('密碼登入')).toBeVisible()
  })

  test('services page: unhealthy-first list, status filter, check now, history toggle, axe, and screenshots', async ({
    page,
    loginAs,
  }) => {
    await loginAs('admin')
    await page.goto('/admin/services')

    await expect(page.getByRole('heading', { level: 1, name: '服務' })).toBeVisible()
    await expect(page.getByText('語音合成')).toBeVisible()
    await expect(page.getByText('異常 · 逾時')).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'admin-services.png')

    // Filter + expand history + check now
    await page.getByRole('button', { name: '異常' }).click()
    await expect(page.getByText('語音合成')).toBeVisible()
    await expect(page.getByText('PostgreSQL')).toHaveCount(0)

    await page.getByRole('button', { name: '最近的狀態變化' }).click()
    await expect(page.getByText('逾時', { exact: true })).toBeVisible()

    await page.getByRole('button', { name: '立即檢查 語音合成' }).click()
    await expect(page.getByText('沒有符合這個狀態的服務。')).toBeVisible()
    await page.getByRole('button', { name: '全部' }).click()
    await expect(page.getByText('14 毫秒')).toBeVisible()
  })

  test('metrics page: live gauges, HTTP distribution, raw exposition toggle, axe, and screenshots', async ({
    page,
    loginAs,
  }, testInfo) => {
    await loginAs('admin')
    await page.goto('/admin/metrics')

    await expect(page.getByRole('heading', { level: 1, name: '系統指標' })).toBeVisible()
    await expect(page.getByText('版本 e2e-build')).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    if (testInfo.project.name === 'desktop') {
      await expect(page.getByRole('table', { name: '各路由請求統計' })).toBeVisible()
    } else {
      await expect(page.getByRole('list', { name: '各路由請求統計' })).toBeVisible()
      await expect(page.getByRole('table', { name: '各路由請求統計' })).toHaveCount(0)
    }

    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'admin-metrics.png')

    await page.getByRole('button', { name: '檢視原始 Prometheus 文字' }).click()
    await expect(page.getByText(/socrates_http_requests_total/)).toBeVisible()

    // Error state with request ID + traceparent and retry recovery
    const metricsTraceparent = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
    await page.evaluate(() => localStorage.setItem('socrates:e2e-fail-once', '/api/admin/metrics'))
    await page.getByRole('button', { name: '重新整理' }).click()
    await expect(page.getByText(/e2e-err-id/)).toBeVisible()
    await expect(page.getByText(new RegExp(metricsTraceparent))).toBeVisible()
    await page.getByRole('button', { name: '重試' }).click()
    await expect(page.getByText('版本 e2e-build')).toBeVisible()
  })
})
