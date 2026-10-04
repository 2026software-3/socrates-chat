import type { Feature, NavItem } from '@/app/feature'
import { AdminHome } from '@/features/admin/admin-home'
import { TeachersPage } from '@/features/admin/teachers-page'
import { TopicsPage } from '@/features/admin/topics-page'
import { RosterPage } from '@/features/teacher/roster-page'

export const adminFeature: Feature = {
  routes: [
    { path: '/admin', element: <AdminHome />, access: 'admin' },
    { path: '/admin/teachers', element: <TeachersPage />, access: 'admin' },
    { path: '/admin/roster', element: <RosterPage />, access: 'admin' },
    { path: '/admin/topics', element: <TopicsPage />, access: 'admin' },
  ],
  // 同時具備教師／學生角色的管理者沿用一般導覽，只多這兩項
  nav: [
    { to: '/admin/teachers', labelKey: 'admin.nav.teachers', access: 'admin' },
    { to: '/admin/topics', labelKey: 'admin.nav.topics', access: 'admin' },
  ],
}

/** 純管理者（沒有教師／學生角色）的維運導覽：只有維運頁面。 */
export const opsNav: NavItem[] = [
  { to: '/admin', labelKey: 'admin.nav.home', access: 'admin' },
  { to: '/admin/teachers', labelKey: 'admin.nav.teachers', access: 'admin' },
  { to: '/admin/roster', labelKey: 'admin.nav.roster', access: 'admin' },
  { to: '/admin/topics', labelKey: 'admin.nav.topics', access: 'admin' },
]
