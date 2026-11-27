import type { Feature } from '@/app/feature'
import { classroomRoutes } from '@/features/classrooms/routes'
import { BooksPage, ClassroomAiSettingsPage, KeysPage, RosterPage, SummariesPage, TopicsPage } from '@/features/staff'

export const teacherFeature: Feature = {
  routes: [
    { path: '/teacher/books', element: <BooksPage />, access: 'teacher' },
    ...classroomRoutes('/teacher', 'teacher'),
    { path: '/teacher/classrooms/:classroomId/ai', element: <ClassroomAiSettingsPage />, access: 'teacher' },
    { path: '/teacher/keys', element: <KeysPage />, access: 'teacher' },
    { path: '/teacher/topics', element: <TopicsPage />, access: 'teacher' },
    { path: '/teacher/roster', element: <RosterPage />, access: 'teacher' },
    { path: '/teacher/summaries', element: <SummariesPage />, access: 'teacher' },
  ],
  nav: [
    { to: '/teacher/classrooms', labelKey: 'classrooms.nav', access: 'teacher' },
    { to: '/teacher/keys', labelKey: 'keys.nav', access: 'teacher' },
    { to: '/teacher/topics', labelKey: 'teacher.nav.topics', access: 'teacher' },
    { to: '/teacher/books', labelKey: 'books.nav', access: 'teacher' },
    { to: '/teacher/roster', labelKey: 'teacher.nav.roster', access: 'teacher' },
    { to: '/teacher/summaries', labelKey: 'teacher.nav.summaries', access: 'teacher' },
  ],
}
