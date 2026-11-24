/**
 * Playwright 假後端的合成帳號（src/test/browser.ts）與測試 fixture（tests/e2e/fixtures.ts）共用。
 * 這個檔案不能 import msw：Playwright 在 Node 端也會讀它。
 */
export type E2eRole = 'student' | 'teacher' | 'admin'

export const E2E_ACCOUNTS: Record<E2eRole, string> = {
  student: 'student@example.com',
  teacher: 'teacher@example.com',
  admin: 'admin@example.com',
}

/** 三個合成帳號共用的密碼。 */
export const E2E_PASSWORD = 'e2e-password'

/** 假的登入狀態存在 localStorage，重新整理頁面仍然是登入狀態（模擬 sid cookie）。 */
export const E2E_SESSION_KEY = 'e2e-session-role'
