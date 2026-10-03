import type { Feature } from '@/app/feature'
import { TeachersPage } from '@/features/admin/teachers-page'

export const adminFeature: Feature = {
  routes: [{ path: '/admin/teachers', element: <TeachersPage />, access: 'admin' }],
  nav: [{ to: '/admin/teachers', labelKey: 'admin.nav.teachers', access: 'admin' }],
}
