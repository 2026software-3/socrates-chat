import type { Feature } from '@/app/feature'
import { AccountPage } from '@/features/account/account-page'

// 沒有 nav：從側欄底部的名稱進入（app/layout.tsx）
export const accountFeature: Feature = {
  routes: [{ path: '/account', element: <AccountPage />, access: 'any' }],
  nav: [],
}
