import type { Feature } from '@/app/feature'
import { TeachersPage } from '@/features/admin/teachers-page'
import { TopicsPage } from '@/features/admin/topics-page'

export const adminFeature: Feature = {
  routes: [
    { path: '/admin/teachers', element: <TeachersPage />, access: 'admin' },
    { path: '/admin/topics', element: <TopicsPage />, access: 'admin' },
  ],
  nav: [
    { to: '/admin/teachers', labelKey: 'admin.nav.teachers', access: 'admin' },
    { to: '/admin/topics', labelKey: 'admin.nav.topics', access: 'admin' },
  ],
}
