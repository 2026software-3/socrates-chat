import { defineConfig, devices } from '@playwright/test'

/**
 * 前端點擊測試、視覺回歸與無障礙檢查（S-11.6）。
 * 跑在 `vite --mode e2e` 上：瀏覽器內的 MSW 代替後端（src/test/browser.ts），不需要後端與資料庫。
 * 基準截圖只認 Linux（官方 Playwright Docker image）；怎麼跑、怎麼更新見 frontend/README.md。
 */
const PORT = 5174
const CI = !!process.env.CI

export default defineConfig({
  testDir: './tests/e2e',
  // 基準截圖：tests/e2e/__screenshots__/<spec>/<名稱>-<寬度>-<平台>.png；只有 -linux 進版控
  snapshotPathTemplate: '{testDir}/__screenshots__/{testFilePath}/{arg}-{projectName}-{platform}{ext}',
  outputDir: './test-results',
  fullyParallel: true,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  // 本機（非 Linux）第一次跑會寫出自己平台的基準（.gitignore 已排除），CI 只比對、不寫
  updateSnapshots: CI ? 'none' : 'missing',
  reporter: CI ? [['github'], ['html', { open: 'never' }]] : [['list'], ['html', { open: 'never' }]],
  expect: {
    toHaveScreenshot: { animations: 'disabled', caret: 'hide', scale: 'css' },
  },
  use: {
    baseURL: `http://localhost:${PORT}`,
    locale: 'zh-TW',
    timezoneId: 'Asia/Taipei',
    colorScheme: 'light',
    reducedMotion: 'reduce',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  // S-11.6：固定三種寬度、只跑 Chromium
  projects: [
    { name: 'mobile', use: { ...devices['Desktop Chrome'], viewport: { width: 375, height: 812 } } },
    { name: 'tablet', use: { ...devices['Desktop Chrome'], viewport: { width: 768, height: 1024 } } },
    { name: 'desktop', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
  ],
  webServer: {
    command: `npx vite --mode e2e --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !CI,
    timeout: 60_000,
  },
})
