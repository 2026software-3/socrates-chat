import type { Feature, NavItem } from '@/app/feature'
import { classroomRoutes } from '@/features/classrooms/routes'
import {
  AdminAnnouncementsPage,
  AdminHome,
  AdminPartsPage,
  AdminTasksPage,
  DiagnosticsPage,
  RosterPage,
  TeachersPage,
  TopicsPage,
} from '@/features/staff'

export const adminFeature: Feature = {
  routes: [
    { path: '/admin', element: <AdminHome />, access: 'admin' },
    ...classroomRoutes('/admin', 'admin'),
    { path: '/admin/teachers', element: <TeachersPage />, access: 'admin' },
    { path: '/admin/roster', element: <RosterPage />, access: 'admin' },
    { path: '/admin/topics', element: <TopicsPage />, access: 'admin' },
    { path: '/admin/diagnostics', element: <DiagnosticsPage />, access: 'admin' },
    { path: '/admin/tasks', element: <AdminTasksPage />, access: 'admin' },
    { path: '/admin/parts', element: <AdminPartsPage />, access: 'admin' },
    { path: '/admin/announcements', element: <AdminAnnouncementsPage />, access: 'admin' },
  ],
  // 同時具備教師／學生角色的管理者沿用一般導覽，另有管理者專屬頁面。
  nav: [
    { to: '/admin/teachers', labelKey: 'admin.nav.teachers', access: 'admin' },
    { to: '/admin/diagnostics', labelKey: 'admin.nav.diagnostics', access: 'admin' },
    { to: '/admin/tasks', labelKey: 'tasks.admin', access: 'admin' },
    { to: '/admin/parts', labelKey: 'parts.admin', access: 'admin' },
    { to: '/admin/announcements', labelKey: 'admin.nav.announcements', access: 'admin' },
  ],
}

/** 純管理者（沒有教師／學生角色）的維運導覽：只有維運頁面。 */
export const opsNav: NavItem[] = [
  { to: '/admin', labelKey: 'admin.nav.home', access: 'admin' },
  { to: '/admin/classrooms', labelKey: 'classrooms.nav', access: 'admin' },
  { to: '/admin/teachers', labelKey: 'admin.nav.teachers', access: 'admin' },
  { to: '/admin/roster', labelKey: 'admin.nav.roster', access: 'admin' },
  { to: '/admin/topics', labelKey: 'admin.nav.topics', access: 'admin' },
  { to: '/admin/diagnostics', labelKey: 'admin.nav.diagnostics', access: 'admin' },
  { to: '/admin/tasks', labelKey: 'tasks.admin', access: 'admin' },
  { to: '/admin/parts', labelKey: 'parts.admin', access: 'admin' },
  { to: '/admin/announcements', labelKey: 'admin.nav.announcements', access: 'admin' },
  { to: '/admin/complaints', labelKey: 'complaints.admin.nav', access: 'admin' },
]
