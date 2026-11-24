import { E2E_ACCOUNTS, E2E_PASSWORD } from '../../src/test/e2e-accounts'
import { expect, expectNoA11yViolations, expectScreenshot, test } from './fixtures'

test.describe('student login → home', () => {
  test('shows the login page without accessibility violations', async ({ page }) => {
    await page.goto('/login')
    await expect(page.getByRole('button', { name: '使用 Google 登入' })).toBeVisible()
    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'login.png')
  })

  test('rejects a wrong password with a translated message', async ({ page }) => {
    await page.goto('/login')
    await page.getByLabel('學號或電子郵件').fill(E2E_ACCOUNTS.student)
    await page.getByLabel('密碼').fill(`${E2E_PASSWORD}-wrong`)
    await page.getByRole('button', { name: '登入', exact: true }).click()
    await expect(page.getByRole('alert')).toHaveText('學號、電子郵件或密碼錯誤。')
    await expect(page).toHaveURL(/\/login$/)
  })

  test('lands on the topic picker with the classroom activity', async ({ page, loginAs }) => {
    await loginAs('student')
    await expect(page.getByRole('heading', { level: 1, name: '選擇題目' })).toBeVisible()
    await expect(page.getByText('什麼是正義？')).toBeVisible()
    await expect(page.getByText('美德可以教嗎？')).toBeVisible()
    await expectNoA11yViolations(page)
    await expectScreenshot(page, 'student-home.png')
  })
})

test.describe('other roles', () => {
  test('a teacher lands on 我的教室', async ({ page, loginAs }) => {
    await loginAs('teacher')
    await expect(page).toHaveURL(/\/teacher\/classrooms$/)
    await expect(page.getByRole('heading', { level: 1, name: '我的教室' })).toBeVisible()
    await expect(page.getByText('合成 A 班')).toBeVisible()
  })

  test('an admin lands on the admin home', async ({ page, loginAs }) => {
    await loginAs('admin')
    await expect(page).toHaveURL(/\/admin$/)
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
  })
})
