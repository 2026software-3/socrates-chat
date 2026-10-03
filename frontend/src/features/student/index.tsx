import type { Feature } from '@/app/feature'
import { AvailablePage } from '@/features/student/available-page'

export const studentFeature: Feature = {
  routes: [{ path: '/available', element: <AvailablePage />, access: 'member' }],
  nav: [{ to: '/available', labelKey: 'student.nav.available', access: 'member' }],
}
