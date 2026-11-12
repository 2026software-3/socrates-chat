import { Navigate, useLocation } from 'react-router'
import type { Feature } from '@/app/feature'

/** 個人儀表板已併入帳號頁（S-18）：舊網址一律導向 `/account`，查詢字串保留（例如 Google 連結結果）。 */
function LegacyDashboardRedirect() {
  const { search } = useLocation()
  return <Navigate to={{ pathname: '/account', search }} replace />
}

// 班上論點分布是教師功能，路由在 features/teacher；學生的儀表板內容在 features/account
export const dashboardFeature: Feature = {
  routes: [{ path: '/dashboard', element: <LegacyDashboardRedirect />, access: 'any' }],
  nav: [],
}
