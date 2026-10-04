import type { Feature } from '@/app/feature'
import { TopicsPage } from '@/features/admin/topics-page'
import { ClassDashboardPage } from '@/features/dashboard/class-dashboard-page'
import { ActivitiesPage } from '@/features/teacher/activities-page'
import { RosterPage } from '@/features/teacher/roster-page'
import { SummariesPage } from '@/features/teacher/summaries-page'

export const teacherFeature: Feature = {
  routes: [
    { path: '/teacher/dashboard', element: <ClassDashboardPage />, access: 'teacher' },
    { path: '/teacher/activities', element: <ActivitiesPage />, access: 'teacher' },
    { path: '/teacher/topics', element: <TopicsPage base="/api/topics" />, access: 'teacher' },
    { path: '/teacher/roster', element: <RosterPage />, access: 'teacher' },
    { path: '/teacher/summaries', element: <SummariesPage />, access: 'teacher' },
  ],
  nav: [
    { to: '/teacher/dashboard', labelKey: 'teacher.nav.dashboard', access: 'teacher' },
    { to: '/teacher/activities', labelKey: 'teacher.nav.activities', access: 'teacher' },
    { to: '/teacher/topics', labelKey: 'teacher.nav.topics', access: 'teacher' },
    { to: '/teacher/roster', labelKey: 'teacher.nav.roster', access: 'teacher' },
    { to: '/teacher/summaries', labelKey: 'teacher.nav.summaries', access: 'teacher' },
  ],
}
