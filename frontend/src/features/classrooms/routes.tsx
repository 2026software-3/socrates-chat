import type { AppRoute } from '@/app/feature'
import {
  ClassroomActivityPage,
  ClassroomPage,
  ClassroomStudentPage,
  ClassroomStudentsPage,
} from '@/features/classrooms/classroom-pages'
import { ClassroomsPage } from '@/features/classrooms/classrooms-page'

/** 教室 → 活動／學生 的階層路由；教師在 `/teacher`、管理者在 `/admin`。 */
export function classroomRoutes(prefix: '/teacher' | '/admin', access: AppRoute['access']): AppRoute[] {
  const base = `${prefix}/classrooms`
  return [
    { path: base, element: <ClassroomsPage />, access },
    { path: `${base}/:classroomId`, element: <ClassroomPage />, access },
    { path: `${base}/:classroomId/students`, element: <ClassroomStudentsPage />, access },
    { path: `${base}/:classroomId/students/:studentId`, element: <ClassroomStudentPage />, access },
    { path: `${base}/:classroomId/activities/:activityId`, element: <ClassroomActivityPage />, access },
  ]
}
