import type { Feature } from '@/app/feature'
import { PersonalDashboardPage } from '@/features/dashboard/personal-dashboard-page'

// 班上論點分布是教師功能，路由在 features/teacher
export const dashboardFeature: Feature = {
  routes: [{ path: '/dashboard', element: <PersonalDashboardPage />, access: 'member' }],
  nav: [{ to: '/dashboard', labelKey: 'dashboard.nav.me', access: 'member' }],
}
