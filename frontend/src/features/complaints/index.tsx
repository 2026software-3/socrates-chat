import type { Feature } from '@/app/feature'
import { AdminComplaintsPage } from '@/features/complaints/admin-complaints-page'
import { ComplaintPage } from '@/features/complaints/complaint-page'

export const complaintsFeature: Feature = {
  routes: [
    { path: '/complaint', element: <ComplaintPage />, access: 'reporter' },
    { path: '/admin/complaints', element: <AdminComplaintsPage />, access: 'admin' },
  ],
  nav: [
    { to: '/complaint', labelKey: 'complaints.nav', access: 'reporter' },
    { to: '/admin/complaints', labelKey: 'complaints.admin.nav', access: 'admin' },
  ],
}
