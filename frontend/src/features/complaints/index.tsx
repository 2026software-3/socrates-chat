import type { Feature } from '@/app/feature'
import { ComplaintPage } from '@/features/complaints/complaint-page'
import { AdminComplaintsPage } from '@/features/staff'

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
