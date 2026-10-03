import type { Feature } from '@/app/feature'
import { ActivitiesPage } from '@/features/teacher/activities-page'
import { RosterPage } from '@/features/teacher/roster-page'
import { SummariesPage } from '@/features/teacher/summaries-page'

export const teacherFeature: Feature = {
  routes: [
    { path: '/teacher/activities', element: <ActivitiesPage />, access: 'teacher' },
    { path: '/teacher/roster', element: <RosterPage />, access: 'teacher' },
    { path: '/teacher/summaries', element: <SummariesPage />, access: 'teacher' },
  ],
  nav: [
    { to: '/teacher/activities', labelKey: 'teacher.nav.activities', access: 'teacher' },
    { to: '/teacher/roster', labelKey: 'teacher.nav.roster', access: 'teacher' },
    { to: '/teacher/summaries', labelKey: 'teacher.nav.summaries', access: 'teacher' },
  ],
}
