import type { Feature } from '@/app/feature'
import { RosterPage } from '@/features/teacher/roster-page'
import { SummariesPage } from '@/features/teacher/summaries-page'
import { TopicsPage } from '@/features/teacher/topics-page'

export const teacherFeature: Feature = {
  routes: [
    { path: '/teacher/topics', element: <TopicsPage />, access: 'teacher' },
    { path: '/teacher/roster', element: <RosterPage />, access: 'teacher' },
    { path: '/teacher/summaries', element: <SummariesPage />, access: 'teacher' },
  ],
  nav: [
    { to: '/teacher/topics', labelKey: 'teacher.nav.topics', access: 'teacher' },
    { to: '/teacher/roster', labelKey: 'teacher.nav.roster', access: 'teacher' },
    { to: '/teacher/summaries', labelKey: 'teacher.nav.summaries', access: 'teacher' },
  ],
}
