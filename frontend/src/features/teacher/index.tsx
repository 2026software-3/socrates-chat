import type { Feature } from '@/app/feature'
import { classroomRoutes } from '@/features/classrooms/routes'
import { RosterPage, SummariesPage, TopicsPage } from '@/features/staff'

export const teacherFeature: Feature = {
  routes: [
    ...classroomRoutes('/teacher', 'teacher'),
    { path: '/teacher/topics', element: <TopicsPage />, access: 'teacher' },
    { path: '/teacher/roster', element: <RosterPage />, access: 'teacher' },
    { path: '/teacher/summaries', element: <SummariesPage />, access: 'teacher' },
  ],
  nav: [
    { to: '/teacher/classrooms', labelKey: 'classrooms.nav', access: 'teacher' },
    { to: '/teacher/topics', labelKey: 'teacher.nav.topics', access: 'teacher' },
    { to: '/teacher/roster', labelKey: 'teacher.nav.roster', access: 'teacher' },
    { to: '/teacher/summaries', labelKey: 'teacher.nav.summaries', access: 'teacher' },
  ],
}
